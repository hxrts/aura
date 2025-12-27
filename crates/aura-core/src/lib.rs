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
//! - YES Effect trait definitions (interfaces, no implementations)
//! - YES Domain types: `AuthorityId`, `ContextId`, `SessionId`, `FlowBudget`
//! - YES Semantic traits: `JoinSemilattice`, `MeetSemiLattice`, `CvState`, `MvState`
//! - YES Cryptographic utilities: key derivation, FROST types, merkle trees
//! - YES Error types: `AuraError`, error codes, and guard metadata
//! - YES Configuration system with validation
//! - YES Extension traits providing convenience methods (e.g., `LeakageChoreographyExt`)
//! - NO implementations (those go in aura-effects or domain crates)
//! - NO application logic (that goes in feature crates)
//! - NO handler composition (that's aura-composition)
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
//! - `PhysicalTimeEffects`: Physical clock access, timestamps\n//! - `LogicalClockEffects`: Causal ordering, vector clocks\n//! - `OrderClockEffects`: Privacy-preserving deterministic ordering
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

/// Domain-specific logic types (consensus, journal, content addressing)
pub mod domain;
/// Core domain types (identifiers, authority, scope, flow, epochs, sessions, relationships)
pub mod types;
/// Utility modules (serialization, conversions, context derivation, test utilities)
pub mod util;

/// Operation-scoped execution context for effectful calls
pub mod context;

/// Cryptographic primitives and utilities (hash, signing, FROST, merkle trees)
pub mod crypto;
/// Pure effect interfaces (no implementations)
pub mod effects;
/// Unified error handling
pub mod errors;
/// Core message envelopes and versioning
pub mod messages;
/// Query trait and Datalog types for unified query execution
pub mod query;
/// Reactive primitives for TUI and database subscriptions
pub mod reactive;
/// Relational domain types for cross-authority coordination
pub mod relational;
/// Core algebraic types and semilattice laws
pub mod semilattice;
/// Unified threshold signing types
pub mod threshold;
/// Time semantics (Logical/Order/Physical/Range)
pub mod time;
/// Tree operation types
pub mod tree;

// === Backwards-Compatible Module Re-exports ===
// These re-exports maintain compatibility with code using aura_core::identifiers::X pattern
// The canonical location is now aura_core::types::*, but we re-export modules
// to avoid breaking existing consumers.
pub use crypto::hash;
pub use domain::journal;
pub use types::authority;
pub use types::epochs;
pub use types::flow;
pub use types::identifiers;
pub use types::relationships;
pub use types::scope;
pub use util::context as context_derivation;

// === Public API Re-exports ===

pub use time::TimeDomain;

// Core algebraic types
#[doc = "stable: Core journal types with semver guarantees"]
pub use domain::journal::{AuthLevel, Cap, Fact, FactValue, Journal};
#[doc = "internal: Semilattice traits are implementation details, use Journal API instead"]
pub use semilattice::{
    Bottom, CmState, CvState, DeltaState, JoinSemilattice, MeetSemiLattice, MvState, Top,
};

// Identifiers and contexts
pub use context::{ContextSnapshot, EffectContext};
#[doc = "stable: Core identifier types with semver guarantees"]
pub use types::identifiers::{
    AccountId, AuthorityId, ChannelId, ContextId, DataId, DeviceId, DkdContextId, EventId,
    EventNonce, GroupId, GuardianId, IndividualId, IndividualIdExt, MemberId, MessageContext,
    OperationId, RelayId, SessionId,
};
#[doc = "unstable: Context derivation system is under active development"]
pub use util::context::{
    ContextDerivationService, ContextParams, DkdContextDerivation, GroupConfiguration,
    GroupContextDerivation, RelayContextDerivation,
};

// Authority abstraction (new architecture)
#[doc = "unstable: Authority model is under active development - migration from AccountId ongoing"]
pub use types::authority::{Authority, AuthorityRef, AuthorityState, TreeStateSummary};
pub use types::facts::{decode_domain_fact, encode_domain_fact, FactEncoding, FactEnvelope};

