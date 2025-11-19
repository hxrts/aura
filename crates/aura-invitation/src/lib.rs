//! Aura Invitation Choreographies
//!
//! This crate provides choreographic protocols for invitation and acceptance
//! operations in the Aura threshold identity platform.
//!
//! # Architecture
//!
//! This crate implements invitation choreographies:
//! - `G_invitation` - Main invitation and acceptance choreography
//! - `guardian_invitation` - Guardian relationship establishment
//! - `device_invitation` - Device onboarding and acceptance
//! - `relationship_formation` - Trust relationship creation
//!
//! # Design Principles
//!
//! - Uses choreographic programming for distributed invitation coordination
//! - Integrates with Web of Trust (WoT) for relationship management
//! - Provides clean separation to avoid namespace conflicts (E0428 errors)
//! - Supports capability-based invitation validation and acceptance

#![allow(missing_docs)]
#![forbid(unsafe_code)]

/// Main invitation and acceptance choreography (G_invitation)
pub mod invitation_acceptance;

/// Guardian relationship invitation protocols
pub mod guardian_invitation;

/// Device onboarding invitation protocols
pub mod device_invitation;

/// Trust relationship formation
pub mod relationship_formation;

mod transport;

/// Error type for invitation operations
///
/// Type alias for `AuraError` used in invitation-related operations
pub type InvitationError = AuraError;

/// Result type for invitation operations
///
/// Type alias for `AuraResult<T>` used in invitation-related operations
pub type InvitationResult<T> = AuraResult<T>;

/// A complete relationship record between devices
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Relationship {
    /// Unique relationship identifier
    pub id: Vec<u8>,
    /// Devices participating in relationship
    pub parties: Vec<DeviceId>,
    /// Account context
    pub account_id: AccountId,
    /// Trust level
    pub trust_level: TrustLevel,
    /// Type of relationship
    pub relationship_type: RelationshipType,
    /// Additional metadata
    pub metadata: Vec<(String, String)>,
    /// Creation timestamp
    pub created_at: u64,
}

/// Type alias for guardian identifier
///
/// Compatibility alias for `GuardianId` used in invitation contexts
pub type Guardian = GuardianId;

/// Type alias for a set of guardians
///
/// Represents a collection of guardian identifiers
pub type GuardianSet = Vec<GuardianId>;

/// Type alias for authentication errors
///
/// Compatibility alias for `AuthenticationError` used in invitation flows
pub type AuthError = AuthenticationError;

/// Type alias for authentication results
///
/// Compatibility alias for authentication operation results
pub type AuthResult<T> = Result<T, AuthenticationError>;

// Re-export core types
pub use aura_core::{
    AccountId, AuraError, AuraResult, DeviceId, GuardianId, Journal, RelationshipId,
    RelationshipType, TrustLevel,
};

// Re-export WoT types (using Biscuit tokens instead of legacy capabilities)
pub use aura_wot::{
    AccountAuthority, BiscuitError, BiscuitTokenManager, TreePolicy as TrustPolicy,
};
pub use biscuit_auth::Biscuit as BiscuitToken;

// Re-export auth types
pub use aura_authenticate::{
    AuthenticationError, AuthenticationResult, IdentityProof, VerifiedIdentity,
};

// Re-export core effect types
pub use aura_core::effects::{
    ConsoleEffects, CryptoEffects, JournalEffects, NetworkEffects, TimeEffects,
};

// MPST types removed - using stateless effect system instead

// Error re-exports removed - use aura_core::AuraError directly
