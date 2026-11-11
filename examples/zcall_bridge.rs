//! Complete local two-turn flow driven by real zcall streaming events.
use zharness::{
    Error, Executor, Tool, ToolOutput, ToolSet,
    bridge::{Invocation, InvocationStorage},
    validation::{ArgumentLimits, Arguments},
    zcall::v1::*,
};

struct LocalTools;
impl Executor for LocalTools {
    fn execute(
        &mut self,
        name: &str,
        arguments: Arguments<'_>,
        output: &mut [u8],
    ) -> Result<ToolOutput, Error> {
        if !matches!(name, "lookup" | "save") {
            return Err(Error::UnknownTool);
        }
        let raw = arguments.as_bytes();
        output
            .get_mut(..raw.len())
            .ok_or(Error::Capacity { needed: raw.len() })?
            .copy_from_slice(raw);
        Ok(ToolOutput::success(raw.len()))
    }
}
static TOOLS: &[Tool<'static>] = &[
    Tool::new(
        "lookup",
        "Look up a record",
        r#"{"type":"object","properties":{"a":{"type":"integer"}}}"#,
    ),
    Tool::new(
        "save",
        "Save a record",
        r#"{"type":"object","properties":{"b":{"type":"integer"}}}"#,
    ),
];

fn encode(request: &mut Request<'_, '_>, output: &mut [u8]) -> usize {
    request
        .encode(
            Protocol::OpenAiChat.into(),
            output,
            &mut [0; 2048],
            &mut TokenStorage::<256>::new(),
            EncodeContext::default(),
            Policy::default(),
        )
        .expect("encode request")
        .written
}

pub fn main() {
    let tools = ToolSet::new(TOOLS).unwrap();
    let mut tokens = TokenStorage::<128>::new();
    let mut initial_records = RecordStorage::<16>::new();
    let mut initial = RequestBuilder::new(
        Protocol::OpenAiChat.into(),
        Presence::Value(Text::new("model-x")),
        &mut initial_records,
    )
    .unwrap();
    initial
        .message(
            Role::User,
            &[Content::text("Look up record 1 and save record 2")],
        )
        .unwrap();
    for tool in tools.as_slice() {
        initial.tool(tool.as_zcall(&mut tokens).unwrap()).unwrap();
    }
    let mut initial = initial.finish();
    let mut wire_request = [0; 4096];
    let n = encode(&mut initial, &mut wire_request);
    assert!(n > 0); // Application sends these bytes through its chosen transport.

    let mut framing = [0; 1024];
    let mut payload = [0; 1024];
    let mut aggregate = [0; 1024];
    let mut workspace = StreamWorkspace::<128, 32, 8>::new();
    let mut session = Session::new(
        SessionConfig::new(Protocol::OpenAiChat.into()),
        &mut framing,
        &mut payload,
        &mut aggregate,
        &mut workspace,
    )
    .unwrap();
    session.prepare().unwrap();
    let stream = include_bytes!("../tests/fixtures/interleaved.sse");
    let mut retained = [
        InvocationStorage::<256>::new(),
        InvocationStorage::<256>::new(),
    ];
    let mut ready = 0;
    let mut offset = 0;
    loop {
        let end = (offset + 17).min(stream.len());
        let step = session.step(&stream[offset..end], end == stream.len(), |event| {
            if let Some(call) = Invocation::from_event(event.data()) {
                // Retention only; no tool side effect inside a replayable callback.
                retained
                    .get_mut(ready)
                    .expect("two fixture calls")
                    .store(call)
                    .expect("bounded retention");
                ready += 1;
            }
            Control::Continue
        });
        offset += step.consumed;
        match step.status {
            StepStatus::Done => break,
            StepStatus::NeedInput | StepStatus::NeedEvents => assert!(
                step.consumed != 0 || step.emitted != 0,
                "no stream progress"
            ),
            other => panic!("stream failure: {other:?}"),
        }
    }
    assert_eq!(offset, stream.len());
    assert_eq!(ready, 2);
    let a = retained[0]
        .view()
        .unwrap()
        .prepare(&tools, &mut tokens, ArgumentLimits::default())
        .unwrap();
    let b = retained[1]
        .view()
        .unwrap()
        .prepare(&tools, &mut tokens, ArgumentLimits::default())
        .unwrap();
    assert_ne!(a.call_id(), b.call_id());
    assert!([a.call_id(), b.call_id()].contains(&"call_a"));
    assert!([a.call_id(), b.call_id()].contains(&"call_b"));

    // Application authorizes and chooses execution order. Reverse completion is valid.
    let mut out_a = [0; 128];
    let mut out_b = [0; 128];
    let mut executor = LocalTools;
    let result_b = b.execute(&mut executor, &mut out_b).unwrap();
    let result_a = a.execute(&mut executor, &mut out_a).unwrap();
    let mut next_records = RecordStorage::<16>::new();
    let mut next = RequestBuilder::new(
        Protocol::OpenAiChat.into(),
        Presence::Value(Text::new("model-x")),
        &mut next_records,
    )
    .unwrap();
    next.message(
        Role::User,
        &[Content::text("Look up record 1 and save record 2")],
    )
    .unwrap();
    next.message(Role::Assistant, &[a.content(), b.content()])
        .unwrap();
    let mut next = next.finish();
    result_b.append_to(&mut next).unwrap();
    result_a.append_to(&mut next).unwrap();
    result_b.submit(&mut session).unwrap();
    result_a.submit(&mut session).unwrap();
    assert!(result_a.submit(&mut session).is_err());
    let n = encode(&mut next, &mut wire_request);
    let text = core::str::from_utf8(&wire_request[..n]).unwrap();
    assert!(text.contains("call_a") && text.contains("call_b"));
    println!("{text}");
}
