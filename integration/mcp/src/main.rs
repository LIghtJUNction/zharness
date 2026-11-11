use zharness::{Error, Executor, Tool, ToolOutput, ToolSet};
use zharness::validation::{ArgumentLimits, Arguments};
use zharness::zcall::v1::TokenStorage;
use zmcp::json::{Document, Limits, Token};
use zmcp::mcp::{Action, Server};
use zmcp::tools::{Content, ToolResult};

struct Echo;
impl Executor for Echo {
    fn execute(&mut self, name: &str, arguments: Arguments<'_>, output: &mut [u8]) -> Result<ToolOutput, Error> {
        if name != "echo" { return Err(Error::Denied); }
        let bytes = arguments.as_bytes();
        output.get_mut(..bytes.len()).ok_or(Error::Capacity { needed: bytes.len() })?.copy_from_slice(bytes);
        Ok(ToolOutput::success(bytes.len()))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let descriptors = [Tool::new("echo", "Echo explicitly allowed input", r#"{"type":"object"}"#)];
    let registry = ToolSet::new(&descriptors)?;
    let request = br#"{"jsonrpc":"2.0","id":"mcp-request-7","method":"tools/call","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}},"name":"echo","arguments":{"text":"hello"}}}"#;
    let mut server = Server::new("composed", "1");
    let mut tokens = [Token::EMPTY; 128];
    let mut response = [0; 2048];
    let Action::CallTool(call) = server.handle(request, &mut tokens, Limits::default(), &mut response)? else {
        return Err("expected tools/call action".into());
    };
    let mut name_storage = [0; 128];
    let name = call.name.decode_into(&mut name_storage)?;
    if registry.get(name).is_none() { return Err("unregistered tool".into()); }
    // Application authorization precedes execution. MCP IDs remain in the MCP context;
    // they are not invented model-tool IDs and are never submitted to a zcall Session.
    if name != "echo" { return Err("not authorized".into()); }
    let mut validation_tokens = TokenStorage::<64>::new();
    let arguments = Arguments::parse(call.arguments_bytes(), &mut validation_tokens, ArgumentLimits::default())?;
    let mut tool_output = [0; 512];
    let result = Echo.execute(name, arguments, &mut tool_output)?;
    let text = core::str::from_utf8(tool_output.get(..result.written).ok_or("invalid executor length")?)?;
    let content = [Content::Text(text)];
    let n = server.tool_result(call.context, ToolResult { content: &content, structured_content: None, is_error: result.is_error }, &mut response)?;
    let mut verify_tokens = [Token::EMPTY; 128];
    let parsed = Document::parse(&response[..n], &mut verify_tokens, Limits::default())?;
    assert!(parsed.root().get("id").unwrap().string().unwrap().matches("mcp-request-7"));
    assert_eq!(parsed.root().get("result").unwrap().get("isError").unwrap().boolean(), Some(false));
    println!("{}", core::str::from_utf8(&response[..n])?);
    Ok(())
}
