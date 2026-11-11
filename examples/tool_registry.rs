use zharness::tools::{Tool, ToolSet};

static TOOLS: &[Tool<'static>] = &[
    Tool {
        name: "search",
        description: "Search a knowledge source",
        input_schema: r#"{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}"#,
    },
    Tool {
        name: "clock",
        description: "Read an application clock",
        input_schema: r#"{"type":"object","properties":{}}"#,
    },
];

fn main() -> Result<(), zharness::mcp::Error> {
    let tools = ToolSet::new(TOOLS)?;
    assert!(tools.get("search").is_some());
    Ok(())
}
