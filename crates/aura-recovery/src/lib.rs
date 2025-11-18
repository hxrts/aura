//! Aura Guardian Recovery Choreographies
//!
//! This crate provides three essential choreographic protocols for guardian-based
//! threshold identity management in Aura.
//!
//! # Core Choreographies
//!
//! 1. **Guardian Setup** - Initial establishment of guardian relationships
//! 2. **Guardian Membership** - Adding/removing guardians from the set
//! 3. **Guardian Key Recovery** - Emergency key recovery with guardian approval
//!
//! # Design Principles
//!
//! - Simple, focused choreographies for specific use cases
//! - Emergency-only recovery (no priority levels)
//! - Clean integration with threshold signatures and authentication

#![allow(missing_docs)]
#![forbid(unsafe_code)]

/// Guardian setup choreography for initial relationship establishment
pub mod guardian_setup;

/// Guardian membership change choreography for adding/removing guardians
pub mod guardian_membership;

/// Guardian key recovery choreography for emergency key recovery
pub mod guardian_key_recovery;

/// Shared types for guardian operations
pub mod types;

// Core error types
pub use aura_core::{AuraError, AuraResult};

/// Recovery-specific error type
pub type RecoveryError = AuraError;

/// Recovery-specific result type
pub type RecoveryResult<T> = AuraResult<T>;

// Re-export essential types
pub use types::{GuardianProfile, GuardianSet, RecoveryRequest, RecoveryResponse};

// Re-export auth types
pub use aura_authenticate::guardian_auth::{
    GuardianAuthCoordinator, GuardianAuthRequest, GuardianAuthResponse, RecoveryContext,
    RecoveryOperationType,
};

// Re-export choreography coordinators
pub use guardian_key_recovery::GuardianKeyRecoveryCoordinator;
pub use guardian_membership::GuardianMembershipCoordinator;
pub use guardian_setup::GuardianSetupCoordinator;

// Re-export Biscuit types for convenience
pub use aura_protocol::guards::BiscuitGuardEvaluator;
pub use aura_wot::{BiscuitTokenManager, ResourceScope};

// Re-export membership change types
pub use guardian_membership::{MembershipChange, MembershipChangeRequest};
