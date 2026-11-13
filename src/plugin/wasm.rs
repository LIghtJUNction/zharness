//! Named operations over zplugin's isolated, synchronous Wasm byte-buffer protocol.
//!
//! Loading does not grant filesystem, network, WASI or host callbacks. Native
//! loading remains a separate unsafe API; module filenames never select trust.
//! Guest-memory limits are not process RSS limits and fuel is not a compilation
//! deadline. High-risk hosts can add process isolation outside this library.

use core::fmt;

use crate::{Error, Executor, ToolOutput, tools::validate_name, validation::Arguments};
use zplugin::wasm::Plugin as Sandbox;
pub use zplugin::wasm::{Error as SandboxError, Interface, Limits};

/// A host-approved name and its plugin-defined numeric operation. Several names
/// may intentionally alias one numeric operation; names themselves must be unique.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Operation<'a> {
    pub name: &'a str,
    pub code: u32,
}

impl<'a> Operation<'a> {
    pub const fn new(name: &'a str, code: u32) -> Self {
        Self { name, code }
    }
}

/// Bounded execution errors for the existing Copy harness error and hook events.
/// The complete engine diagnostic is retained by [`WasmExtension::last_error`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Fault {
    InputTooLarge { size: usize, limit: usize },
    GuestStatus(i32),
    InvalidOutput { size: usize, limit: usize },
    FuelExhausted,
    StackExhausted,
    Trap,
    Poisoned,
    Engine,
}

impl From<&SandboxError> for Fault {
    fn from(error: &SandboxError) -> Self {
        match *error {
            SandboxError::InputTooLarge { size, limit } => Self::InputTooLarge { size, limit },
            SandboxError::GuestStatus(status) => Self::GuestStatus(status),
            SandboxError::InvalidOutput { size, limit } => Self::InvalidOutput { size, limit },
            SandboxError::FuelExhausted => Self::FuelExhausted,
            SandboxError::StackExhausted => Self::StackExhausted,
            SandboxError::Trap(_) => Self::Trap,
            SandboxError::Poisoned => Self::Poisoned,
            _ => Self::Engine,
        }
    }
}

/// Loading preserves detailed validation/compatibility diagnostics without
/// changing the allocation-free error representation used throughout zharness.
#[derive(Debug)]
pub enum LoadError {
    Configuration(Error),
    Sandbox(SandboxError),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(error) => write!(f, "invalid Wasm operation map: {error}"),
            Self::Sandbox(error) => error.fmt(f),
        }
    }
}

impl core::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Configuration(error) => Some(error),
            Self::Sandbox(error) => Some(error),
        }
    }
}

/// One stateful sandbox with a caller-owned operation allowlist. It is neither
/// Send nor Sync; the runtime owns its memory and runs only during explicit calls.
///
/// Tool visibility and authorization remain with Registry and execution hooks.
/// Direct `call_bytes` bypasses those hooks, like the native byte-call API.
pub struct WasmExtension<'a> {
    sandbox: Sandbox,
    interface: Interface,
    operations: &'a [Operation<'a>],
    last_error: Option<SandboxError>,
}

impl<'a> WasmExtension<'a> {
    /// Validate the host map, then validate, instantiate and bind a binary module.
    /// Callers loading a file should bound the read by `limits.max_module_bytes`.
    pub fn load(
        bytes: &[u8],
        interface: Interface,
        operations: &'a [Operation<'a>],
        limits: Limits,
    ) -> Result<Self, LoadError> {
        for (index, operation) in operations.iter().enumerate() {
            validate_name(operation.name).map_err(LoadError::Configuration)?;
            if operations[..index]
                .iter()
                .any(|prior| prior.name == operation.name)
            {
                return Err(LoadError::Configuration(Error::DuplicateTool));
            }
        }
        Ok(Self {
            sandbox: Sandbox::new(bytes, interface, limits).map_err(LoadError::Sandbox)?,
            interface,
            operations,
            last_error: None,
        })
    }

    /// Call exactly once. The engine copies successful bytes into caller storage;
    /// engine errors and unknown operation names leave that storage unchanged.
    pub fn call_bytes<'o>(
        &mut self,
        name: &str,
        input: &[u8],
        output: &'o mut [u8],
    ) -> Result<&'o [u8], Error> {
        self.last_error = None;
        let operation = self
            .operations
            .iter()
            .find(|operation| operation.name == name)
            .ok_or(Error::UnknownTool)?;
        match self.sandbox.call(operation.code, input, output) {
            Ok(written) => Ok(&output[..written]),
            Err(error) => {
                let fault = Fault::from(&error);
                self.last_error = Some(error);
                Err(Error::Wasm(fault))
            }
        }
    }

    /// Validate UTF-8 after a successful byte call. Invalid UTF-8 produces
    /// `InvalidOutput`; bytes are already copied and the instance remains usable.
    pub fn call_text<'o>(
        &mut self,
        name: &str,
        input: &[u8],
        output: &'o mut [u8],
    ) -> Result<&'o str, Error> {
        core::str::from_utf8(self.call_bytes(name, input, output)?)
            .map_err(|_| Error::InvalidOutput)
    }

    /// Validate a fresh instance of the same interface before replacing state.
    /// Failure leaves the current instance and its diagnostic untouched. Success
    /// drops old state without guest destructors; no state migration is implicit.
    /// For application-specific initialization, load a separate candidate and
    /// validate its operations before replacing the caller's Router slot.
    pub fn reload(&mut self, bytes: &[u8], limits: Limits) -> Result<(), SandboxError> {
        let candidate = Sandbox::new(bytes, self.interface, limits)?;
        self.sandbox = candidate;
        self.last_error = None;
        Ok(())
    }

    /// Full sandbox error for the most recent call. Cleared before every attempt,
    /// including unknown names, and after successful reload. Host UTF-8 errors
    /// have no engine diagnostic because the byte call succeeded.
    pub fn last_error(&self) -> Option<&SandboxError> {
        self.last_error.as_ref()
    }

    pub fn is_poisoned(&self) -> bool {
        self.sandbox.is_poisoned()
    }

    pub fn interface(&self) -> Interface {
        self.interface
    }

    pub fn operations(&self) -> &[Operation<'_>] {
        self.operations
    }

    /// Read-only runtime diagnostics and negotiated capacities; no mutable escape.
    pub fn runtime(&self) -> &Sandbox {
        &self.sandbox
    }
}

impl Executor for WasmExtension<'_> {
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
