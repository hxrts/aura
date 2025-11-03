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
pub mod fabric;
pub mod middleware;
mod operations;
mod state;
mod sync;
mod types;

// Re-exports
pub use effects::*;
pub use error::{Error, Result};
pub use fabric::*;
pub use middleware::*;
pub use operations::*;
pub use state::*;
pub use sync::*;
pub use types::*;
