//! # Aura Web of Trust
//!
//! Authority-based authorization system using Biscuit tokens for
//! cryptographically verifiable capability delegation.
//!
//! This crate implements the Web of Trust layer from Aura's architectural
//! model, providing Biscuit-based authorization with authority-centric
//! resource scopes described in docs/002_theoretical_model.md.
//!
//! ## Authorization System
//!
//! The crate provides:
//! - Biscuit token management with cryptographic verification
//! - Authority-centric resource scopes (AuthorityOp, ContextOp)
//! - Token delegation with built-in attenuation
//! - Datalog-based policy enforcement
//!
//! ## Usage
//!
//! ```rust
//! use aura_wot::{ResourceScope, AuthorityOp};
//! use aura_core::{AuthorityId};
//!
//! // Authority-based resource authorization  
//! let resource = ResourceScope::Authority {
//!     authority_id: AuthorityId::new(),
//!     operation: AuthorityOp::UpdateTree,
//! };
//! // Token verification handles cryptographic delegation chains
//! ```

pub mod errors;

// Legacy capability system removed - Phase 4 of authorization unification complete
// Use Biscuit tokens via BiscuitTokenManager instead

// Biscuit-based authorization (new implementation)
pub mod biscuit_resources;
pub mod biscuit_token;
pub mod resource_scope; // Authority-based resource scopes

pub use errors::{AuraError, AuraResult, WotError, WotResult};

// Legacy capability types removed - use Biscuit tokens instead
// Legacy tree policy types removed - use authority-based ResourceScope instead

// Re-export semilattice traits for convenience
pub use aura_core::semilattice::{MeetSemiLattice, Top};

// Re-export Biscuit types
pub use biscuit_auth::{Biscuit, KeyPair, PublicKey};
pub use biscuit_resources::{
    AdminOperation, JournalOp, RecoveryType, ResourceScope as LegacyResourceScope, StorageCategory,
};
pub use biscuit_token::{AccountAuthority, BiscuitError, BiscuitTokenManager, SerializableBiscuit};

// Re-export authority-based resource scopes
pub use resource_scope::{AuthorityOp, ContextOp, ResourceScope};

/// Type alias for capability meet operation results
pub type CapResult<T> = Result<T, WotError>;
