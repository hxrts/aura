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

#![warn(missing_docs)]
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

/// Errors for invitation operations
pub type InvitationError = AuraError;
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

// Type aliases for compatibility
pub type Guardian = GuardianId;
pub type GuardianSet = Vec<GuardianId>;
pub type AuthError = AuthenticationError;
pub type AuthResult<T> = Result<T, AuthenticationError>;

// Re-export core types
pub use aura_core::{
    AccountId, AuraError, AuraResult, Cap, DeviceId, GuardianId, Journal, RelationshipId,
    RelationshipType, TrustLevel,
};

// Re-export WoT types
pub use aura_wot::{CapabilitySet, TreePolicy as TrustPolicy};

// Re-export auth types
pub use aura_authenticate::{
    AuthenticationError, AuthenticationResult, IdentityProof, VerifiedIdentity,
};

// Re-export core effect types
pub use aura_core::effects::{
    ConsoleEffects, CryptoEffects, JournalEffects, NetworkEffects, TimeEffects,
};

// Re-export MPST types
pub use aura_mpst::{
    AuraRuntime, CapabilityGuard, ExecutionContext, JournalAnnotation, MpstError, MpstResult,
};

// Re-export effect system
pub use aura_protocol::AuraEffectSystem;

// Error re-exports removed - use aura_core::AuraError directly
