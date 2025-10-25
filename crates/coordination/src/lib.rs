//! Protocol Coordination for Aura
//!
//! This crate provides coordination infrastructure for Aura's distributed protocols.

#![allow(
    missing_docs,
    dead_code,
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::too_many_arguments,
    clippy::manual_async_fn,
    clippy::large_enum_variant,
    clippy::single_match,
    clippy::format_in_format_args,
    clippy::default_constructed_unit_structs
)]

// ========== Error Types ==========
pub mod error;
pub use error::{CoordinationError, Result};

// ========== Basic Infrastructure ==========
pub mod utils;

// ========== Legacy Types ==========
pub mod types;
pub use types::*;

// Re-export utilities that compile
pub use utils::{compute_lottery_ticket, determine_lock_winner};

// ========== Protocol Execution ==========
pub mod execution;
pub use execution::{ProductionTimeSource, ProtocolContext, StubTransport, Transport};

// ========== Choreography ==========
pub mod choreography;

// ========== Tracing and Logging ==========
pub mod tracing;
pub use tracing::*;

// ========== Test Utilities ==========
#[cfg(test)]
#[allow(warnings, clippy::all)]
pub mod test_utils;

// ========== Error Recovery ==========
pub mod error_recovery;
pub use error_recovery::*;

// ========== Local Session Runtime ==========
pub mod local_runtime;
pub use local_runtime::{
    DkdResult, LocalSessionRuntime, SessionCommand, SessionProtocolType, SessionResponse,
};
