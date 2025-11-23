//! # Aura Core - Layer 1: Foundation
//!
//! **Purpose**: Single source of truth for all domain concepts and interfaces.
//!
//! This crate provides the foundational algebraic types, effect trait interfaces,
//! and domain types that define the whole system model for Aura. It contains only
//! pure mathematical abstractions with no implementation details or application logic.
//!
//! # Architecture Constraints
//!
//! **Layer 1 has ZERO dependencies on other Aura crates** (foundation).
//! - ✅ Effect trait definitions (interfaces, no implementations)
//! - ✅ Domain types: `AuthorityId`, `ContextId`, `SessionId`, `FlowBudget`
//! - ✅ Semantic traits: `JoinSemilattice`, `MeetSemilattice`, `CvState`, `MvState`
//! - ✅ Cryptographic utilities: key derivation, FROST types, merkle trees
//! - ✅ Error types: `AuraError`, error codes, and guard metadata
//! - ✅ Configuration system with validation
//! - ✅ Extension traits providing convenience methods (e.g., `LeakageChoreographyExt`)
//! - ❌ NO implementations (those go in aura-effects or domain crates)
//! - ❌ NO application logic (that goes in feature crates)
//! - ❌ NO handler composition (that's aura-composition)
//!
//! # Core Abstractions
//!
//! ## Algebraic Types
//! - `Cap`: Meet-semilattice (⊓) for capabilities/authority
//! - `Fact`: Join-semilattice (⊔) for knowledge accumulation
//! - `Journal { facts: Fact, caps: Cap }`: CRDT pullback structure
//! - Contexts (`ContextId`): Privacy partitions
//!
//! ## Effect Trait Categories
//!
//! **Infrastructure Effects** (require handlers in aura-effects):
//! - `CryptoEffects`: Signing, hashing, key derivation
//! - `NetworkEffects`: TCP, message sending/receiving
//! - `StorageEffects`: File I/O, chunk operations
//! - `TimeEffects`: Current time, delays
//! - `RandomEffects`: Cryptographic randomness
//!
//! **Application Effects** (implemented in domain crates with infrastructure effects):
//! - `JournalEffects`: Fact-based journal operations
//! - `AuthorizationEffects`: Biscuit token evaluation
//! - `FlowBudgetEffects`: Privacy budget management
//! - `LeakageEffects`: Metadata leakage tracking
//!
//! ## Semilattice CRDT Laws
//! - Monotonic growth: `Fₜ₊₁ = Fₜ ⊔ δ` → `Fₜ ≤ Fₜ₊₁`
//! - Monotonic restriction: `Cₜ₊₁ = Cₜ ⊓ γ` → `Cₜ₊₁ ≤ Cₜ`
//! - Compositional confluence: `merge(δ₁) ; merge(δ₂) ≡ merge(δ₁ ⊔ δ₂)`
//!
//! ## Privacy & Security Contracts
//! - Context isolation: Different `ContextId` prevents cross-context flow
//! - Unlinkability: Computational indistinguishability across contexts
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

/// API stability annotations
pub mod stability;

/// Relationship and web-of-trust types
pub mod relationships;

/// Session epochs and participant management
pub mod session_epochs;

/// Tree operation types
pub mod tree;

/// FlowBudget primitives
pub mod flow;

/// Authority abstraction (new architecture)
pub mod authority;

/// Core consensus types and prestate management
pub mod consensus;

/// Type conversion utilities (internal helpers)
#[doc(hidden)]
pub mod conversions;

/// Pure synchronous hash trait for content addressing
pub mod hash;

/// Cryptographic domain types and utilities
pub mod crypto;

/// Maintenance operation types
pub mod maintenance;

/// Relational domain types for cross-authority coordination
pub mod relational;

/// Internal test utilities (Layer 1 - does not use aura-testkit to avoid circular dependencies)
#[doc(hidden)]
pub mod test_utils;

// === Public API Re-exports ===

// Core algebraic types
#[doc = "stable: Core journal types with semver guarantees"]
pub use journal::{AuthLevel, Cap, Fact, FactValue, Journal};
#[doc = "internal: Semilattice traits are implementation details, use Journal API instead"]
pub use semilattice::{
    Bottom, CmState, CvState, DeltaState, JoinSemilattice, MeetSemiLattice, MvState, Top,
};

// Identifiers and contexts
#[doc = "unstable: Context derivation system is under active development"]
pub use context_derivation::{
    ContextDerivationService, ContextParams, DkdContextDerivation, GroupConfiguration,
    GroupContextDerivation, RelayContextDerivation,
};
#[doc = "stable: Core identifier types with semver guarantees"]
pub use identifiers::{
    AccountId, AuthorityId, ChannelId, ContextId, DataId, DeviceId, DkdContextId, EventId,
    EventNonce, GroupId, GuardianId, IndividualId, IndividualIdExt, MemberId, MessageContext,
    OperationId, RelayId, SessionId,
};

