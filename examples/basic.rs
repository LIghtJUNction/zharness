use zharness::tools::{Tool, ToolSet};

static TOOLS: &[Tool<'static>] = &[Tool {
    name: "echo",
    description: "Return the supplied text",
    input_schema: r#"{"type":"object","properties":{"text":{"type":"string"}}}"#,
}];

fn main() -> Result<(), zharness::mcp::Error> {
    let tools = ToolSet::new(TOOLS)?;
    assert_eq!(tools.len(), 1);
    Ok(())
}
