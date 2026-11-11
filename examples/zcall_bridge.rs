use zharness::{
    mcp::Error,
    tools::{Executor, Invocation, Tool, ToolSet},
};
use zcall::v1::ReadyTool;

struct Dispatch;
impl Executor for Dispatch {
    fn execute(&mut self, _: &str, arguments: &[u8], output: &mut [u8]) -> Result<usize, Error> {
        if arguments.len() > output.len() {
            return Err(Error::Capacity);
        }
        output[..arguments.len()].copy_from_slice(arguments);
        Ok(arguments.len())
    }
}

static TOOLS: &[Tool<'static>] = &[Tool {
    name: "echo",
    description: "Echo a JSON object",
    input_schema: r#"{"type":"object"}"#,
}];

fn dispatch_ready(
    ready: ReadyTool<'_>,
    tools: &ToolSet<'_>,
    executor: &mut impl Executor,
    output: &mut [u8],
) -> Result<usize, Error> {
    let invocation = Invocation::from_zcall(ready);
    if tools.get(invocation.name).is_none() {
        return Err(Error::UnknownTool);
    }
    executor.execute(invocation.name, invocation.arguments, output)
}

fn main() -> Result<(), Error> {
    let tools = ToolSet::new(TOOLS)?;
    let _dispatch = dispatch_ready;
    let _tools = tools;
    let _executor = Dispatch;
    Ok(())
}
