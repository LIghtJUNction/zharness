#![no_std]
#![forbid(unsafe_code)]
//! Caller-buffered tool composition for zcall. MCP wire handling lives in zmcp.
//! No agent loop, transport I/O, scheduling or authorization is performed here.

pub mod bridge;
pub mod tools;
pub mod validation;

pub use bridge::Invocation;
pub use tools::{Executor, Tool, ToolOutput, ToolSet};
pub use zcall;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Local failures retain their category; underlying zcall offsets are preserved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Error {
    InvalidTool,
    DuplicateTool,
    UnknownTool,
    InvalidArguments { offset: usize },
    StringLimit { offset: usize, limit: usize },
    IntegerOverflow { offset: usize },
    Capacity { needed: usize },
    InvalidOutput,
    Denied,
    ExecutionFailed,
    Zcall(zcall::v1::Error),
}
impl From<zcall::v1::Error> for Error {
    fn from(value: zcall::v1::Error) -> Self {
        Self::Zcall(value)
    }
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl core::error::Error for Error {}
