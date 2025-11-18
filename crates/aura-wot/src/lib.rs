//! # Aura Web of Trust
//!
//! Capability-based authorization implementing meet-semilattice laws for
//! monotonic capability restriction and delegation chains.
//!
//! This crate implements the Web of Trust layer from Aura's architectural
//! model, providing concrete realizations of the theoretical foundations
//! described in docs/001_theoretical_model.md.
//!
//! ## Theoretical Foundations
//!
//! This implementation directly corresponds to the mathematical model in:
//! - §2.1 Foundation Objects: Capabilities as meet-semilattice elements (C, ⊓, ⊤)
//! - §2.4 Semantic Laws: Meet operations that are associative, commutative, and idempotent
//! - §5.2 Web-of-Trust Model: Delegation composition via meet operations
//!
//! The crate provides:
//! - Meet-semilattice capability objects that can only shrink (⊓)
//! - Capability delegation chains with proper attenuation
//! - Policy enforcement via capability intersection
//! - Formal verification of semilattice laws
//!
//! ## Core Concepts
//!
//! Capabilities follow meet-semilattice laws from §1.4 Algebraic Laws:
//! - **Associative**: (a ⊓ b) ⊓ c = a ⊓ (b ⊓ c)
//! - **Commutative**: a ⊓ b = b ⊓ a
//! - **Idempotent**: a ⊓ a = a
//! - **Monotonic**: a ⊓ b ⪯ a and a ⊓ b ⪯ b (Monotonic Restriction)
//!
//! ## Usage
//!
//! ```rust
//! use aura_wot::{Capability, CapabilitySet, MeetSemiLattice};
//!
//! // Capabilities only shrink via meet operation
//! let base_policy = CapabilitySet::from_permissions(&["read:docs", "write:data"]);
//! let delegation = CapabilitySet::from_permissions(&["read:docs"]);

//!
//! // Effective capabilities = intersection (can only get smaller)
//! let effective = base_policy.meet(&delegation);
//! assert!(effective.permits("read:docs"));
//! assert!(!effective.permits("write:data")); // Lost via intersection
//! ```

pub mod errors;

// Legacy capability system (for backward compatibility with tests)
pub mod capability;
pub mod tree_policy;

// Biscuit-based authorization (new implementation)
pub mod biscuit_resources;
pub mod biscuit_token;

pub use errors::{AuraError, AuraResult, WotError, WotResult};

// Export legacy capability types
pub use capability::{
    evaluate_capabilities, Capability, CapabilitySet, DelegationChain, DelegationLink,
    EvaluationContext, LocalChecks, Policy,
};

// Export tree policy types
pub use tree_policy::Policy as TreePolicy;

// Re-export semilattice traits for convenience
pub use aura_core::semilattice::{MeetSemiLattice, Top};

// Re-export Biscuit types
pub use biscuit_auth::{Biscuit, KeyPair, PublicKey};
pub use biscuit_resources::{
    AdminOperation, JournalOp, RecoveryType, ResourceScope, StorageCategory,
};
pub use biscuit_token::{AccountAuthority, BiscuitError, BiscuitTokenManager, SerializableBiscuit};

/// Type alias for capability meet operation results
pub type CapResult<T> = Result<T, WotError>;
