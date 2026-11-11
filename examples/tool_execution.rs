use zharness::{mcp::Error, tools::{Executor, Tool}};

struct Echo;

impl Executor for Echo {
    fn execute(
        &mut self,
        name: &str,
        arguments: &[u8],
        output: &mut [u8],
    ) -> Result<usize, Error> {
        if name != "echo" {
            return Err(Error::UnknownTool);
        }
        let len = arguments.len().min(output.len());
        output[..len].copy_from_slice(&arguments[..len]);
        if len != arguments.len() {
            return Err(Error::Capacity);
        }
        Ok(len)
    }
}

static ECHO: Tool<'static> = Tool {
    name: "echo",
    description: "Echo a JSON object",
    input_schema: r#"{"type":"object"}"#,
};

fn main() -> Result<(), Error> {
    let mut executor = Echo;
    let mut output = [0; 64];
    let n = executor.execute("echo", br#"{"ok":true}"#, &mut output)?;
    assert_eq!(&output[..n], br#"{"ok":true}"#);
    let _ = ECHO;
    Ok(())
}
