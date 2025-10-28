//! Protocol execution infrastructure
//!
//! This module provides the infrastructure for executing choreographic protocols:
//! - Protocol-specific contexts for DKD, Resharing, Recovery, Locking
//! - `TimeSource` - Time abstraction for simulation vs production
//! - Core types for protocol instructions and results
//! - Helper abstractions for reducing choreography boilerplate

pub mod base_context;
pub mod context;
pub mod helpers;
pub mod protocol_contexts;
pub mod time;
pub mod types;

// Re-export common types
pub use base_context::{BaseContext, Transport};
pub use context::{ProtocolContext, MemoryTransport};
pub use helpers::*;
pub use protocol_contexts::{
    CompactionContext, DkdContext, LockingContext, ProtocolContextTrait, RecoveryContext,
    ResharingContext,
};
pub use time::*;
pub use types::*;
