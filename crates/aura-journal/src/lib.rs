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

// CRDT causal context module moved from aura-core
pub mod causal_context;

// New ratchet tree implementation (Phase 2)
pub mod ratchet_tree;

// Clean Journal API (Phase 1 API cleanup)
pub mod journal_api;

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
// Primary Journal API (STABLE)
pub use journal_api::{AccountSummary, Journal, JournalFact};

// CRDT Implementation Details (INTERNAL - subject to change without notice)
#[doc(hidden)]
pub use semilattice::{
    integration, DeviceRegistry, EpochLog, GuardianRegistry, IntentPool,
    JournalMap as CRDTJournalMap, MaxCounter, ModernAccountState as AccountState, OpLog,
};

// New ratchet tree re-exports (tree types moved from aura-core)
pub use ratchet_tree::{
    // Re-export tree types for consumers that expect them from aura-journal
    commit_branch,
    commit_leaf,
    compute_root_commitment,
    policy_hash,
    reduce,
    AttestedOp,
    BranchNode,
    Epoch,
    LeafId,
    LeafNode,
    LeafRole,
    NodeIndex,
    NodeKind,
    Policy,
    TreeCommitment,
    TreeOpKind,
    TreeState,
};

// Causal context re-exports
pub use causal_context::{ActorId, CausalContext, OperationId, VectorClock};

// Selective re-exports to avoid conflicts
pub use middleware::JournalContext;
pub use types::{DeviceMetadata, DeviceType, GuardianMetadata, Session};

// Tests
#[cfg(test)]
mod tests;
