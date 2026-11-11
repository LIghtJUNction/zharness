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

    let mut ready = 0usize;
    let mut call_ids = [[0u8; 32]; 2];
    let mut call_id_lens = [0usize; 2];
    let mut call_names = [[0u8; 32]; 2];
    let mut call_name_lens = [0usize; 2];
    let mut call_arguments = [[0u8; 128]; 2];
    let mut call_argument_lens = [0usize; 2];
    let mut results = [[0u8; 128]; 2];
    let mut result_lens = [0usize; 2];
    let mut offset = 0usize;
    while offset < STREAM.len() {
        let end = (offset + 17).min(STREAM.len());
        let step = session.step(&STREAM.as_bytes()[offset..end], end == STREAM.len(), |event| {
            if let EventData::ToolReady(tool) = event.data() {
                let invocation = Invocation::from_zcall(tool);
                assert!(tools.get(invocation.name).is_some());
                assert!(ready < 2);
                let slot = ready;
                let id = invocation.call_id.as_bytes();
                let name = invocation.name.as_bytes();
                assert!(id.len() <= call_ids[slot].len());
                assert!(name.len() <= call_names[slot].len());
                assert!(invocation.arguments.len() <= call_arguments[slot].len());
                call_ids[slot][..id.len()].copy_from_slice(id);
                call_id_lens[slot] = id.len();
                call_names[slot][..name.len()].copy_from_slice(name);
                call_name_lens[slot] = name.len();
                call_arguments[slot][..invocation.arguments.len()]
                    .copy_from_slice(invocation.arguments);
                call_argument_lens[slot] = invocation.arguments.len();
                let written = executor
                    .execute(invocation.name, invocation.arguments, &mut scratch)
                    .expect("execute authorized example tool");
                assert!(written <= results[slot].len());
                results[slot][..written].copy_from_slice(&scratch[..written]);
                result_lens[slot] = written;
                ready += 1;
            }
            Control::Continue
        });
        offset += step.consumed;
        match step.status {
            StepStatus::NeedInput | StepStatus::NeedEvents => {}
            StepStatus::Done => break,
            StepStatus::NeedBuffer(_) => panic!("example workspace is too small"),
            StepStatus::Failed(error) => panic!("stream failed: {error:?}"),
        }
    }
    assert_eq!(offset, STREAM.len());
    assert_eq!(ready, 2);

    let call_id_a = core::str::from_utf8(&call_ids[0][..call_id_lens[0]]).unwrap();
    let call_id_b = core::str::from_utf8(&call_ids[1][..call_id_lens[1]]).unwrap();
    let call_name_a = core::str::from_utf8(&call_names[0][..call_name_lens[0]]).unwrap();
    let call_name_b = core::str::from_utf8(&call_names[1][..call_name_lens[1]]).unwrap();
    let call_args_a =
        core::str::from_utf8(&call_arguments[0][..call_argument_lens[0]]).unwrap();
    let call_args_b =
        core::str::from_utf8(&call_arguments[1][..call_argument_lens[1]]).unwrap();
    let result_a = core::str::from_utf8(&results[0][..result_lens[0]]).unwrap();
    let result_b = core::str::from_utf8(&results[1][..result_lens[1]]).unwrap();

    // zcall validates that each result belongs to a completed ToolReady call.
    session.submit_tool_result(call_id_a).expect("submit first tool result");
    session.submit_tool_result(call_id_b).expect("submit second tool result");

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
            id: Presence::Value(Text::new(call_id_a)),
            name: Text::new(call_name_a),
            arguments: Data::JsonText(Text::new(call_args_a)),
            legacy: false,
        },
        Content::ToolCall {
            id: Presence::Value(Text::new(call_id_b)),
            name: Text::new(call_name_b),
            arguments: Data::JsonText(Text::new(call_args_b)),
            legacy: false,
        },
    ];
    builder
        .message(Role::Assistant, &calls)
        .expect("append assistant calls");
    let mut request = builder.finish();
    request
        .append_tool_result(
            call_id_a,
            Data::Text(Text::new(result_a)),
            Presence::Missing,
        )
        .expect("append lookup result");
    request
        .append_tool_result(
            call_id_b,
            Data::Text(Text::new(result_b)),
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
