use zharness::{
    Error,
    context::{Budget, Context, Group, Message, Overflow},
    hooks::{Decision, HookResult},
    lifecycle::{
        Correlation, Fault, Gate, Lifecycle, NoGate, NoObserver, Notice, Observer, Outcome,
        Payload, Phase, Proposal, domains::response,
    },
    response::ResponseStream,
    zcall::v1::*,
};
const IDS: Correlation = Correlation {
    conversation: 1,
    run: 1,
    request: Some(1),
    message: None,
    agent: None,
    attempt: 0,
};
struct PauseFirst(bool);
impl<'a> Gate<response::Event<'a>> for PauseFirst {
    fn before(&mut self, p: Proposal<response::Event<'a>>) -> Decision {
        if p.event.first_visible && !self.0 {
            self.0 = true;
            Decision::Pause
        } else {
            Decision::Allow
        }
    }
}
#[derive(Default)]
struct Count {
    first: usize,
    paused: usize,
}
impl<'a> Observer<response::Event<'a>> for Count {
    fn observe(&mut self, n: Notice<response::Event<'a>>) {
        if n.phase == Phase::Paused {
            self.paused += 1;
        }
        if n.phase == Phase::Finished(Outcome::Succeeded) && n.proposal.event.first_visible {
            self.first += 1;
        }
    }
}
#[test]
fn actual_session_replays_paused_event_without_duplicating_output() {
    let bytes = include_bytes!("fixtures/final.sse");
    let mut frame = [0; 1024];
    let mut payload = [0; 1024];
    let mut aggregate = [0; 1024];
    let mut work = StreamWorkspace::<128, 32, 8>::new();
    let mut session = Session::new(
        SessionConfig::new(Protocol::OpenAiChat.into()),
        &mut frame,
        &mut payload,
        &mut aggregate,
        &mut work,
    )
    .unwrap();
    session.prepare().unwrap();
    let mut stream = ResponseStream::new(IDS);
    let mut bus = Lifecycle::new(1, PauseFirst(false), Count::default());
    let mut answer = String::new();
    let mut offset = 0;
    let mut done = false;
    for _ in 0..32 {
        let step = session.step(&bytes[offset..], true, |event| {
            stream
                .accept(event.data(), &mut bus, |data| {
                    if let EventData::Delta {
                        kind: BlockType::Text,
                        text,
                        ..
                    } = data
                    {
                        let mut decoded = [0; 64];
                        let n = text.decode_into(&mut decoded).unwrap();
                        answer.push_str(core::str::from_utf8(&decoded[..n]).unwrap());
                    }
                    Ok(Control::Continue)
                })
                .unwrap()
        });
        offset += step.consumed;
        if step.status == StepStatus::Done {
            done = true;
            break;
        }
    }
    assert!(done);
    assert_eq!(offset, bytes.len());
    assert_eq!(answer, "done");
    assert_eq!(bus.observer().paused, 1);
    assert_eq!(bus.observer().first, 1);
}
#[test]
fn native_context_operations_gate_mutation_and_build_real_requests() {
    static TEXT: [Content<'static>; 1] = [Content::text("x")];
    static MESSAGES: [Message<'static>; 1] = [Message::new(Role::User, &TEXT)];
    let mut slots = [None; 3];
    let mut c = Context::new(
        &mut slots,
        Budget {
            limit: 100,
            reserved: 10,
        },
    )
    .unwrap();
    c.push(Group::new(1, &MESSAGES, 10), Overflow::Reject)
        .unwrap();
    let mut bus = Lifecycle::new(1, NoGate, NoObserver);
    let _ = c
        .set_budget_with_lifecycle(
            IDS,
            Budget {
                limit: 50,
                reserved: 10,
            },
            &mut bus,
        )
        .unwrap();
    let mut fork_slots = [None; 3];
    let HookResult::Applied(fork) = c
        .fork_with_lifecycle(IDS, &mut fork_slots, &mut bus)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(fork.tokens(), 10);
    let _ = c
        .compact_with_lifecycle(IDS, 0..1, Group::new(2, &MESSAGES, 5), &mut bus)
        .unwrap();
    let mut records = RecordStorage::<16>::new();
    let HookResult::Applied(request) = c
        .build_with_lifecycle(
            IDS,
            Protocol::OpenAiChat.into(),
            Presence::Value(Text::new("m")),
            &mut records,
            &[],
            &mut bus,
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(request.messages().count(), 1);
    assert_eq!(
        c.set_budget_with_lifecycle(
            IDS,
            Budget {
                limit: 1,
                reserved: 2
            },
            &mut bus
        ),
        Err(Fault::Core(Error::InvalidBudget))
    );
    assert_eq!(c.tokens(), 5);
}
#[test]
fn consumer_failure_cancel_and_hidden_reasoning_do_not_report_final_success() {
    let mut stream = ResponseStream::new(IDS);
    let mut bus = Lifecycle::new(1, NoGate, Count::default());
    let started = EventData::Started {
        response_id: None,
        model: None,
        created: None,
    };
    assert_eq!(
        stream.accept(started, &mut bus, |_| Err(Fault::Application(7))),
        Err(Fault::Application(7))
    );
    assert_eq!(stream.accepted_events(), 0);
    stream
        .accept(started, &mut bus, |_| Ok(Control::Continue))
        .unwrap();
    stream
        .accept(
            EventData::Delta {
                key: EventKey::default(),
                kind: BlockType::Reasoning,
                text: Text::new("provider-exposed summary"),
            },
            &mut bus,
            |_| Ok(Control::Continue),
        )
        .unwrap();
    assert!(!stream.has_visible_content());
    assert_eq!(bus.observer().first, 0);
    let _ = stream.cancel(&mut bus).unwrap();
    assert!(
        stream
            .finalize(Payload::metadata(0), &mut bus, || Ok(()))
            .is_err()
    );
}
#[path = "../examples/conversation_hooks.rs"]
mod example;
#[test]
fn executable_end_to_end_conversation_example() {
    example::main().unwrap();
}
