#![no_std]
#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

pub mod bridge;
pub mod cluster;
#[doc = include_str!("../COMMANDS.md")]
pub mod command;
pub mod context;
pub mod conversation;
pub mod execution;
pub mod hooks;
#[doc = include_str!("../LIFECYCLE.md")]
pub mod lifecycle;
mod native_lifecycle;
pub mod output;
#[doc = include_str!("../PLUGINS.md")]
pub mod plugin;
pub mod request;
pub mod response;
pub mod settings;
#[doc = include_str!("../SKILLS.md")]
pub mod skills;
pub mod tools;
pub mod turn;
pub mod validation;

pub use bridge::Invocation;
pub use output::ToolResultTransform;
pub use tools::{Executor, Tool, ToolOutput, ToolSet};
pub use turn::ToolLoop;
pub use zcall;
pub use zplugin;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Local failures are bounded values; underlying zcall diagnostics are preserved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Error {
    InvalidTool,
    DuplicateTool,
    UnknownTool,
    InvalidArguments {
        offset: usize,
    },
    StringLimit {
        offset: usize,
        limit: usize,
    },
    IntegerOverflow {
        offset: usize,
    },
    /// Units are bytes for byte buffers and slots for descriptor/queue storage.
    Capacity {
        needed: usize,
    },
    InvalidOutput,
    Denied,
    ExecutionFailed,
    InvalidState,
    InvalidBudget,
    BudgetExceeded {
        needed: u64,
        available: u64,
    },
    DuplicateId,
    UnknownId,
    InvalidRange,
    Pinned,
    Stale,
    Overflow,
    Busy,
    Incompatible,
    LimitExceeded,
    Zcall(zcall::v1::Error),
    /// A native plugin failed to load, bind, create or execute. Preserve its
    /// original status; a call failure does not imply side-effect rollback.
    Plugin(zplugin::Error),
}
impl From<zcall::v1::Error> for Error {
    fn from(value: zcall::v1::Error) -> Self {
        Self::Zcall(value)
    }
}
impl From<zplugin::Error> for Error {
    fn from(value: zplugin::Error) -> Self {
        Self::Plugin(value)
    }
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl core::error::Error for Error {}
