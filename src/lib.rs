#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Small, composable primitives for building AI harnesses.
//!
//! zharness is not an agent runtime. It provides bounded MCP/JSON-RPC building
//! blocks and bridges zcall's normalized tool events to caller-owned executors.
//! Scheduling, model selection, transport policy, permissions and tool business
//! logic remain the application's responsibility.

mod json;
pub mod mcp;
pub mod tools;
pub mod transport;

/// Re-export the model-call contract used by this crate.
pub use zcall;

/// Library version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
