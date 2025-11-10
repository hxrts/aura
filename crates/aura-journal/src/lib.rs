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
pub mod sync;
mod types;

// Domain modules moved from aura-core
pub mod journal;
pub mod ledger;
pub mod semilattice;

// New ratchet tree implementation (Phase 2)
pub mod ratchet_tree;

// Re-exports
pub use effects::*;
pub use error::{AuraError, Result};
pub use operations::*;
pub use sync::*;

// Domain re-exports
pub use journal::*;
pub use ledger::{
    CapabilityId, CapabilityRef, Intent, IntentId, IntentStatus, JournalMap, Priority,
};
// Note: TreeOp and TreeOpRecord are now aura_core::tree::TreeOpKind and aura_core::tree::AttestedOp
pub use aura_core::tree::{AttestedOp as TreeOpRecord, TreeOpKind as TreeOp};
pub use semilattice::{
    integration, DeviceRegistry, EpochLog, GuardianRegistry, IntentPool,
    JournalMap as CRDTJournalMap, MaxCounter, ModernAccountState as AccountState, OpLog,
};

// New ratchet tree re-exports
pub use ratchet_tree::{reduce, TreeState};

// Selective re-exports to avoid conflicts
pub use middleware::{JournalHandler, JournalMiddleware};
pub use types::{DeviceMetadata, DeviceType, GuardianMetadata, Session};