// Consensus types
#[doc = "stable: Core consensus types with semver guarantees"]
pub use domain::consensus::{Prestate, PrestateBuilder};

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
pub use util::serialization::{
    from_slice, hash_canonical, to_vec, SemanticVersion as SerVersion, SerializationError,
    VersionedMessage,
};

// Errors
#[doc = "stable: Error types with semver guarantees"]
pub use errors::{AuraError, ProtocolErrorCode, Result as AuraResult};

// Effect interfaces
pub use effects::{
    AntiEntropyEffects,
    AuthorizationEffects,
    // Reliability types (unified retry implementation)
    BackoffStrategy,
    // Supertraits for common effect combinations
    ChoreographyEffects,
    ConsoleEffects,
    CrdtEffects,
    CryptoEffects,
    // Core effect system types
    EffectType,
    ExecutionMode,
    // Indexed journal types (B-tree, Bloom, Merkle)
    FactId,
    FlowBudgetEffects,
    FlowHint,
    IndexStats,
    IndexedFact,
    IndexedJournalEffects,
    JournalEffects,
    LogicalClockEffects,
    MinimalEffects,
    OrderClockEffects,
    PhysicalTimeEffects,
    // Query effects for Datalog execution
    QueryEffects,
    QueryError,
    QuerySubscription,
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
    // Unified threshold signing effects
    ThresholdSigningEffects,
    TimeEffects,
    TraceEffects,
    TreeEffects,
    WakeCondition,
};

// Simulation/testing effect interfaces (feature-gated)
#[cfg(feature = "simulation")]
pub use effects::{ChaosEffects, TestingEffects};

// Query trait and Datalog types
#[doc = "unstable: Query interface is under active development"]
pub use query::{
    DatalogBindings, DatalogFact, DatalogProgram, DatalogRow, DatalogRule, DatalogValue,
    FactPredicate, Query, QueryCapability, QueryParseError,
};

// Cryptographic utilities
#[doc = "stable: Core cryptographic utilities with semver guarantees"]
pub use crypto::{
    build_commitment_tree, build_merkle_root, ed25519_verify, verify_merkle_proof,
    Ed25519Signature, Ed25519SigningKey, Ed25519VerifyingKey, HpkeKeyPair, HpkePrivateKey,
    HpkePublicKey, IdentityKeyContext, KeyDerivationSpec, MerkleProof, PermissionKeyContext,
    SimpleMerkleProof,
};

// FROST threshold cryptography module (primitives live here; aura-frost deprecated)
#[doc = "unstable: FROST implementation may change significantly"]
pub use crypto::frost;

// Time and content
#[doc = "stable: Content addressing types with semver guarantees"]
pub use domain::content::{ChunkId, ContentId, ContentSize, Hash32};
#[doc = "stable: Time semantics with semver guarantees"]
pub use time::{
    AttestationValidity, LogicalTime, OrderTime, OrderingPolicy, PhysicalTime, RangeTime,
    TimeConfidence, TimeMetadata, TimeOrdering, TimeProof, TimeStamp,
};

// Protocol and session types (compat shim; slated for app-layer relocation)
#[doc = "stable: Core relational types for cross-authority coordination with semver guarantees"]
pub use relational::*;
#[doc = "stable: Tree types are foundational Layer 1 abstractions required by effect traits and FROST primitives"]
pub use tree::{
    commit_branch, commit_leaf, compute_root_commitment, policy_hash, AttestedOp, BranchNode,
    Epoch, LeafId, LeafNode, LeafRole, NodeIndex, NodeKind, Policy, TreeCommitment, TreeOp,
    TreeOpKind,
};
#[doc = "stable: Epoch counters and participant identifiers"]
pub use types::epochs::*;
#[doc = "unstable: FlowBudget API is experimental and may change"]
pub use types::flow::{FlowBudget, Receipt};
#[doc = "unstable: Relationship types are under active development"]
pub use types::relationships::*;
#[doc = "stable: Resource scope types for authorization with semver guarantees"]
pub use types::scope::{AuthorityOp, ContextOp, ResourceScope};

// Threshold signing types
#[doc = "unstable: Unified threshold signing types are under active development"]
pub use threshold::{
    ApprovalContext, ParticipantEndpoint, ParticipantIdentity, SignableOperation, SigningContext,
    SigningParticipant, ThresholdSignature,
};

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
