use zharness::{
    mcp::{Outcome, Server},
    tools::{Executor, Tool, ToolSet},
};

struct Echo;

impl Executor for Echo {
    fn execute(
        &mut self,
        _name: &str,
        arguments: &[u8],
        output: &mut [u8],
    ) -> Result<usize, zharness::mcp::Error> {
        if arguments.len() > output.len() {
            return Err(zharness::mcp::Error::Capacity);
        }
        output[..arguments.len()].copy_from_slice(arguments);
        Ok(arguments.len())
    }
}

static TOOLS: &[Tool<'static>] = &[Tool {
    name: "echo",
    description: "Return the arguments",
    input_schema: r#"{"type":"object"}"#,
}];

#[test]
fn registry_rejects_duplicate_names() {
    let duplicate = [
        TOOLS[0],
        Tool {
            name: "echo",
            description: "duplicate",
            input_schema: r#"{"type":"object"}"#,
        },
    ];
    assert!(matches!(
        ToolSet::new(&duplicate),
        Err(zharness::mcp::Error::DuplicateTool)
    ));
}

#[test]
fn ping_is_a_json_rpc_response() {
    let tools = ToolSet::new(TOOLS).unwrap();
    let mut output = [0; 128];
    let mut scratch = [0; 128];
    let mut executor = Echo;
    let outcome = Server::new("test", "0.1.0")
        .handle(
            br#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#,
            &mut output,
            &mut scratch,
            &tools,
            &mut executor,
        )
        .unwrap();
    let Outcome::Response(len) = outcome else {
        panic!("expected response");
    };
    assert_eq!(&output[..len], br#"{"jsonrpc":"2.0","id":7,"result":{}}"#);
}

#[test]
fn malformed_json_is_rejected_before_dispatch() {
    let tools = ToolSet::new(TOOLS).unwrap();
    let mut output = [0; 128];
    let mut scratch = [0; 128];
    let mut executor = Echo;
    let result = Server::new("test", "0.1.0").handle(
        br#"{"jsonrpc":"2.0","id":1,"method":"ping","broken" 3}"#,
        &mut output,
        &mut scratch,
        &tools,
        &mut executor,
    );
    assert_eq!(result, Err(zharness::mcp::Error::InvalidJson));
}

#[test]
fn notification_does_not_emit_a_response() {
    let tools = ToolSet::new(TOOLS).unwrap();
    let mut output = [0; 128];
    let mut scratch = [0; 128];
    let mut executor = Echo;
    let outcome = Server::new("test", "0.1.0")
        .handle(
            br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            &mut output,
            &mut scratch,
            &tools,
            &mut executor,
        )
        .unwrap();
    assert_eq!(outcome, Outcome::Notification);
}


#[test]
fn modern_discovery_advertises_current_revision() {
    let tools = ToolSet::new(TOOLS).unwrap();
    let mut output = [0; 512];
    let mut scratch = [0; 128];
    let mut executor = Echo;
    let request = br#"{"jsonrpc":"2.0","id":"d","method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#;
    let Outcome::Response(len) = Server::new("test", "0.1.0")
        .handle(request, &mut output, &mut scratch, &tools, &mut executor)
        .unwrap()
    else {
        panic!("expected discovery response");
    };
    let response = core::str::from_utf8(&output[..len]).unwrap();
    assert!(response.contains("2026-07-28"));
    assert!(response.contains(r#""resultType":"complete""#));
}

#[test]
fn modern_tools_list_uses_complete_result_envelope() {
    let tools = ToolSet::new(TOOLS).unwrap();
    let mut output = [0; 512];
    let mut scratch = [0; 128];
    let mut executor = Echo;
    let request = br#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#;
    let Outcome::Response(len) = Server::new("test", "0.1.0")
        .handle(request, &mut output, &mut scratch, &tools, &mut executor)
        .unwrap()
    else {
        panic!("expected list response");
    };
    let response = core::str::from_utf8(&output[..len]).unwrap();
    assert!(response.contains(r#""resultType":"complete""#));
    assert!(response.contains(r#""name":"echo""#));
}

#[test]
fn malformed_schema_is_rejected() {
    let invalid = [Tool {
        name: "bad",
        description: "invalid schema",
        input_schema: r#"{"type": object}"#,
    }];
    assert!(matches!(
        ToolSet::new(&invalid),
        Err(zharness::mcp::Error::InvalidTool)
    ));
}


#[test]
fn line_decoder_handles_split_input_and_multiple_frames() {
    let mut storage = [0u8; 32];
    let mut decoder = zharness::transport::LineDecoder::new(&mut storage);
    assert_eq!(decoder.push(b"{\"jsonrpc\":\"2.0\"").unwrap(), 15);
    assert!(decoder.message().is_none());
    let consumed = decoder.push(b"}\r\n{\"id\":2}\n").unwrap();
    assert_eq!(consumed, 18);
    assert_eq!(decoder.message(), Some(&b"{\"jsonrpc\":\"2.0\"}"[..]));
    decoder.consume().unwrap();
    assert_eq!(decoder.message(), Some(&b"{\"id\":2}"[..]));
    decoder.consume().unwrap();
    assert_eq!(decoder.buffered_len(), 0);
}

#[test]
fn line_decoder_capacity_error_does_not_consume_input() {
    let mut storage = [0u8; 3];
    let mut decoder = zharness::transport::LineDecoder::new(&mut storage);
    assert_eq!(
        decoder.push(b"four"),
        Err(zharness::transport::TransportError::Capacity)
    );
    assert_eq!(decoder.buffered_len(), 0);
}
