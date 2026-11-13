//! Explicit plugin loading, versioned byte calls and caller-owned execution routes.
//!
//! [`Plugin`](crate::plugin::Plugin) is zplugin's actual loader: native loading remains an explicit
//! unsafe operation in the host. After loading, [`ExtensionApi::create`](crate::plugin::ExtensionApi::create) binds a
//! checked interface and creates an instance whose proxy borrows the library.
//! The application owns operation names, payload formats, permission decisions,
//! scheduling and lifecycle policy. No plugin discovery or execution is implicit.

use crate::{Error, Executor, ToolOutput, validation::Arguments};

pub use zplugin::{Plugin, UnloadPolicy};

mod router;
pub use router::{Route, Router, Slot};

/// Optional import-free Wasm sandbox and named-operation Executor adapter.
#[cfg(feature = "wasm")]
pub mod wasm;

/// A single synchronous extension point with caller-owned input and output.
///
/// Return the number of initialized bytes written into `output`. Inputs and the
/// output buffer are borrowed for this call only. There is no mandatory payload
/// encoding, operation registry or semantic interpretation at this boundary.
/// A failure may follow external effects or partial output writes: discard that
/// output and let the application decide recovery. No automatic retry occurs.
///
/// Interface ID, version, method name/order and signature form the v1 contract.
/// Change the interface version when changing this shape. Payload schema versions
/// and operation meanings remain separate application contracts.
#[zplugin::interface(id = "dev.project-z.zharness.extension", version = 1)]
pub trait Extension {
    fn call(
        &mut self,
        operation: &str,
        input: &[u8],
        output: &mut [u8],
    ) -> Result<u64, zplugin::Error>;
}

impl ExtensionProxy<'_> {
    /// Call once and return only the initialized prefix of the caller's buffer.
    /// Invalid or unrepresentable lengths are rejected, including on 32-bit hosts.
    /// Returned bytes borrow `output`, not this proxy or the loaded library.
    pub fn call_bytes<'o>(
        &mut self,
        operation: &str,
        input: &[u8],
        output: &'o mut [u8],
    ) -> Result<&'o [u8], Error> {
        let written = self.call(operation, input, output)?;
        let written = usize::try_from(written).map_err(|_| Error::InvalidOutput)?;
        output.get(..written).ok_or(Error::InvalidOutput)
    }

    /// The byte-call boundary with an additional UTF-8 output check.
    pub fn call_text<'o>(
        &mut self,
        operation: &str,
        input: &[u8],
        output: &'o mut [u8],
    ) -> Result<&'o str, Error> {
        core::str::from_utf8(self.call_bytes(operation, input, output)?)
            .map_err(|_| Error::InvalidOutput)
    }
}

/// Explicit bridge to the existing tool execution boundary. Tool names become
/// operation names; input is the original validated JSON object, without copying
/// or re-encoding. The caller's Registry controls exposed tools. Normal returns
/// are successful ToolOutputs; native failures retain their zplugin error. A
/// host-specific business-result protocol can be implemented in an outer adapter.
/// Use Prepared::execute_with_hooks for the existing authorization/observation
/// callbacks immediately around the actual native call.
impl Executor for ExtensionProxy<'_> {
    fn execute(
        &mut self,
        name: &str,
        arguments: Arguments<'_>,
        output: &mut [u8],
    ) -> Result<ToolOutput, Error> {
        let text = self.call_text(name, arguments.as_bytes(), output)?;
        Ok(ToolOutput::success(text.len()))
    }
}
