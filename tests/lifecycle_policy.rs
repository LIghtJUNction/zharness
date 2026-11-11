use std::cell::Cell;
use zharness::{
    Error,
    conversation::Conversation,
    hooks::{Decision, HookResult},
    lifecycle::{
        Correlation, Fault, Gate, Interceptor, Lifecycle, NoGate, NoObserver, Notice, Observer,
        Outcome, Payload, Phase, Proposal,
        domains::{conversation, model, persistence, prompt, response},
    },
    settings::ModelSettings,
};

#[test]
fn message_interception_cannot_change_identity_and_gate_checks_accepted_content() {
    struct Redact;
    impl<'a> Interceptor<Payload<'a>> for Redact {
        fn intercept(&mut self, p: &mut Payload<'a>) -> Result<(), Fault> {
            *p = Payload::utf8("filtered user text");
            Ok(())
        }
    }
    struct Policy;
    impl<'a> Gate<conversation::Event<'a>> for Policy {
        fn before(&mut self, p: Proposal<conversation::Event<'a>>) -> Decision {
            if p.event.action == conversation::Action::AcceptUser {
                assert_eq!(p.correlation.conversation, 9);
                assert_eq!(p.correlation.message, Some(11));
                assert!(
                    p.event
                        .payload
                        .as_text()
                        .unwrap()
                        .eq_str("filtered user text")
                );
            }
            Decision::Allow
        }
    }
    let ids = Correlation {
        conversation: 9,
        ..Correlation::default()
    };
    let original = Payload::utf8("sensitive input");
    let mut chat = Conversation::new(ids);
    let mut bus = Lifecycle::new(1, Policy, NoObserver);
    let _ = chat.start(&mut bus, || Ok(())).unwrap();
    let result = chat
        .user_message_intercepted(11, original, &mut Redact, &mut bus, |p| {
            Ok(p.as_text().unwrap().eq_str("filtered user text"))
        })
        .unwrap();
    assert_eq!(result, HookResult::Applied(true));
    assert_eq!(chat.revision(), 1);
    assert!(original.as_text().unwrap().eq_str("sensitive input"));
}
#[test]
fn invalid_final_model_configuration_is_rejected_before_gate_or_mutation() {
    struct Invalid;
    impl<'a> Interceptor<model::Configuration<'a>> for Invalid {
        fn intercept(&mut self, c: &mut model::Configuration<'a>) -> Result<(), Fault> {
            c.model = "";
            Ok(())
        }
    }
    struct Count<'a>(&'a Cell<usize>);
    impl<'a> Gate<model::Event<'a>> for Count<'_> {
        fn before(&mut self, _: Proposal<model::Event<'a>>) -> Decision {
            self.0.set(self.0.get() + 1);
            Decision::Allow
        }
    }
    let calls = Cell::new(0);
    let mut bus = Lifecycle::new(1, Count(&calls), NoObserver);
    let HookResult::Applied(mut config) = ModelSettings::select(
        Correlation::default(),
        model::Configuration::new("old"),
        &mut bus,
    )
    .unwrap() else {
        panic!()
    };
    assert!(
        config
            .configure(model::Configuration::new("new"), &mut Invalid, &mut bus)
            .is_err()
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(config.current().model, "old");
    assert_eq!(config.revision(), 0);
}
#[test]
fn async_operation_receipts_keep_identity_across_out_of_order_completion() {
    #[derive(Default)]
    struct Audit(Vec<(Option<u64>, Outcome)>);
    impl<'a> Observer<persistence::Event<'a>> for Audit {
        fn observe(&mut self, n: Notice<persistence::Event<'a>>) {
            if let Phase::Finished(outcome) = n.phase {
                self.0.push((n.proposal.correlation.request, outcome));
            }
        }
    }
    let mut bus = Lifecycle::new(1, NoGate, Audit::default());
    let event = persistence::Event::new(persistence::Action::Checkpoint);
    let HookResult::Applied(mut a) = bus
        .begin(
            Correlation {
                request: Some(1),
                ..Correlation::default()
            },
            event,
        )
        .unwrap()
    else {
        panic!()
    };
    let HookResult::Applied(mut b) = bus
        .begin(
            Correlation {
                request: Some(2),
                ..Correlation::default()
            },
            event,
        )
        .unwrap()
    else {
        panic!()
    };
    // The application can await two storage operations independently here.
    bus.finish(&mut b, Outcome::Succeeded).unwrap();
    bus.finish(&mut a, Outcome::Failed(Fault::Timeout)).unwrap();
    assert_eq!(
        bus.observer().0,
        [
            (Some(2), Outcome::Succeeded),
            (Some(1), Outcome::Failed(Fault::Timeout))
        ]
    );
    assert_eq!(
        bus.finish(&mut b, Outcome::Succeeded),
        Err(Fault::Core(Error::Stale))
    );
}
#[test]
fn custom_prompt_transform_failure_does_not_fall_back_to_unfiltered_input() {
    struct Reject;
    impl<'a> Interceptor<prompt::Event<'a>> for Reject {
        fn intercept(&mut self, _: &mut prompt::Event<'a>) -> Result<(), Fault> {
            Err(Fault::Application(42))
        }
    }
    let mut bus = Lifecycle::new(1, NoGate, NoObserver);
    let result = bus.run_intercepted(
        Correlation::default(),
        prompt::Event::new(prompt::Action::InjectContext).with_payload(Payload::utf8("secret")),
        &mut Reject,
        |_| -> Result<(), Fault> {
            panic!("must not inject raw text");
        },
    );
    assert_eq!(result, Err(Fault::Application(42)));
}
#[test]
fn failed_upstream_turn_is_not_finalized_even_when_terminal_event_was_accepted() {
    use zharness::{response::ResponseStream, zcall::v1::*};
    let mut bus = Lifecycle::new(1, NoGate, NoObserver);
    let mut stream = ResponseStream::new(Correlation::default());
    let terminal = Terminal {
        outcome: TurnOutcome::UpstreamFailed,
        usage: StreamUsage::default(),
        error: None,
        upstream_code: None,
        upstream_message: None,
    };
    stream
        .accept(EventData::Terminal(terminal), &mut bus, |_| {
            Ok(Control::Continue)
        })
        .unwrap();
    assert!(
        stream
            .finalize(Payload::metadata(0), &mut bus, || Ok(()))
            .is_err()
    );
    let mut e = response::Event::new(response::Action::TurnEnd);
    e.terminal = Some(TurnOutcome::UpstreamFailed);
    bus.notify(Correlation::default(), e).unwrap();
}
