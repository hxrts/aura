//! Aura Core - Whole System Model Foundation
//!
//! This crate provides the foundational algebraic types and effect interfaces
//! that define the whole system model for Aura. It contains only pure mathematical
//! abstractions with no implementation details or application logic.
//!
//! # Architecture Layers
//!
//! ## Core Algebraic Types
//! - `Cap`: Meet-semilattice (⊓) for capabilities/authority
//! - `Fact`: Join-semilattice (⊔) for knowledge accumulation
//! - `Journal { facts: Fact, caps: Cap }`: CRDT pullback structure
//! - Contexts (`RID`, `GID`, `DKD`): Privacy partitions
//!
//! ## Effect Interfaces (Pure Signatures)
//! - `JournalEffects`: `merge_facts`, `refine_caps`
//! - `CryptoEffects`: `sign_threshold`, `aead_seal`, `ratchet_step`
//! - `TransportEffects`: `send`, `recv`, `connect`
//! - `TimeEffects`, `RandEffects`: Simulation/testing support
//!
//! ## Semilattice CRDT Laws
//! - Monotonic growth: `Fₜ₊₁ = Fₜ ⊔ δ` → `Fₜ ≤ Fₜ₊₁`
//! - Monotonic restriction: `Cₜ₊₁ = Cₜ ⊓ γ` → `Cₜ₊₁ ≤ Cₜ`
//! - Compositional confluence: `merge(δ₁) ; merge(δ₂) ≡ merge(δ₁ ⊔ δ₂)`
//!
//! ## Privacy & Security Contracts
//! - Context isolation: `κ₁ ≠ κ₂` prevents cross-context flow
//! - Unlinkability: `τ[c1↔c2] ≈_ext τ` (computational indistinguishability)
//! - Leakage bounds: `L(τ, observer) ≤ Budget(observer)`

#![allow(missing_docs)]
#![forbid(unsafe_code)]

// === Core Modules ===

/// Core algebraic types and semilattice laws
pub mod semilattice;

/// Journal CRDT with facts (⊔) and capabilities (⊓)
pub mod journal;

/// Device, account, and context identifiers
pub mod identifiers;

/// Context derivation for privacy partitions
pub mod context_derivation;

/// Core message envelopes and versioning
pub mod messages;

/// Pure effect interfaces (no implementations)
pub mod effects;

/// Unified error handling
pub mod errors;

/// DAG-CBOR serialization (canonical format)
pub mod serialization;

/// Time utilities for deterministic simulation
pub mod time;

/// Content addressing and IPLD compatibility
pub mod content;

/// Protocol type definitions
pub mod protocols;

/// Relationship and web-of-trust types
pub mod relationships;

/// Session epochs and participant management
pub mod session_epochs;

/// Tree operation types
pub mod tree;

/// FlowBudget primitives
pub mod flow;

/// Type conversion utilities
pub mod conversions;

/// Causal context and vector clocks for CRDT ordering
pub mod causal_context;

/// Pure synchronous hash trait for content addressing
pub mod hash;

/// Cryptographic domain types and utilities
pub mod crypto;

/// Maintenance operation types
pub mod maintenance;

/// Internal test utilities (Layer 1 - does not use aura-testkit to avoid circular dependencies)
#[cfg(test)]
pub mod test_utils;

// === Public API Re-exports ===

// Core algebraic types
pub use journal::{AuthLevel, Cap, Fact, FactValue, Journal};
pub use semilattice::{
    Bottom, CmState, CvState, DeltaState, JoinSemilattice, MeetSemiLattice, MvState, Top,
};

// Identifiers and contexts
pub use context_derivation::{
    ContextDerivationService, ContextParams, DkdContextDerivation, GroupConfiguration,
    GroupContextDerivation, RelayContextDerivation,
};
pub use identifiers::*;

// Messages and versioning
pub use messages::{
    AuthStrength, AuthTag, MessageValidation, MessageValidator, Msg, SemanticVersion, TypedMessage,
};
pub use serialization::{
    from_slice, hash_canonical, to_vec, SemanticVersion as SerVersion, SerializationError,
    VersionedMessage,
};

// Errors
pub use errors::{AuraError, Result as AuraResult};

// Effect interfaces
pub use effects::{
    AuthorizationEffects, ChaosEffects, ConsoleEffects, CryptoEffects, JournalEffects, RandomEffects,
    ReliabilityEffects, TestingEffects, TimeEffects,
    // Supertraits for common effect combinations
    ChoreographyEffects, TreeEffects, SigningEffects, CrdtEffects, AntiEntropyEffects, 
    MinimalEffects, SnapshotEffects,
};

// Cryptographic utilities
pub use crypto::{
    derive_encryption_key, derive_key_material, ed25519_verify, generate_uuid,
    build_commitment_tree, build_merkle_root, verify_merkle_proof,
    Ed25519Signature, Ed25519SigningKey, Ed25519VerifyingKey,
    HpkeKeyPair, HpkePrivateKey, HpkePublicKey, IdentityKeyContext, KeyDerivationSpec,
    MerkleProof, PermissionKeyContext, SimpleMerkleProof,
};

// FROST threshold cryptography module (re-export for aura-frost compatibility)
pub use crypto::frost;

// Time and content
pub use content::{ChunkId, ContentId, ContentSize, Hash32};
pub use time::{
    current_system_time, current_unix_timestamp, current_unix_timestamp_millis, LamportTimestamp,
};

// Protocol and session types (temporary - will move to app layer)
pub use flow::{FlowBudget, Receipt};
pub use protocols::*;
pub use relationships::*;
pub use session_epochs::*;
pub use tree::{
    commit_branch, commit_leaf, compute_root_commitment, policy_hash, AttestedOp, BranchNode,
    Epoch, LeafId, LeafNode, LeafRole, NodeIndex, NodeKind, Policy, TreeCommitment, TreeOp,
    TreeOpKind,
};

// Utilities
pub use causal_context::{CausalContext, OperationId, VectorClock};

// Maintenance events
pub use maintenance::{AdminReplaced, MaintenanceEvent};

/// Standard result type for core operations
pub type Result<T> = std::result::Result<T, AuraError>;

/// Type error for identifier and conversion operations
#[derive(thiserror::Error, Debug, Clone)]
pub enum TypeError {
    /// Invalid identifier format error
    #[error("Invalid identifier format: {0}")]
    InvalidIdentifier(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Parse error
    #[error("Parse error: {0}")]
    ParseError(String),
}
