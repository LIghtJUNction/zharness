use zharness::{
    mcp::{Outcome, Server},
    tools::{Executor, ToolSet},
};

fn on_complete_message<E: Executor>(
    server: &Server<'_>,
    frame: &[u8],
    output: &mut [u8],
    scratch: &mut [u8],
    tools: &ToolSet<'_>,
    executor: &mut E,
) -> Result<Outcome, zharness::mcp::Error> {
    server.handle(frame, output, scratch, tools, executor)
}

fn main() {
    // A TCP, HTTP, WebSocket or in-memory transport calls this function only
    // after it has assembled one complete JSON-RPC message.
    let _ = on_complete_message::<Never>(
        &Server::new("custom", "0.1.0"),
        b"",
        &mut [],
        &mut [],
        &ToolSet::new(&[]).unwrap(),
        &mut Never,
    );
}

struct Never;
impl Executor for Never {
    fn execute(&mut self, _: &str, _: &[u8], _: &mut [u8]) -> Result<usize, zharness::mcp::Error> {
        Err(zharness::mcp::Error::UnknownTool)
    }
}
