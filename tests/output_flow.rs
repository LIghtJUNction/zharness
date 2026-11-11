use zharness::{Error, Executor, Tool, ToolLoop, ToolOutput, ToolSet, bridge::InvocationStorage, context::{Budget, Context, Group, Message, Overflow}, hooks::{AfterResult, BeforeResult, Decision, HookResult, Hooks, NoHooks}, output::{DedupAdjacentLines, Trim}, turn::{Action, CallSlot, LoopLimits, State}, validation::{ArgumentLimits, Arguments}, zcall::v1::*};

struct EchoLog { calls: usize }
impl Executor for EchoLog {
    fn execute(&mut self, name: &str, _: Arguments<'_>, out: &mut [u8]) -> Result<ToolOutput, Error> {
        let text = if name == "save" { "error: no permission\nerror: no permission\n" } else { "found\nfound\n" };
        out.get_mut(..text.len()).ok_or(Error::Capacity { needed: text.len() })?.copy_from_slice(text.as_bytes());
        self.calls += 1;
        Ok(ToolOutput { written: text.len(), is_error: name == "save" })
    }
}
struct Gate { approved: bool, completed: usize }
impl Hooks for Gate {
    fn before_result(&mut self, _: BeforeResult<'_>) -> Decision { if self.approved { Decision::Allow } else { Decision::Pause } }
    fn after_result(&mut self, _: AfterResult<'_>) { self.completed += 1; }
}

#[test]
fn real_interleaved_calls_transform_before_history_and_loop_commit() {
    let tools = [Tool::new("lookup", "Lookup", r#"{"type":"object"}"#), Tool::new("save", "Save", r#"{"type":"object"}"#)];
    let registry = ToolSet::new(&tools).unwrap();
    let mut slots = [const { CallSlot::<256>::new() }; 2];
    let mut flow = ToolLoop::new(42, &mut slots, LoopLimits::default());
    assert!(matches!(flow.next(&mut NoHooks).unwrap(), Action::CallModel { round: 1, .. }));
    let mut frame = [0; 1024]; let mut payload = [0; 1024]; let mut aggregate = [0; 2048];
    let mut workspace = StreamWorkspace::<128, 32, 8>::new();
    let mut session = Session::new(SessionConfig::new(Protocol::OpenAiChat.into()), &mut frame, &mut payload, &mut aggregate, &mut workspace).unwrap();
    session.prepare().unwrap();
    let input = include_bytes!("fixtures/interleaved.sse"); let mut offset = 0;
    loop {
        let end = (offset + 17).min(input.len());
        let step = session.step(&input[offset..end], end == input.len(), |event| {
            flow.observe(event.data(), &mut NoHooks).unwrap(); Control::Continue
        });
        offset += step.consumed;
        match step.status {
            StepStatus::Done => break,
            StepStatus::NeedInput | StepStatus::NeedEvents => assert!(step.consumed != 0 || step.emitted != 0),
            other => panic!("unexpected fixture result: {other:?}"),
        }
    }
    let Action::Execute(a) = flow.next(&mut NoHooks).unwrap() else { panic!() };
    let Action::Execute(b) = flow.next(&mut NoHooks).unwrap() else { panic!() };
    let mut history_a = InvocationStorage::<256>::new(); let mut history_b = InvocationStorage::<256>::new();
    history_a.store(flow.invocation(a).unwrap()).unwrap(); history_b.store(flow.invocation(b).unwrap()).unwrap();
    let mut tokens = TokenStorage::<64>::new();
    let prepared_a = history_a.view().unwrap().prepare(&registry, &mut tokens, ArgumentLimits::default()).unwrap();
    let prepared_b = history_b.view().unwrap().prepare(&registry, &mut tokens, ArgumentLimits::default()).unwrap();
    let mut raw_a = [0; 128]; let mut raw_b = [0; 128];
    let mut executor = EchoLog { calls: 0 };
    let original_a = prepared_a.execute(&mut executor, &mut raw_a).unwrap();
    let original_b = prepared_b.execute(&mut executor, &mut raw_b).unwrap();
    let mut out_a = [0; 128]; let mut out_b = [0; 128];
    let mut gate = Gate { approved: false, completed: 0 };
    assert!(matches!(original_a.transform_with_hooks(prepared_a.name(), &mut DedupAdjacentLines, &mut out_a, &mut gate).unwrap(), HookResult::Paused));
    assert_eq!(flow.state(), State::Tools); assert_eq!(flow.next(&mut NoHooks).unwrap(), Action::Pending);
    gate.approved = true;
    assert!(matches!(original_a.transform_with_hooks(prepared_a.name(), &mut DedupAdjacentLines, &mut [], &mut gate), Err(Error::Capacity { .. })));
    assert_eq!(gate.completed, 0); assert_eq!(executor.calls, 2);
    // Retry only output processing after obtaining more storage, not tool execution.
    let HookResult::Applied(reduced_a) = original_a.transform_with_hooks(prepared_a.name(), &mut DedupAdjacentLines, &mut out_a, &mut gate).unwrap() else { panic!() };
    let HookResult::Applied(reduced_b) = original_b.transform_with_hooks(prepared_b.name(), &mut DedupAdjacentLines, &mut out_b, &mut gate).unwrap() else { panic!() };
    assert_eq!(gate.completed, 2); assert_eq!(executor.calls, 2);
    assert_eq!(reduced_a.call_id, "call_a"); assert_eq!(reduced_b.call_id, "call_b");
    assert!(!reduced_a.is_error); assert!(reduced_b.is_error);
    assert!(reduced_a.text.len() < original_a.text.len()); assert!(reduced_b.text.len() < original_b.text.len());

    let calls = [prepared_a.content(), prepared_b.content()];
    let messages = [Message::new(Role::Assistant, &calls), Message::ToolResult(reduced_b), Message::ToolResult(reduced_a)];
    let mut groups = [None; 1];
    let mut context = Context::new(&mut groups, Budget { limit: 4096, reserved: 1024 }).unwrap();
    context.push(Group::new(1, &messages, 128), Overflow::Reject).unwrap();
    let mut records = RecordStorage::<24>::new();
    let request = context.build(Protocol::AnthropicMessages.into(), Presence::Value(Text::new("model-x")), &mut records, &[]).unwrap();
    assert_eq!(request.messages().count(), 3);
    reduced_b.submit(&mut session).unwrap(); flow.commit_result(b, &reduced_b, &mut NoHooks).unwrap();
    assert!(flow.commit_result(b, &reduced_b, &mut NoHooks).is_err());
    reduced_a.submit(&mut session).unwrap(); flow.commit_result(a, &reduced_a, &mut NoHooks).unwrap();
    assert!(matches!(flow.next(&mut NoHooks).unwrap(), Action::CallModel { round: 2, .. }));
}

#[test]
fn parallel_transforms_keep_per_call_buffers_and_metadata_separate() {
    use zharness::bridge::Completion;
    let mut a = [0; 32]; let mut b = [0; 32];
    let (na, nb) = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            let raw = Completion { call_id: "a", text: "  first  ", is_error: false };
            let result = raw.transform("one", &mut Trim, &mut a).unwrap();
            assert_eq!(result.call_id, "a"); assert!(!result.is_error); result.text.len()
        });
        let second = scope.spawn(|| {
            let raw = Completion { call_id: "b", text: "  second error  ", is_error: true };
            let result = raw.transform("two", &mut Trim, &mut b).unwrap();
            assert_eq!(result.call_id, "b"); assert!(result.is_error); result.text.len()
        });
        (first.join().unwrap(), second.join().unwrap())
    });
    assert_eq!(&a[..na], b"first"); assert_eq!(&b[..nb], b"second error");
}

#[path = "../examples/tool_output.rs"]
mod example;
#[test]
fn runnable_output_example() { example::main().unwrap(); }
