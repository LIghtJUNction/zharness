//! Import-free, allocation-free Wasm extension; operations belong to this example.
#![no_std]

use zharness::zplugin::{Error, Result, wasm_guest::Guest};

struct TextExtension {
    calls: u64,
    counter: u64,
}

impl TextExtension {
    const fn new() -> Self {
        Self {
            calls: 0,
            counter: 0,
        }
    }
}

impl Guest for TextExtension {
    fn call(&mut self, operation: u32, input: &[u8], output: &mut [u8]) -> Result<usize> {
        if operation != 7 {
            self.calls = self.calls.wrapping_add(1);
        }
        match operation {
            0 | 1 => {
                let destination = output.get_mut(..input.len()).ok_or(Error::BufferTooSmall)?;
                destination.copy_from_slice(input);
                if operation == 1 {
                    destination.make_ascii_uppercase();
                }
                Ok(input.len())
            }
            2 | 7 => {
                let destination = output.get_mut(..8).ok_or(Error::BufferTooSmall)?;
                let value = if operation == 2 {
                    self.counter = self.counter.wrapping_add(1);
                    self.counter
                } else {
                    self.calls
                };
                destination.copy_from_slice(&value.to_le_bytes());
                Ok(8)
            }
            3 => {
                output.fill(0xcc);
                Err(Error::PluginStatus(77))
            }
            4 => loop {
                core::hint::spin_loop();
            },
            5 => core::arch::wasm32::unreachable(),
            6 => {
                *output.first_mut().ok_or(Error::BufferTooSmall)? = 0xff;
                Ok(1)
            }
            8 => {
                let destination = output.get_mut(..4).ok_or(Error::BufferTooSmall)?;
                let previous_pages = core::arch::wasm32::memory_grow::<0>(65_536);
                destination.copy_from_slice(&(previous_pages as u32).to_le_bytes());
                Ok(4)
            }
            _ => Err(Error::Unsupported),
        }
    }
}

zharness::zplugin::export_wasm!(
    TextExtension,
    constructor = TextExtension::new(),
    interface = 0x7a68_6172_6e65_7373_u64,
    version = 1,
    input = 1_048_576,
    output = 1_048_576
);

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}
