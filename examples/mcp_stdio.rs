use std::io::{self, Read, Write};

use zharness::{
    mcp::{LineCodec, Outcome, Server},
    tools::{Executor, ToolSet},
};

struct Noop;
impl Executor for Noop {
    fn execute(&mut self, _: &str, _: &[u8], _: &mut [u8]) -> Result<usize, zharness::mcp::Error> {
        Err(zharness::mcp::Error::UnknownTool)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tools = ToolSet::new(&[])?;
    let server = Server::new("stdio-example", env!("CARGO_PKG_VERSION"));
    let mut executor = Noop;
    let mut input = [0u8; 16 * 1024];
    let mut output = [0u8; 16 * 1024];
    let mut scratch = [0u8; 8 * 1024];
    let mut used = 0usize;
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut stdout = io::stdout().lock();
    let mut byte = [0u8; 1];

    while reader.read(&mut byte)? == 1 {
        if used == input.len() {
            return Err("stdio frame exceeds configured input capacity".into());
        }
        input[used] = byte[0];
        used += 1;
        if byte[0] != b'\n' {
            continue;
        }
        let frame = LineCodec::first(&input[..used]).ok_or("missing line boundary")?;
        if let Outcome::Response(len) = server.handle(
            frame,
            &mut output,
            &mut scratch,
            &tools,
            &mut executor,
        )? {
            stdout.write_all(&output[..len])?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
        used = 0;
    }
    if used != 0 {
        return Err("unterminated stdio frame".into());
    }
    Ok(())
}
