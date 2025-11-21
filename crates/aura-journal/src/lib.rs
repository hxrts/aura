//! Fact-based distributed journal for Aura
//!
//! This crate provides a fact-based CRDT journal using join semilattices,
//! enabling automatic conflict resolution and convergence across authorities.
//!
//! # Architecture
//!
//! - **Facts**: Immutable, attested operations that form the journal
//! - **Reduction**: Deterministic state computation from facts
//! - **Namespaces**: Authority and context-scoped journals
//! - **Integration**: Bridge with commitment tree for AttestedOp support

// Core modules
mod error;
mod operations;
mod types;

// Domain modules moved from aura-core
pub mod journal;
pub mod effect_api;
pub mod semilattice;

// CRDT causal context module moved from aura-core
pub mod causal_context;

// New commitment tree implementation (Phase 2)
pub mod commitment_tree;

// Clean Journal API (Phase 1 API cleanup)
pub mod journal_api;

// New fact-based journal implementation (Phase 2)
pub mod commitment_integration;
pub mod fact;
pub mod fact_journal;
pub mod reduction;

// Authority state derivation (Phase 5)
pub mod authority_state;

// Note: Choreographic protocols moved to aura-sync (Layer 5)

// Test effects moved to aura-testkit to maintain clean domain layer

// Re-exports
pub use error::{AuraError, Result};
pub use operations::*;
// Note: Sync types moved to aura-sync (Layer 5)

// Core type re-exports
pub use aura_core::Hash32;

// Domain re-exports
pub use journal::*; // Now re-exports fact-based types
pub use effect_api::{
    CapabilityId, CapabilityRef, Intent, IntentId, IntentStatus, JournalMap, Priority,
};

// New fact-based journal exports
pub use fact::{
    AttestedOp as FactAttestedOp, Fact, FactContent, FactId, FlowBudgetFact, RelationalFact,
    SnapshotFact,
};
pub use fact_journal::{Journal as FactJournal, JournalNamespace};
pub use reduction::{reduce_authority, reduce_context, RelationalState};
// Note: TreeOp and TreeOpRecord are now aura_core::tree::TreeOpKind and aura_core::tree::AttestedOp
pub use aura_core::tree::{AttestedOp as TreeOpRecord, TreeOpKind as TreeOp};
// Primary Journal API (STABLE)
pub use journal_api::{AccountSummary, Journal, JournalFact};

// CRDT Implementation Details (INTERNAL - subject to change without notice)
#[doc(hidden)]
pub use semilattice::{
    integration,
    EpochLog,
    GuardianRegistry,
    IntentPool, // DeviceRegistry removed - use authority-based TreeState instead
    JournalMap as CRDTJournalMap,
    MaxCounter,
    ModernAccountState as AccountState,
    OpLog,
};

// New commitment tree re-exports (tree types moved from aura-core)
pub use commitment_tree::{
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
pub use types::{GuardianMetadata, Session};

// DeviceMetadata and DeviceType are deprecated - use authority-derived views instead
// See docs/100_authority_and_identity.md for migration guidance

// Tests
#[cfg(test)]
mod tests;
