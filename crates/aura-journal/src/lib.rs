//! Automerge-based distributed ledger for Aura
//!
//! This crate provides a CRDT-based account ledger using Automerge,
//! enabling automatic conflict resolution and convergence across devices.
//!
//! # Architecture
//!
//! - **State**: Automerge document storing account configuration
//! - **Operations**: Type-safe operations that map to Automerge changes
//! - **Effects**: Algebraic effect system for ledger operations
//! - **Sync**: Built-in protocol for efficient state synchronization

// Core modules
mod effects;
mod error;
pub mod journal_ops;
pub mod middleware;
mod operations;
mod state;
mod sync;
mod types;

// Domain modules moved from aura-types
pub mod crdt;
pub mod journal;
pub mod ledger;
pub mod tree;

// Re-exports
pub use effects::*;
pub use error::{Error, Result};
pub use journal_ops::*;
pub use middleware::*;
pub use operations::*;
pub use state::*;
pub use sync::*;
pub use types::*;

// Domain re-exports
pub use crdt::*;
pub use journal::*;
pub use ledger::*;
pub use tree::*;
