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
pub mod journal;
pub mod ledger;
pub mod semilattice;
pub mod tree;

// Re-exports
pub use effects::*;
pub use error::{Error, Result};
pub use operations::*;
pub use state::*;
pub use sync::*;

// Domain re-exports
pub use journal::*;
pub use ledger::{
    CapabilityId, CapabilityRef, Intent, IntentId, IntentStatus, JournalMap, Priority, TreeOp,
    TreeOpRecord,
};
pub use semilattice::{
    integration, DeviceRegistry, EpochLog, GuardianRegistry, IntentPool,
    JournalMap as CRDTJournalMap, MaxCounter, ModernAccountState,
};
pub use tree::*;

// Selective re-exports to avoid conflicts
pub use middleware::{JournalHandler, JournalMiddleware};
pub use types::{DeviceMetadata, DeviceType, GuardianMetadata, Session};
