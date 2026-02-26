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
//! - Domain logic for journal semantics
//! - Fact model, validation rules, deterministic reduction
//! - Semilattice operations and CRDT laws
//! - Application effects (e.g., `JournalEffects`) composing infrastructure effects
//!
//! # Key Concepts
//!
//! - **Facts**: Immutable, attested operations that form the journal
//! - **Join-semilattice**: Facts merge via set union (monotonic growth)
//! - **Reduction**: Deterministic state computation from fact set
//! - **Namespaces**: Authority-scoped and context-scoped journals
//! - **AttestedOps**: Commitment tree updates expressed as facts

mod error;
mod types;

/// Application effects implementation
pub mod effects;

/// Domain-specific algebraic CRDT types
pub mod algebra;

/// CRDT handler implementations
pub mod crdt;

/// Effect API types for capabilities and intents
pub mod effect_api;

/// Causal context for CRDT ordering
pub mod causal_context;

/// Commitment tree state machine and reduction
pub mod commitment_tree;

/// High-level Journal API
pub mod journal_api;

/// Commitment tree integration utilities
pub mod commitment_integration;

/// Fact model and journal operations
pub mod fact;

/// Deterministic state reduction
pub mod reduction;

/// Pure functions for formal verification
pub mod pure;

/// Extensible fact type infrastructure
pub mod extensibility;

/// Authority state derivation
pub mod authority_state;

// Re-exports
pub use error::{AuraError, Result};

/// Application effect handler
pub use effects::{JournalHandler, JournalHandlerFactory};

// Core type re-exports
pub use aura_core::time::OrderTime;
pub use aura_core::Hash32;

// Domain re-exports
pub use effect_api::{CapabilityId, CapabilityRef, Intent, IntentId, IntentStatus, Priority};

// Fact-based journal exports
pub use fact::{
    AckStorage, AttestedOp as FactAttestedOp, Fact, FactContent, FactOptions, GcResult,
    Journal as FactJournal, JournalNamespace, ProtocolRelationalFact, RelationalFact, SnapshotFact,
    TreeOpKind,
};
pub use reduction::{
    reduce_authority, reduce_context, ChannelEpochState, ReductionNamespaceError, RelationalState,
};

/// Primary Journal API
pub use journal_api::{AccountSummary, CommittedFact, Journal, JournalFact};

// CRDT implementation details (internal)
#[doc(hidden)]
pub use algebra::{AccountState, EpochLog, GuardianRegistry, IntentPool, MaxCounter, OpLog};

// Tree types from aura-core
pub use aura_core::tree::{
    commit_branch, commit_leaf, compute_root_commitment, policy_hash, AttestedOp, BranchNode,
    Epoch, LeafId, LeafNode, LeafRole, NodeIndex, NodeKind, Policy, TreeCommitment,
};

// Commitment tree state and reduction
pub use commitment_tree::{reduce, TreeState};

// Causal context
pub use aura_core::time::VectorClock;
pub use causal_context::{ActorId, CausalContext, OperationId, VectorClockExt};

// Types
pub use types::GuardianMetadata;

// Extensibility infrastructure
pub use extensibility::{
    decode_domain_fact, encode_domain_fact, parse_envelope, DomainFact, FactReducer, FactRegistry,
};
