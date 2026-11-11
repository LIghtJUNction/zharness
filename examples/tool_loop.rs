//! An actual two-round controller/context/ToolReady flow using recorded bytes.
//! The fixture adapter uses expect/assert for fixed demonstration capacities.
//! Production transports must propagate errors, backpressure and cancellation.
use zharness::{Error, Executor, Tool, ToolLoop, ToolOutput, ToolSet, bridge::{Completion, InvocationStorage}, context::{Budget, Context, Group, Message, Overflow}, hooks::{After, Before, Decision, Hooks}, turn::{Action, CallSlot, LoopLimits, State, Stop}, validation::{ArgumentLimits, Arguments}, zcall::v1::*};

static TOOLS: &[Tool<'static>] = &[
    Tool::new("lookup", "Echo a lookup request", r#"{"type":"object"}"#),
    Tool::new("save", "Echo a save request without performing a write", r#"{"type":"object"}"#),
];
static USER: [Content<'static>; 1] = [Content::text("Look up record 1 and save record 2")];
#[derive(Default)]
struct Audit { models: usize, tools: usize }
impl Hooks for Audit {
    fn after(&mut self, event: After<'_>) {
        match event { After::ModelFinished { .. } => self.models += 1, After::ToolFinished { .. } => self.tools += 1, _ => {} }
    }
}
struct AllowDemoTools;
impl Hooks for AllowDemoTools {
    fn before(&mut self, event: Before<'_>) -> Decision {
        match event { Before::Tool { call, .. } if !matches!(call.name, "lookup" | "save") => Decision::Deny, _ => Decision::Allow }
    }
}
struct Echo;
impl Executor for Echo {
    fn execute(&mut self, name: &str, arguments: Arguments<'_>, output: &mut [u8]) -> Result<ToolOutput, Error> {
        if !matches!(name, "lookup" | "save") { return Err(Error::Denied); }
        let bytes = arguments.as_bytes();
        output.get_mut(..bytes.len()).ok_or(Error::Capacity { needed: bytes.len() })?.copy_from_slice(bytes);
        Ok(ToolOutput::success(bytes.len()))
    }
}

fn encode_history(messages: &[Message<'_>], output: &mut [u8]) -> usize {
    let mut slots = [None; 2];
    let mut context = Context::new(&mut slots, Budget { limit: 4096, reserved: 1024 }).unwrap();
    // The whole fixture turn is one eviction unit. 128 is a demonstration estimate.
    context.push(Group::new(1, messages, 128), Overflow::Reject).unwrap();
    let mut tokens = TokenStorage::<128>::new();
    let definitions = [TOOLS[0].as_zcall(&mut tokens).unwrap(), TOOLS[1].as_zcall(&mut tokens).unwrap()];
    let mut records = RecordStorage::<32>::new();
    let mut request = context.build(Protocol::OpenAiChat.into(), Presence::Value(Text::new("model-x")), &mut records, &definitions).unwrap();
    request.encode(Protocol::OpenAiChat.into(), output, &mut [0; 2048], &mut tokens, EncodeContext::default(), Policy::default()).unwrap().written
}

fn encode_next(history: &[InvocationStorage<256>; 2], results: &[[u8; 128]; 2], lengths: [usize; 2], registry: &ToolSet<'_>, output: &mut [u8]) -> usize {
    match (history[0].view(), history[1].view()) {
        (Some(a), Some(b)) => {
            let mut tokens = TokenStorage::<64>::new();
            let a = a.prepare(registry, &mut tokens, ArgumentLimits::default()).unwrap();
            let b = b.prepare(registry, &mut tokens, ArgumentLimits::default()).unwrap();
            let calls = [a.content(), b.content()];
            let messages = [
                Message::new(Role::User, &USER),
                Message::new(Role::Assistant, &calls),
                Message::ToolResult(Completion { call_id: a.call_id(), text: core::str::from_utf8(&results[0][..lengths[0]]).unwrap(), is_error: false }),
                Message::ToolResult(Completion { call_id: b.call_id(), text: core::str::from_utf8(&results[1][..lengths[1]]).unwrap(), is_error: false }),
            ];
            encode_history(&messages, output)
        }
        (None, None) => encode_history(&[Message::new(Role::User, &USER)], output),
        _ => panic!("the fixture requires two retained results"),
    }
}

pub fn main() {
    let registry = ToolSet::new(TOOLS).unwrap();
    let mut slots = [const { CallSlot::<256>::new() }; 2];
    let mut flow = ToolLoop::new(1, &mut slots, LoopLimits { max_rounds: 4, max_tool_calls: 4 });
    let mut hooks = (Audit::default(), AllowDemoTools);
    // Durable caller-owned transcript storage is separate from per-turn queue slots.
    let mut history = [InvocationStorage::<256>::new(), InvocationStorage::<256>::new()];
    let mut results = [[0u8; 128]; 2]; let mut lengths = [0; 2];
    let mut request_bytes = [0; 4096];
    loop {
        match flow.next(&mut hooks).unwrap() {
            Action::CallModel { round, .. } => {
                let n = encode_next(&history, &results, lengths, &registry, &mut request_bytes);
                let request = core::str::from_utf8(&request_bytes[..n]).unwrap();
                if round == 2 { assert!(request.contains("call_a") && request.contains("call_b")); }
                // Application transport sends request_bytes[..n]. This demo reads a fixture.
                let input: &[u8] = if round == 1 { include_bytes!("../tests/fixtures/interleaved.sse") } else { include_bytes!("../tests/fixtures/final.sse") };
                let mut frame = [0; 1024]; let mut payload = [0; 1024]; let mut aggregate = [0; 2048];
                let mut work = StreamWorkspace::<128, 32, 8>::new();
                let mut session = Session::new(SessionConfig::new(Protocol::OpenAiChat.into()), &mut frame, &mut payload, &mut aggregate, &mut work).unwrap();
                session.prepare().unwrap();
                let mut offset = 0;
                loop {
                    let end = (offset + 17).min(input.len()); let mut error = None;
                    let step = session.step(&input[offset..end], end == input.len(), |event| {
                        match flow.observe(event.data(), &mut hooks) { Ok(()) => Control::Continue, Err(e) => { error = Some(e); Control::Pause } }
                    });
                    assert!(error.is_none(), "fixture observation failed: {error:?}"); offset += step.consumed;
                    match step.status {
                        StepStatus::Done => break,
                        StepStatus::NeedInput | StepStatus::NeedEvents => assert!(step.consumed != 0 || step.emitted != 0),
                        other => panic!("fixture stream failed: {other:?}"),
                    }
                }
                if flow.state() == State::Tools {
                    let mut tickets = [None; 2];
                    for (at, ticket) in tickets.iter_mut().enumerate() {
                        let Action::Execute(issued) = flow.next(&mut hooks).unwrap() else { panic!("expected approved tool") };
                        history[at].store(flow.invocation(issued).unwrap()).unwrap(); *ticket = Some(issued);
                    }
                    // The application chooses reverse completion order; no thread is spawned.
                    for at in (0..2).rev() {
                        let call = history[at].view().unwrap();
                        let prepared = call.prepare(&registry, &mut TokenStorage::<64>::new(), ArgumentLimits::default()).unwrap();
                        let result = prepared.execute(&mut Echo, &mut results[at]).unwrap();
                        lengths[at] = result.text.len();
                        result.submit(&mut session).unwrap();
                        flow.commit_result(tickets[at].unwrap(), &result, &mut hooks).unwrap();
                    }
                }
            }
            Action::Stopped(Stop::Completed) => break,
            other => panic!("unexpected demo action {other:?}"),
        }
    }
    assert_eq!(hooks.0.models, 2); assert_eq!(hooks.0.tools, 2);
    println!("Completed two model turns and two tools with bounded history and hooks.");
}
