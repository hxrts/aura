//! Aura Recovery Choreographies
//!
//! This crate provides choreographic protocols for guardian-based recovery
//! operations in the Aura threshold identity platform.
//!
//! # Architecture
//!
//! This crate implements recovery choreographies:
//! - `G_recovery` - Main guardian recovery choreography
//! - `key_recovery` - Device key recovery protocols
//! - `account_recovery` - Account access recovery
//! - `emergency_recovery` - Emergency freeze/unfreeze operations
//!
//! # Design Principles
//!
//! - Uses choreographic programming for distributed recovery coordination
//! - Integrates with guardian authentication and threshold signatures
//! - Provides clean separation to avoid namespace conflicts (E0428 errors)
//! - Works with capability-based access control and privacy budgets

#![warn(missing_docs)]
#![forbid(unsafe_code)]

/// Main guardian recovery choreography (G_recovery)
pub mod guardian_recovery;

/// Device key recovery protocols
pub mod key_recovery;

/// Account access recovery protocols  
pub mod account_recovery;

/// Emergency operations (freeze/unfreeze)
pub mod emergency_recovery;

/// G_recovery choreography implementation
pub mod choreography_impl;

/// Shared recovery data structures
pub mod types;

/// Errors for recovery operations
pub type RecoveryError = AuraError;
pub type RecoveryResult<T> = AuraResult<T>;

// Re-export core types
pub use aura_core::{AccountId, AuraError, AuraResult, Cap, DeviceId, Journal};

// Re-export verification types
pub use aura_authenticate::AuthenticationResult;
pub use aura_verify::session::{SessionScope, SessionTicket};
pub use aura_verify::{AuthenticationError, IdentityProof, VerifiedIdentity};

// Re-export auth choreography types
pub use aura_authenticate::guardian_auth::{
    GuardianAuthCoordinator, GuardianAuthRequest, GuardianAuthResponse, RecoveryContext,
    RecoveryOperationType,
};
pub type AuthError = AuthenticationError;
pub type AuthResult<T> = Result<T, AuthenticationError>;

// Re-export MPST types
pub use aura_mpst::{
    AuraRuntime, CapabilityGuard, ExecutionContext, JournalAnnotation, MpstError, MpstResult,
};

// Re-export WoT types
pub use aura_wot::{CapabilitySet, TreePolicy as TrustPolicy};

// Re-export recovery domain types
pub use types::{
    GuardianProfile as Guardian, GuardianSet, RecoveryDispute, RecoveryEvidence, RecoveryShare,
};

// Re-export guardian recovery types
pub use guardian_recovery::{
    GuardianRecoveryCoordinator, GuardianRecoveryResponse, RecoveryStatus,
    RecoveryPolicyConfig, RecoveryPolicyEnforcer, PolicyValidationResult,
    PolicyViolation, PolicyWarning,
};

// Re-export choreography implementations
pub use choreography_impl::{
    RecoveryChoreography, RecoveryMessage, RecoveryRole, RecoverySessionMetrics,
    RecoverySessionResult,
};

// Error re-exports removed - use aura_core::AuraError directly
