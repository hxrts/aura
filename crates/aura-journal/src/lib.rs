//! # Aura Journal - Layer 2: Specification (Domain Crate)
//!
//! **Purpose**: Define fact-based journal semantics and deterministic reduction logic.
//!
//! This crate provides a fact-based CRDT journal using join semilattices,
//! enabling automatic conflict resolution and convergence across authorities.
//! Facts form a join-semilattice and merge via set union. Identical fact sets
//! produce identical states across all replicas.
//!
//! # Architecture Constraints
//!
//! **Layer 2 depends only on aura-core** (foundation).
//! - YES Domain logic for journal semantics (no effects)
//! - YES Fact model, validation rules, deterministic reduction
//! - YES Semilattice operations and CRDT laws
//! - YES Implement application effects (e.g., `JournalEffects`) by composing infrastructure effects
//! - NO effect handler implementations (use aura-effects or domain crates)
//! - NO multi-party coordination (that's aura-protocol)
//! - NO runtime composition (that's aura-composition/aura-agent)
//!
//! # Key Concepts
//!
//! - **Facts**: Immutable, attested operations that form the journal
//! - **Join-semilattice**: Facts merge via set union `∪` (monotonic growth)
//! - **Reduction**: Deterministic state computation from fact set (identical inputs → identical outputs)
//! - **Namespaces**: Authority-scoped and context-scoped journals
//! - **AttestedOps**: Commitment tree updates expressed as facts

// Core modules
mod error;
mod types;

// Application effects implementation (Layer 2 pattern)
pub mod effects;

// Domain modules moved from aura-core
pub mod algebra;
pub mod crdt;
pub mod effect_api;

// CRDT causal context module moved from aura-core
pub mod causal_context;

// New commitment tree implementation (Phase 2)
pub mod commitment_tree;

// Clean Journal API (Phase 1 API cleanup)
pub mod journal_api;

// New fact-based journal implementation (Phase 2)
pub mod commitment_integration;
pub mod fact;
pub mod reduction;

// Pure functions for Aeneas translation (formal verification)
pub mod pure;

// Extensible fact type infrastructure (Open/Closed Principle)
pub mod extensibility;

// Authority state derivation (Phase 5)
pub mod authority_state;

// Domain event types

// Note: Choreographic protocols moved to aura-sync (Layer 5)

// Test effects moved to aura-testkit to maintain clean domain layer

// Re-exports
pub use error::{AuraError, Result};
// Note: Sync types moved to aura-sync (Layer 5)

// Application effect handler re-export
pub use effects::{JournalHandler, JournalHandlerFactory};

// Core type re-exports
pub use aura_core::time::OrderTime;
pub use aura_core::Hash32;

// Domain re-exports
pub use effect_api::{CapabilityId, CapabilityRef, Intent, IntentId, IntentStatus, Priority};

// New fact-based journal exports
pub use fact::{
    AckStorage, AttestedOp as FactAttestedOp, Fact, FactContent, FactOptions, GcResult,
    Journal as FactJournal, JournalNamespace, ProtocolRelationalFact, RelationalFact, SnapshotFact,
    TreeOpKind,
};
pub use reduction::{
    reduce_authority, reduce_context, ChannelEpochState, ReductionNamespaceError, RelationalState,
};
// Primary Journal API (STABLE)
pub use journal_api::{AccountSummary, CommittedFact, Journal, JournalFact};

// CRDT Implementation Details (INTERNAL - subject to change without notice)
#[doc(hidden)]
pub use algebra::{AccountState, EpochLog, GuardianRegistry, IntentPool, MaxCounter, OpLog};

// Re-export tree types from aura-core for consumers that expect them from aura-journal
pub use aura_core::tree::{
    commit_branch, commit_leaf, compute_root_commitment, policy_hash, AttestedOp, BranchNode,
    Epoch, LeafId, LeafNode, LeafRole, NodeIndex, NodeKind, Policy, TreeCommitment,
};

// Re-export commitment tree reduction and TreeState
pub use commitment_tree::{reduce, TreeState};

// Causal context re-exports
pub use aura_core::time::VectorClock;
pub use causal_context::{ActorId, CausalContext, OperationId, VectorClockExt};

// Selective re-exports to avoid conflicts
pub use types::GuardianMetadata;

// Event type re-exports

// Extensibility infrastructure re-exports
pub use extensibility::{
    decode_domain_fact, encode_domain_fact, parse_envelope, DomainFact, FactReducer, FactRegistry,
};

// See docs/102_authority_and_identity.md for migration guidance
