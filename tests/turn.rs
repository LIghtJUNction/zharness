use zharness::{
    Error, ToolLoop,
    bridge::Completion,
    hooks::{After, Before, Decision, Hooks, NoHooks},
    turn::{Action, CallSlot, LoopLimits, State, Stop},
    zcall::v1::*,
};

fn deliver<const N: usize>(
    runtime: &mut ToolLoop<'_, N>,
    hooks: &mut impl Hooks,
    bytes: &[u8],
) -> Result<(), Error> {
    let mut frame = [0; 1024];
    let mut payload = [0; 1024];
    let mut aggregate = [0; 2048];
    let mut work = StreamWorkspace::<128, 32, 8>::new();
    let mut session = Session::new(
        SessionConfig::new(Protocol::OpenAiChat.into()),
        &mut frame,
        &mut payload,
        &mut aggregate,
        &mut work,
    )?;
    session.prepare()?;
    let mut offset = 0;
    loop {
        let end = (offset + 17).min(bytes.len());
        let mut failure = None;
        let step = session.step(
            &bytes[offset..end],
            end == bytes.len(),
            |event| match runtime.observe(event.data(), hooks) {
                Ok(()) => Control::Continue,
                Err(error) => {
                    failure = Some(error);
                    Control::Pause
                }
            },
        );
        if let Some(error) = failure {
            return Err(error);
        }
        offset += step.consumed;
        match step.status {
            StepStatus::Done => return Ok(()),
            StepStatus::NeedEvents | StepStatus::NeedInput => {
                assert!(step.consumed != 0 || step.emitted != 0)
            }
            StepStatus::Failed(error) => return Err(Error::Zcall(error)),
            other => panic!("unexpected fixture status {other:?}"),
        }
    }
}
const TOOLS: &[u8] = include_bytes!("fixtures/interleaved.sse");
const FINAL: &[u8] = include_bytes!("fixtures/final.sse");

