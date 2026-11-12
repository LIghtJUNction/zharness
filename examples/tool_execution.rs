use zharness::{
    Error, Executor, Invocation, Tool, ToolOutput, ToolSet,
    validation::{ArgumentLimits, Arguments},
    zcall::v1::TokenStorage,
};
struct Echo;
impl Executor for Echo {
    fn execute(
        &mut self,
        _name: &str,
        arguments: Arguments<'_>,
        output: &mut [u8],
    ) -> Result<ToolOutput, Error> {
        let raw = arguments.as_bytes();
        let dst = output
            .get_mut(..raw.len())
            .ok_or(Error::Capacity { needed: raw.len() })?;
        dst.copy_from_slice(raw);
        Ok(ToolOutput::success(raw.len()))
    }
}
fn main() -> Result<(), Error> {
    let definitions = [Tool::new("echo", "Echo", r#"{"type":"object"}"#)];
    let tools = ToolSet::new(&definitions)?;
    let invocation = Invocation {
        signature: None,
        call_id: "app-1",
        name: "echo",
        arguments: br#"{"message":"hello"}"#,
    };
    let prepared = invocation.prepare(
        &tools,
        &mut TokenStorage::<32>::new(),
        ArgumentLimits::default(),
    )?;
    // The application explicitly allows this harmless local operation.
    let mut output = [0; 256];
    let completed = prepared.execute(&mut Echo, &mut output)?;
    println!("{}: {}", completed.call_id, completed.text);
    Ok(())
}
