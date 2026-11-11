//! Native contract fixture. Invalid output lengths are values, never memory writes.
use zharness::plugin::{Extension, ExtensionExport};
use zplugin::{Error, Result};

struct Fixture {
    calls: u64,
}

impl Extension for Fixture {
    fn call(&mut self, operation: &str, input: &[u8], output: &mut [u8]) -> Result<u64> {
        if operation == "calls" {
            let target = output.get_mut(..8).ok_or(Error::BufferTooSmall)?;
            target.copy_from_slice(&self.calls.to_le_bytes());
            return Ok(8);
        }
        self.calls += 1;
        match operation {
            "echo" => {
                output
                    .get_mut(..input.len())
                    .ok_or(Error::BufferTooSmall)?
                    .copy_from_slice(input);
                Ok(input.len() as u64)
            }
            "fail" => Err(Error::PluginStatus(1701)),
            "empty" => Ok(0),
            "overreport" => Ok(output.len() as u64 + 1),
            "huge" => Ok(u64::MAX),
            "invalid_utf8" => {
                *output.first_mut().ok_or(Error::BufferTooSmall)? = 0xff;
                Ok(1)
            }
            "panic" => panic!("intentional native fixture panic"),
            _ => Err(Error::Unsupported),
        }
    }
}

fn create() -> Result<Fixture> {
    Ok(Fixture { calls: 0 })
}

// SAFETY: Only the tracked object owns state. There are no threads, callbacks,
// external registrations or destructors that need code after instance destruction.
unsafe impl zplugin::export::UnloadSafe for Fixture {}

zplugin::export!(
    ExtensionExport<Fixture>,
    constructor = create,
    version = (1, 0, 0),
    unload = cooperative
);