// DeviceId is now internal to aura-journal/src/commitment_tree/ only
// For migration: use AuthorityId for external APIs, DeviceId only within commitment tree

// Authority abstraction (new architecture)
#[doc = "unstable: Authority model is under active development - migration from AccountId ongoing"]
pub use authority::{Authority, AuthorityRef, AuthorityState, TreeState};

// Consensus types
#[doc = "stable: Core consensus types with semver guarantees"]
pub use consensus::{Prestate, PrestateBuilder};

// Messages and versioning
#[doc = "stable: Core message types with semver guarantees"]
pub use messages::{
    // Message error helper functions
    cid_mismatch_error,
    invalid_envelope_size_error,
    invalid_message_format_error,
    message_deserialization_error,
    message_serialization_error,
    message_too_large_error,
    unsupported_version_error,
    AuthStrength,
    AuthTag,
    MessageError,
    MessageResult,
    MessageValidation,
    MessageValidator,
    Msg,
    SemanticVersion,
    TypedMessage,
    WireEnvelope,
    WIRE_FORMAT_VERSION,
};
#[doc = "stable: Canonical serialization with semver guarantees"]
pub use serialization::{
    from_slice, hash_canonical, to_vec, SemanticVersion as SerVersion, SerializationError,
    VersionedMessage,
};

// Errors
#[doc = "stable: Error types with semver guarantees"]
pub use errors::{AuraError, Result as AuraResult};

// Effect interfaces
pub use effects::{
    AntiEntropyEffects,
    AuthorizationEffects,
    // Reliability types (unified retry implementation)
    BackoffStrategy,
    ChaosEffects,
    // Supertraits for common effect combinations
    ChoreographyEffects,
    ConsoleEffects,
    CrdtEffects,
    CryptoEffects,
    // Core effect system types
    EffectType,
    ExecutionMode,
    FlowBudgetEffects,
    FlowHint,
    JournalEffects,
    MinimalEffects,
    RandomEffects,
    RateLimit,
    // Rate limiting types (unified rate limiting implementation)
    RateLimitConfig,
    RateLimitResult,
    RateLimiter,
    RateLimiterStatistics,
    ReliabilityEffects,
    ReliabilityError,
    RetryContext,
    RetryPolicy,
    RetryResult,
    SigningEffects,
    SnapshotEffects,
    TestingEffects,
    TimeEffects,
    TreeEffects,
};

// Cryptographic utilities
#[doc = "stable: Core cryptographic utilities with semver guarantees"]
pub use crypto::{
    build_commitment_tree, build_merkle_root, ed25519_verify, generate_uuid, verify_merkle_proof,
    Ed25519Signature, Ed25519SigningKey, Ed25519VerifyingKey, HpkeKeyPair, HpkePrivateKey,
    HpkePublicKey, IdentityKeyContext, KeyDerivationSpec, MerkleProof, PermissionKeyContext,
    SimpleMerkleProof,
};

// FROST threshold cryptography module (re-export for aura-frost compatibility)
#[doc = "unstable: FROST implementation may change significantly"]
pub use crypto::frost;

// Time and content
#[doc = "stable: Content addressing types with semver guarantees"]
pub use content::{ChunkId, ContentId, ContentSize, Hash32};
#[doc = "stable: Time utilities with semver guarantees"]
pub use time::{
    current_system_time, current_unix_timestamp, current_unix_timestamp_millis, LamportTimestamp,
};

// Protocol and session types (temporary - will move to app layer)
#[doc = "unstable: FlowBudget API is experimental and may change"]
pub use flow::{FlowBudget, Receipt};
#[doc = "internal: Protocol types are moving to higher layers"]
#[deprecated(
    note = "Protocol/session types now live in higher layers; prefer importing from the owning crate instead of aura_core::protocols"
)]
pub use protocols::*;
#[doc = "stable: Core relational types for cross-authority coordination with semver guarantees"]
pub use relational::*;
#[doc = "unstable: Relationship types are under active development"]
pub use relationships::*;
#[doc = "internal: Session epoch management is moving to aura-agent"]
pub use session_epochs::*;
#[deprecated(
    note = "Tree types moved to aura-journal::commitment_tree. Use `aura_journal::{AttestedOp, TreeOp, etc}` instead"
)]
pub use tree::{
    commit_branch, commit_leaf, compute_root_commitment, policy_hash, AttestedOp, BranchNode,
    Epoch, LeafId, LeafNode, LeafRole, NodeIndex, NodeKind, Policy, TreeCommitment, TreeOp,
    TreeOpKind,
};

// Utilities
// Note: CausalContext, OperationId, VectorClock moved to aura-journal

// Maintenance events
#[deprecated(
    note = "Maintenance types moved to aura-agent::maintenance. Use `aura_agent::{AdminReplaced, MaintenanceEvent}` instead"
)]
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
