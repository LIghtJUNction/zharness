use std::cell::{Cell, RefCell};
use zharness::{
    Error,
    hooks::{Decision, HookResult},
    lifecycle::{
        Correlation, Event, Fault, Gate, Interceptor, Lifecycle, NoGate, NoObserver, Notice,
        Observer, Outcome, Payload, Phase, Proposal, Visibility, domains::*,
    },
};

const IDS: Correlation = Correlation {
    conversation: 1,
    run: 7,
    request: Some(2),
    message: Some(3),
    agent: Some(4),
    attempt: 1,
};
#[derive(Default)]
struct Records {
    phases: Vec<Phase>,
    serials: Vec<u64>,
    exposed: Vec<bool>,
}
impl<'a> Observer<conversation::Event<'a>> for Records {
    fn observe(&mut self, n: Notice<conversation::Event<'a>>) {
        assert_eq!(n.proposal.correlation, IDS);
        self.phases.push(n.phase);
        self.serials.push(n.proposal.serial);
        self.exposed
            .push(n.proposal.event.payload.as_text().is_some());
    }
}
struct Policy(Decision);
impl<E: Event> Gate<E> for Policy {
    fn before(&mut self, _: Proposal<E>) -> Decision {
        self.0
    }
}
fn event() -> conversation::Event<'static> {
    conversation::Event::new(conversation::Action::AcceptUser)
        .with_payload(Payload::utf8("private text"))
}

