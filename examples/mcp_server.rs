use zharness::{
    mcp::{Outcome, Server},
    tools::{Executor, Tool, ToolSet},
};

struct Noop;

impl Executor for Noop {
    fn execute(&mut self, _: &str, _: &[u8], _: &mut [u8]) -> Result<usize, zharness::mcp::Error> {
        Err(zharness::mcp::Error::UnknownTool)
    }
}

static TOOLS: &[Tool<'static>] = &[];

fn main() -> Result<(), zharness::mcp::Error> {
    let tools = ToolSet::new(TOOLS)?;
    let server = Server::new("example", env!("CARGO_PKG_VERSION"));
    let mut output = [0; 256];
    let mut scratch = [0; 128];
    let mut executor = Noop;
    let result = server.handle(
        br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
        &mut output,
        &mut scratch,
        &tools,
        &mut executor,
    )?;
    assert!(matches!(result, Outcome::Response(_)));
    Ok(())
}
