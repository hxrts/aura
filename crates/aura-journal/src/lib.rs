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
mod error;
pub mod journal_ops;
pub mod middleware;
mod operations;
mod types;

// Domain modules moved from aura-core
pub mod journal;
pub mod ledger;
pub mod semilattice;

// New ratchet tree implementation (Phase 2)
pub mod ratchet_tree;

// Note: Choreographic protocols moved to aura-sync (Layer 5)

// Test effects moved to aura-testkit to maintain clean domain layer

// Re-exports
pub use error::{AuraError, Result};
pub use operations::*;
// Note: Sync types moved to aura-sync (Layer 5)

// Core type re-exports
pub use aura_core::Hash32;

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
pub use middleware::JournalContext;
pub use types::{DeviceMetadata, DeviceType, GuardianMetadata, Session};
