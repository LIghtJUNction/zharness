//! Complete local zcall stream -> ToolReady -> executor -> next RequestBuilder turn.
use zcall::v1::*;
use zharness::tools::{Executor, Invocation, Tool, ToolSet};

struct LocalTools;

impl Executor for LocalTools {
    fn execute(
        &mut self,
        name: &str,
        arguments: &[u8],
        output: &mut [u8],
    ) -> Result<usize, zharness::mcp::Error> {
        let result = match name {
            "lookup" => b"lookup completed".as_slice(),
            "save" => b"save completed".as_slice(),
            _ => return Err(zharness::mcp::Error::UnknownTool),
        };
        let _ = arguments;
        if result.len() > output.len() {
            return Err(zharness::mcp::Error::Capacity);
        }
        output[..result.len()].copy_from_slice(result);
        Ok(result.len())
    }
}

static TOOLS: &[Tool<'static>] = &[
    Tool {
        name: "lookup",
        description: "Look up a record",
        input_schema: r#"{"type":"object","properties":{"a":{"type":"integer"}}}"#,
    },
    Tool {
        name: "save",
        description: "Save a record",
        input_schema: r#"{"type":"object","properties":{"b":{"type":"integer"}}}"#,
    },
];

const STREAM: &str = concat!(
    "data: {\"id\":\"response_9\",\"model\":\"model-x\",\"created\":9,\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{\\\"a\\\":\"}}]},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"save\",\"arguments\":\"{\\\"b\\\":\"}}]},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\"2}\"}},{\"index\":0,\"function\":{\"arguments\":\"1}\"}}]},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":4,\"total_tokens\":9}}\n\n",
    "data: [DONE]\n\n"
);

fn main() {
    let tools = ToolSet::new(TOOLS).expect("valid registry");
    let mut executor = LocalTools;
    let mut scratch = [0u8; 128];
    let mut framing = [0u8; 1024];
    let mut payload = [0u8; 1024];
    let mut aggregate = [0u8; 512];
    let mut workspace = StreamWorkspace::<128, 32, 8>::new();

    let mut session = Session::new(
        SessionConfig::new(Protocol::OpenAiChat.into()),
        &mut framing,
        &mut payload,
        &mut aggregate,
        &mut workspace,
    )
    .expect("session storage");
    session.prepare().expect("prepare model turn");

    let mut ready = 0u8;
    let step = session.step(STREAM.as_bytes(), true, |event| {
        if let EventData::ToolReady(tool) = event.data() {
            let invocation = Invocation::from_zcall(tool);
            assert!(tools.get(invocation.name).is_some());
            executor
                .execute(invocation.name, invocation.arguments, &mut scratch)
                .expect("execute authorized example tool");
            ready += 1;
        }
        Control::Continue
    });
    assert_eq!(step.consumed, STREAM.len());
    assert!(matches!(step.status, StepStatus::Done));
    assert_eq!(ready, 2);

    // zcall validates that each result belongs to a completed ToolReady call.
    session.submit_tool_result("call_a").expect("submit lookup result");
    session.submit_tool_result("call_b").expect("submit save result");

    // Build the next turn from the assistant's original calls and their results.
    let mut records = RecordStorage::<16>::new();
    let mut builder = RequestBuilder::new(
        Protocol::OpenAiChat.into(),
        Presence::Value(Text::new("model-x")),
        &mut records,
    )
    .expect("request storage");
    let calls = [
        Content::ToolCall {
            id: Presence::Value(Text::new("call_a")),
            name: Text::new("lookup"),
            arguments: Data::JsonText(Text::new(r#"{"a":1}"#)),
            legacy: false,
        },
        Content::ToolCall {
            id: Presence::Value(Text::new("call_b")),
            name: Text::new("save"),
            arguments: Data::JsonText(Text::new(r#"{"b":2}"#)),
            legacy: false,
        },
    ];
    builder
        .message(Role::Assistant, &calls)
        .expect("append assistant calls");
    let mut request = builder.finish();
    request
        .append_tool_result(
            "call_a",
            Data::Text(Text::new("lookup completed")),
            Presence::Missing,
        )
        .expect("append lookup result");
    request
        .append_tool_result(
            "call_b",
            Data::Text(Text::new("save completed")),
            Presence::Missing,
        )
        .expect("append save result");

    let mut encoded = [0u8; 2048];
    let mut encode_scratch = [0u8; 1024];
    let mut tokens = TokenStorage::<256>::new();
    let report = request
        .encode(
            Protocol::OpenAiChat.into(),
            &mut encoded,
            &mut encode_scratch,
            &mut tokens,
            EncodeContext::default(),
            Policy::default(),
        )
        .expect("encode next model request");
    println!("{}", core::str::from_utf8(&encoded[..report.written]).unwrap());
}