#[test]
fn real_stream_parallel_tools_and_next_model_turn() {
    let mut slots = [const { CallSlot::<256>::new() }; 2];
    let mut hooks = NoHooks;
    let mut runtime = ToolLoop::new(7, &mut slots, LoopLimits::default());
    assert_eq!(
        runtime.next(&mut hooks).unwrap(),
        Action::CallModel { run: 7, round: 1 }
    );
    assert_eq!(runtime.next(&mut hooks).unwrap(), Action::Pending);
    deliver(&mut runtime, &mut hooks, TOOLS).unwrap();
    assert_eq!(runtime.state(), State::Tools);
    let Action::Execute(a) = runtime.next(&mut hooks).unwrap() else {
        panic!()
    };
    let Action::Execute(b) = runtime.next(&mut hooks).unwrap() else {
        panic!()
    };
    assert_eq!(runtime.next(&mut hooks).unwrap(), Action::Pending);
    let id_a = runtime.invocation(a).unwrap().call_id.to_owned();
    let id_b = runtime.invocation(b).unwrap().call_id.to_owned();
    assert_eq!([id_a.as_str(), id_b.as_str()], ["call_a", "call_b"]);
    let result_b = Completion {
        call_id: &id_b,
        text: "failed",
        is_error: true,
    };
    runtime.commit_result(b, &result_b, &mut hooks).unwrap();
    assert_eq!(
        runtime.commit_result(b, &result_b, &mut hooks),
        Err(Error::Stale)
    );
    assert_eq!(
        runtime.commit_result(a, &result_b, &mut hooks),
        Err(Error::UnknownId)
    );
    runtime
        .commit_result(
            a,
            &Completion {
                call_id: &id_a,
                text: "ok",
                is_error: false,
            },
            &mut hooks,
        )
        .unwrap();
    assert_eq!(
        runtime.next(&mut hooks).unwrap(),
        Action::CallModel { run: 7, round: 2 }
    );
    assert!(runtime.invocation(a).is_err());
    deliver(&mut runtime, &mut hooks, FINAL).unwrap();
    assert_eq!(
        runtime.next(&mut hooks).unwrap(),
        Action::Stopped(Stop::Completed)
    );
    assert_eq!(runtime.total_calls(), 2);
}
struct Gate {
    decision: Decision,
    after: usize,
}
impl Hooks for Gate {
    fn before(&mut self, _: Before<'_>) -> Decision {
        self.decision
    }
    fn after(&mut self, _: After<'_>) {
        self.after += 1;
    }
}
#[test]
fn pause_deny_cancel_and_limits_do_not_duplicate_side_effects() {
    let mut slots = [const { CallSlot::<256>::new() }; 2];
    let mut runtime = ToolLoop::new(
        1,
        &mut slots,
        LoopLimits {
            max_rounds: 1,
            max_tool_calls: 2,
        },
    );
    let mut gate = Gate {
        decision: Decision::Pause,
        after: 0,
    };
    assert_eq!(runtime.next(&mut gate).unwrap(), Action::Paused);
    assert_eq!(runtime.round(), 0);
    gate.decision = Decision::Deny;
    assert_eq!(runtime.next(&mut gate), Err(Error::Denied));
    assert_eq!(runtime.state(), State::Ready);
    gate.decision = Decision::Allow;
    let _ = runtime.next(&mut gate).unwrap();
    deliver(&mut runtime, &mut gate, TOOLS).unwrap();
    gate.decision = Decision::Pause;
    assert_eq!(runtime.next(&mut gate).unwrap(), Action::Paused);
    gate.decision = Decision::Allow;
    let Action::Execute(ticket) = runtime.next(&mut gate).unwrap() else {
        panic!()
    };
    let id = runtime.invocation(ticket).unwrap().call_id.to_owned();
    assert!(runtime.cancel(&mut gate));
    assert!(!runtime.cancel(&mut gate));
    assert!(
        runtime
            .commit_result(
                ticket,
                &Completion {
                    call_id: &id,
                    text: "late",
                    is_error: false
                },
                &mut gate
            )
            .is_err()
    );
    assert_eq!(
        runtime.next(&mut gate).unwrap(),
        Action::Stopped(Stop::Cancelled)
    );
    let mut zero_slots = [];
    let mut zero = ToolLoop::<256>::new(
        2,
        &mut zero_slots,
        LoopLimits {
            max_rounds: 0,
            max_tool_calls: 0,
        },
    );
    assert_eq!(
        zero.next(&mut NoHooks).unwrap(),
        Action::Stopped(Stop::RoundLimit)
    );
}
#[test]
fn failed_model_turn_does_not_schedule_retained_calls() {
    let mut slots = [const { CallSlot::<256>::new() }; 2];
    let mut hooks = NoHooks;
    let mut runtime = ToolLoop::new(1, &mut slots, LoopLimits::default());
    let _ = runtime.next(&mut hooks).unwrap();
    let first = TOOLS.windows(2).position(|w| w == b"\n\n").unwrap() + 2;
    let _ = deliver(&mut runtime, &mut hooks, &TOOLS[..first]);
    assert!(!matches!(
        runtime.next(&mut hooks).unwrap(),
        Action::Execute(_)
    ));
    assert_eq!(runtime.retained_calls(), 0);
}
#[test]
fn capacity_and_total_call_limits_reject_unaccepted_events() {
    let mut small = [CallSlot::<2>::new()];
    let mut hooks = NoHooks;
    let mut runtime = ToolLoop::new(1, &mut small, LoopLimits::default());
    let _ = runtime.next(&mut hooks).unwrap();
    assert!(matches!(
        deliver(&mut runtime, &mut hooks, TOOLS),
        Err(Error::Capacity { .. })
    ));
    assert_eq!(runtime.total_calls(), 0);
    assert_eq!(runtime.retained_calls(), 0);
    let mut slots = [const { CallSlot::<256>::new() }; 2];
    let mut limited = ToolLoop::new(
        2,
        &mut slots,
        LoopLimits {
            max_rounds: 2,
            max_tool_calls: 1,
        },
    );
    let _ = limited.next(&mut hooks).unwrap();
    assert_eq!(
        deliver(&mut limited, &mut hooks, TOOLS),
        Err(Error::LimitExceeded)
    );
    assert_eq!(limited.total_calls(), 1);
    assert_eq!(limited.next(&mut hooks).unwrap(), Action::Pending);
}
#[test]
fn hook_chain_short_circuits_and_observers_run_in_order() {
    use std::cell::RefCell;
    struct Trace<'a>(&'a RefCell<Vec<u8>>, u8, Decision);
    impl Hooks for Trace<'_> {
        fn before(&mut self, _: Before<'_>) -> Decision {
            self.0.borrow_mut().push(self.1);
            self.2
        }
        fn after(&mut self, _: After<'_>) {
            self.0.borrow_mut().push(self.1 + 10);
        }
    }
    let log = RefCell::new(Vec::new());
    let mut chain = (
        Trace(&log, 1, Decision::Pause),
        Trace(&log, 2, Decision::Allow),
    );
    assert_eq!(
        chain.before(Before::Model { run: 0, round: 1 }),
        Decision::Pause
    );
    assert_eq!(*log.borrow(), [1]);
    chain.0.2 = Decision::Allow;
    assert_eq!(
        chain.before(Before::Model { run: 0, round: 1 }),
        Decision::Allow
    );
    chain.after(After::Stopped { run: 0 });
    assert_eq!(*log.borrow(), [1, 1, 2, 11, 12]);
}
