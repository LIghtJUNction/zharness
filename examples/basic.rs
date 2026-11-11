use zharness::{Tool, ToolSet};
static TOOLS: &[Tool<'static>] = &[Tool::new("echo", "Echo validated input", r#"{"type":"object"}"#)];
fn main() -> Result<(), zharness::Error> {
    let tools = ToolSet::new(TOOLS)?;
    assert_eq!(tools.get("echo").unwrap().description, "Echo validated input");
    println!("{} tool registered; no transport or agent loop started", tools.len());
    Ok(())
}
