//! # Aura Web of Trust - Layer 2: Specification (Domain Crate)
//!
//! **Purpose**: Define trust and authorization semantics with capability refinement.
//!
//! Authority-based authorization system using Biscuit tokens for cryptographically
//! verifiable capability delegation. This crate implements the Web of Trust layer
//! from Aura's architectural model, providing Biscuit-based authorization with
//! authority-centric resource scopes.
//!
//! # Architecture Constraints
//!
//! **Layer 2 depends only on aura-core** (foundation).
//! - ✅ Capability refinement logic (meet-semilattice `⊓`)
//! - ✅ Biscuit token helpers and semantics (no cryptographic operations)
//! - ✅ Authorization domain types and policies
//! - ❌ NO cryptographic signing (that's aura-effects via CryptoEffects)
//! - ❌ NO handler composition (that's aura-composition)
//! - ❌ NO multi-party protocol logic (that's aura-protocol)
//!
//! # Authorization System
//!
//! The crate provides:
//! - Biscuit token model and verification semantics
//! - Authority-centric resource scopes (AuthorityOp, ContextOp)
//! - Capability refinement with attenuation rules
//! - Policy evaluation patterns (datalog-based)
//!
//! ## Usage
//!
//! ```rust
//! use aura_wot::{ResourceScope, AuthorityOp};
//! use aura_core::{AuthorityId};
//!
//! // Authority-based resource authorization
//! let resource = ResourceScope::Authority {
//!     authority_id: AuthorityId::default(),
//!     operation: AuthorityOp::UpdateTree,
//! };
//! // Token verification handles cryptographic delegation chains
//! ```

pub mod errors;

// Application effects implementation (Layer 2 pattern)
pub mod effects;
pub mod flow_budget;

// Legacy capability system removed - Phase 4 of authorization unification complete
// Use Biscuit tokens via BiscuitTokenManager instead

// Biscuit-based authorization (new implementation)
// biscuit module is now consolidated into biscuit_authorization.rs
pub mod biscuit_authorization;
pub mod biscuit_token;
pub mod facts;
pub mod resource_scope; // Authority-based resource scopes
pub mod storage_authorization; // Storage authorization logic (moved from aura-store)

pub use errors::{AuraError, AuraResult, WotError, WotResult};

// Application effect handler re-export
pub use effects::WotAuthorizationHandler;

// Legacy capability types removed - use Biscuit tokens instead
// Legacy tree policy types removed - use authority-based ResourceScope instead

// Re-export semilattice traits for convenience
pub use aura_core::semilattice::{MeetSemiLattice, Top};

// Re-export Biscuit types
pub use biscuit_auth::{Biscuit, KeyPair, PublicKey};
#[allow(deprecated)]
pub use biscuit_token::AccountAuthority; // Deprecated alias for TokenAuthority
pub use biscuit_token::{BiscuitError, BiscuitTokenManager, SerializableBiscuit, TokenAuthority};
pub use flow_budget::FlowBudgetHandler;

// Re-export fact types for journal integration
pub use facts::{WotFact, WotFactDelta, WotFactReducer, WOT_FACT_TYPE_ID};

// Re-export authority-based resource scopes from core
// These replace the previous Biscuit-specific resource scopes (AdminOperation, JournalOp, etc.)
// which were internal implementation details that have been migrated to the authority model
pub use aura_core::scope::{AuthorityOp, ContextOp, ResourceScope};

// Re-export Biscuit authorization types (now consolidated in biscuit_authorization.rs)
pub use biscuit_authorization::{AuthorizationResult, BiscuitAuthorizationBridge};

// Re-export storage authorization types (moved from aura-store)
pub use storage_authorization::{
    check_biscuit_access, evaluate_biscuit_access, AccessDecision, AuthorizedStorageHandler,
    BiscuitAccessRequest, BiscuitStorageError, BiscuitStorageEvaluator, PermissionMappings,
    StoragePermission, StorageResource,
};

/// Type alias for capability meet operation results
pub type CapResult<T> = Result<T, WotError>;
