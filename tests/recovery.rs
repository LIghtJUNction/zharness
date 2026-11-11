use zharness::{Error, ToolLoop, context::{Budget, Context, Group, Message, Overflow}, hooks::NoHooks, turn::{Action, CallSlot, LoopLimits, State}, zcall::v1::*};

#[test]
fn paused_zcall_event_replays_after_growing_caller_slot_storage() {
    let bytes = include_bytes!("fixtures/interleaved.sse");
    let mut one = [CallSlot::<256>::new()]; let mut larger = [const { CallSlot::<256>::new() }; 4];
    let mut flow = ToolLoop::new(7, &mut one, LoopLimits::default()); let _ = flow.next(&mut NoHooks).unwrap();
    let mut frame = [0; 1024]; let mut payload = [0; 1024]; let mut aggregate = [0; 2048]; let mut work = StreamWorkspace::<128, 32, 8>::new();
    let mut session = Session::new(SessionConfig::new(Protocol::OpenAiChat.into()), &mut frame, &mut payload, &mut aggregate, &mut work).unwrap(); session.prepare().unwrap();
    let mut failure = None;
    let step = session.step(bytes, true, |event| {
        match flow.observe(event.data(), &mut NoHooks) { Ok(()) => Control::Continue, Err(error) => { failure = Some(error); Control::Pause } }
    });
    assert_eq!(failure, Some(Error::Capacity { needed: 2 }));
    assert_eq!(flow.retained_calls(), 1); assert_eq!(flow.total_calls(), 1);
    flow.replace_slots(&mut larger).unwrap();
    let mut offset = step.consumed;
    for _ in 0..16 {
        let step = session.step(&bytes[offset..], true, |event| { flow.observe(event.data(), &mut NoHooks).unwrap(); Control::Continue });
        offset += step.consumed;
        if step.status == StepStatus::Done { break; }
        assert!(step.consumed != 0 || step.emitted != 0);
    }
    assert_eq!(flow.state(), State::Tools); assert_eq!(flow.retained_calls(), 2); assert_eq!(flow.total_calls(), 2);
    let Action::Execute(first) = flow.next(&mut NoHooks).unwrap() else { panic!() };
    let Action::Execute(second) = flow.next(&mut NoHooks).unwrap() else { panic!() };
    assert_ne!(flow.invocation(first).unwrap().call_id, flow.invocation(second).unwrap().call_id);
}
#[test]
fn duplicate_normalized_tool_event_is_rejected_without_extra_work() {
    let bytes = include_bytes!("fixtures/interleaved.sse");
    let mut slots = [const { CallSlot::<256>::new() }; 4]; let mut flow = ToolLoop::new(1, &mut slots, LoopLimits::default()); let _ = flow.next(&mut NoHooks).unwrap();
    let mut frame = [0; 1024]; let mut payload = [0; 1024]; let mut aggregate = [0; 2048]; let mut work = StreamWorkspace::<128, 32, 8>::new();
    let mut session = Session::new(SessionConfig::new(Protocol::OpenAiChat.into()), &mut frame, &mut payload, &mut aggregate, &mut work).unwrap(); session.prepare().unwrap();
    let mut offset = 0; let mut duplicates = 0;
    for _ in 0..16 {
        let step = session.step(&bytes[offset..], true, |event| {
            let data = event.data(); flow.observe(data, &mut NoHooks).unwrap();
            if matches!(data, EventData::ToolReady(_)) { assert_eq!(flow.observe(data, &mut NoHooks), Err(Error::DuplicateId)); duplicates += 1; }
            Control::Continue
        });
        offset += step.consumed;
        if step.status == StepStatus::Done { break; }
    }
    assert_eq!(duplicates, 2); assert_eq!(flow.total_calls(), 2); assert_eq!(flow.state(), State::Tools);
}
#[test]
fn context_randomized_operations_preserve_accounting_and_failure_atomicity() {
    static CONTENT: [Content<'static>; 1] = [Content::text("x")];
    static MESSAGES: [Message<'static>; 1] = [Message::new(Role::User, &CONTENT)];
    let mut slots = [None; 8]; let mut c = Context::new(&mut slots, Budget { limit: 60, reserved: 10 }).unwrap();
    c.push(Group::new(0, &MESSAGES, 2).pinned(), Overflow::Reject).unwrap();
    let mut random = 0x12345678u32;
    for _ in 0..2000 {
        random ^= random << 13; random ^= random >> 17; random ^= random << 5;
        let id = 1 + u64::from(random % 12); let cost = u64::from(random % 70);
        let before: Vec<_> = c.groups().map(|g| (g.id, g.tokens)).collect();
        if c.push(Group::new(id, &MESSAGES, cost), Overflow::EvictOldest).is_err() {
            assert!(c.groups().map(|g| (g.id, g.tokens)).eq(before));
        }
        assert_eq!(c.groups().map(|g| g.tokens).sum::<u64>(), c.tokens());
        assert!(c.tokens() <= 50); assert!(c.len() <= 8);
        assert_eq!(c.groups().next().unwrap().id, 0);
    }
}
