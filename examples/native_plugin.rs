//! A separately built native library. Operations and payloads belong to the plugin.
use zharness::plugin::{Extension, ExtensionExport};
use zplugin::{Error, Result};

struct TextExtension;

impl Extension for TextExtension {
    fn call(&mut self, operation: &str, input: &[u8], output: &mut [u8]) -> Result<u64> {
        if !matches!(operation, "echo" | "uppercase") {
            return Err(Error::Unsupported);
        }
        let destination = output.get_mut(..input.len()).ok_or(Error::BufferTooSmall)?;
        match operation {
            "echo" => destination.copy_from_slice(input),
            "uppercase" => {
                for (source, target) in input.iter().zip(destination) {
                    *target = source.to_ascii_uppercase();
                }
            }
            _ => unreachable!(),
        }
        Ok(input.len() as u64)
    }
}

fn create() -> Result<TextExtension> {
    Ok(TextExtension)
}

// SAFETY: This implementation has no background activity, external registrations,
// callbacks, TLS state or resources that survive the tracked plugin instance.
unsafe impl zplugin::export::UnloadSafe for TextExtension {}

zplugin::export!(
    ExtensionExport<TextExtension>,
    constructor = create,
    version = (1, 0, 0),
    unload = cooperative
);