#[test]
fn gates_pause_deny_and_error_paths_bracket_actual_operations() {
    let calls = Cell::new(0);
    let mut bus = Lifecycle::new(1, Policy(Decision::Pause), Records::default());
    assert!(matches!(
        bus.run(IDS, event(), |_| {
            calls.set(1);
            Ok(())
        })
        .unwrap(),
        HookResult::Paused
    ));
    bus.gate_mut().0 = Decision::Deny;
    assert_eq!(
        bus.run(IDS, event(), |_| {
            calls.set(2);
            Ok(())
        }),
        Err(Fault::Core(Error::Denied))
    );
    assert_eq!(calls.get(), 0);
    bus.gate_mut().0 = Decision::Allow;
    let result: Result<HookResult<()>, Fault> = bus.run(IDS, event(), |_| {
        calls.set(3);
        Err(Fault::Application(17))
    });
    assert_eq!(result, Err(Fault::Application(17)));
    assert_eq!(calls.get(), 3);
    assert_eq!(
        bus.observer().phases,
        [
            Phase::Paused,
            Phase::Denied,
            Phase::Started,
            Phase::Finished(Outcome::Failed(Fault::Application(17)))
        ]
    );
    assert_eq!(bus.observer().serials, [1, 2, 3, 3]);
    assert!(bus.observer().exposed.iter().all(|b| !b));
}
#[test]
fn payload_debug_and_observer_defaults_do_not_leak_text() {
    let p = Payload::utf8("SECRET_NOT_FOR_LOGS");
    assert!(!format!("{p:?}").contains("SECRET_NOT_FOR_LOGS"));
    assert!(p.as_text().unwrap().eq_str("SECRET_NOT_FOR_LOGS"));
    assert!(p.redacted().as_text().is_none());
    assert_eq!(p.len(), p.redacted().len());
    let mut bus =
        Lifecycle::new(1, NoGate, Records::default()).with_visibility(Visibility::Content);
    let _ = bus.run(IDS, event(), |_| Ok(())).unwrap();
    assert_eq!(bus.observer().exposed, [true, true]);
}
#[test]
fn split_spans_allow_out_of_order_finish_but_reject_duplicates_and_wrong_epoch() {
    let mut bus = Lifecycle::new(10, NoGate, Records::default());
    let mut other = Lifecycle::new(11, NoGate, Records::default());
    let HookResult::Applied(mut a) = bus.begin(IDS, event()).unwrap() else {
        panic!()
    };
    let HookResult::Applied(mut b) = bus.begin(IDS, event()).unwrap() else {
        panic!()
    };
    assert_eq!(
        other.finish(&mut a, Outcome::Succeeded),
        Err(Fault::Core(Error::Stale))
    );
    assert!(!a.is_finished());
    assert!(other.observer().phases.is_empty());
    bus.finish(&mut b, Outcome::Succeeded).unwrap();
    bus.finish(&mut a, Outcome::Cancelled).unwrap();
    assert_eq!(
        bus.finish(&mut a, Outcome::Succeeded),
        Err(Fault::Core(Error::Stale))
    );
    assert_eq!(bus.observer().serials, [1, 2, 2, 1]);
}
#[test]
fn interceptors_are_ordered_copy_on_proposal_and_failure_stops_execution() {
    struct Rewrite;
    impl<'a> Interceptor<conversation::Event<'a>> for Rewrite {
        fn intercept(&mut self, e: &mut conversation::Event<'a>) -> Result<(), Fault> {
            e.payload = Payload::utf8("filtered");
            Ok(())
        }
    }
    struct Reject;
    impl<'a> Interceptor<conversation::Event<'a>> for Reject {
        fn intercept(&mut self, e: &mut conversation::Event<'a>) -> Result<(), Fault> {
            assert!(e.payload.as_text().unwrap().eq_str("filtered"));
            Err(Fault::Application(9))
        }
    }
    let input = event();
    let mut bus = Lifecycle::new(1, NoGate, Records::default());
    let result = bus
        .run_intercepted(IDS, input, &mut Rewrite, |e| {
            Ok(e.payload.as_text().unwrap().eq_str("filtered"))
        })
        .unwrap();
    assert_eq!(result, HookResult::Applied(true));
    assert!(input.payload.as_text().unwrap().eq_str("private text"));
    let result: Result<HookResult<()>, _> =
        bus.run_intercepted(IDS, input, &mut (Rewrite, Reject), |_| {
            panic!("must not run")
        });
    assert_eq!(result, Err(Fault::Application(9)));
    assert_eq!(
        bus.observer().phases.last(),
        Some(&Phase::Rejected(Fault::Application(9)))
    );
}
#[test]
fn tuple_gates_short_circuit_and_observers_keep_order() {
    struct G<'a>(&'a RefCell<Vec<u8>>, u8, Decision);
    impl<E: Event> Gate<E> for G<'_> {
        fn before(&mut self, _: Proposal<E>) -> Decision {
            self.0.borrow_mut().push(self.1);
            self.2
        }
    }
    struct O<'a>(&'a RefCell<Vec<u8>>, u8);
    impl<E: Event> Observer<E> for O<'_> {
        fn observe(&mut self, _: Notice<E>) {
            self.0.borrow_mut().push(self.1);
        }
    }
    let log = RefCell::new(Vec::new());
    let mut gates = (G(&log, 1, Decision::Pause), G(&log, 2, Decision::Allow));
    let mut observers = (O(&log, 3), O(&log, 4));
    let mut bus = Lifecycle::new(1, &mut gates, &mut observers);
    assert!(matches!(
        bus.run(IDS, event(), |_| panic!()).unwrap(),
        HookResult::<()>::Paused
    ));
    assert_eq!(*log.borrow(), [1, 3, 4]);
}
#[test]
fn every_group_is_independent_and_runs_real_closures() {
    let mut bus = Lifecycle::new(1, NoGate, NoObserver);
    let calls = Cell::new(0);
    macro_rules! exercise { ($($name:ident),+) => { $(for action in $name::Action::ALL {
        let e = $name::Event::new(*action);
        let _ = bus.run(IDS, e, |_| { calls.set(calls.get() + 1); Ok(()) }).unwrap();
        bus.notify(IDS, e).unwrap();
    })+ }; }
    exercise!(
        conversation,
        prompt,
        request,
        tool,
        context,
        agent,
        persistence,
        runtime
    );
    for action in model::Action::ALL {
        let e = model::Event {
            action: *action,
            previous: None,
            proposed: model::Configuration::new("m"),
            revision: 0,
        };
        let _ = bus
            .run(IDS, e, |_| {
                calls.set(calls.get() + 1);
                Ok(())
            })
            .unwrap();
    }
    for action in response::Action::ALL {
        let _ = bus
            .run(IDS, response::Event::new(*action), |_| {
                calls.set(calls.get() + 1);
                Ok(())
            })
            .unwrap();
    }
    assert!(calls.get() >= 90);
}
