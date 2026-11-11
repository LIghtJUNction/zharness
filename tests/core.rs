use zharness::{mcp::{Outcome, Server}, tools::{Executor, Tool, ToolSet}};

struct Echo;
impl Executor for Echo {
    fn execute(&mut self, _name: &str, arguments: &[u8], output: &mut [u8]) -> Result<usize, zharness::mcp::Error> {
        let bytes = arguments;
        if bytes.len() > output.len() { return Err(zharness::mcp::Error::Capacity); }
        output[..bytes.len()].copy_from_slice(bytes);
        Ok(bytes.len())
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
        Tool { name: "echo", description: "duplicate", input_schema: r#"{"type":"object"}"# },
    ];
    assert!(matches!(ToolSet::new(&duplicate), Err(zharness::mcp::Error::DuplicateTool)));
}

#[test]
fn ping_is_a_json_rpc_response() {
    let tools = ToolSet::new(TOOLS).unwrap();
    let mut output = [0; 128];
    let mut scratch = [0; 128];
    let mut executor = Echo;
    let outcome = Server::new("test", "0.1.0")
        .handle(br#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#, &mut output, &mut scratch, &tools, &mut executor)
        .unwrap();
    let Outcome::Response(len) = outcome else { panic!("expected response"); };
    assert_eq!(&output[..len], br#"{"jsonrpc":"2.0","id":7,"result":{}}"#);
}

#[test]
fn malformed_json_is_rejected_before_dispatch() {
    let tools = ToolSet::new(TOOLS).unwrap();
    let mut output = [0; 128];
    let mut scratch = [0; 128];
    let mut executor = Echo;
    let result = Server::new("test", "0.1.0")
        .handle(br#"{"jsonrpc":"2.0","id":1,"method":"ping","broken" 3}"#, &mut output, &mut scratch, &tools, &mut executor);
    assert_eq!(result, Err(zharness::mcp::Error::InvalidJson));
}

#[test]
fn notification_does_not_emit_a_response() {
    let tools = ToolSet::new(TOOLS).unwrap();
    let mut output = [0; 128];
    let mut scratch = [0; 128];
    let mut executor = Echo;
    let outcome = Server::new("test", "0.1.0")
        .handle(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, &mut output, &mut scratch, &tools, &mut executor)
        .unwrap();
    assert_eq!(outcome, Outcome::Notification);
}
