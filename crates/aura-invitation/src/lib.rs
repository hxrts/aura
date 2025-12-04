//! # Aura Invitation - Layer 5: Feature/Protocol Implementation
//!
//! This crate implements choreographic protocols for invitation and acceptance
//! of new devices, guardians, and relationships in the Aura threshold identity platform.
//!
//! ## Purpose
//!
//! Layer 5 feature crate providing end-to-end protocol implementations for:
//! - Device invitation and acceptance workflows
//! - Guardian relationship establishment
//! - Relationship formation between authorities
//! - Capability-based invitation validation
//!
//! ## Architecture Constraints
//!
//! This crate depends on:
//! - **Layer 1** (aura-core): Core types, effects, errors
//! - **Layer 2** (aura-journal, aura-wot, aura-verify): Domain semantics
//! - **Layer 3** (aura-effects): Effect handler implementations
//! - **Layer 4** (aura-protocol): Orchestration and guard chain
//! - **Layer 4** (aura-mpst): Session type coordination
//!
//! ## What Belongs Here
//!
//! - Complete invitation protocol implementations (device, guardian, relationship)
//! - Choreographic coordination for multi-party invitation ceremonies
//! - Integration with Web of Trust for trust relationship validation
//! - Capability-based authorization checks during invitation acceptance
//! - MPST protocol definitions and rumpsteak projections for invitations
//!
//! ## What Does NOT Belong Here
//!
//! - Effect handler implementations (belong in aura-effects)
//! - Handler composition or registry (belong in aura-composition)
//! - Low-level multi-party coordination (belong in aura-protocol)
//! - Runtime assembly or effect system management
//! - Domain type definitions (belong in aura-journal/aura-wot/aura-verify)
//!
//! ## Design Principles
//!
//! - Choreographic programming with MPST for distributed coordination
//! - All protocols are stateless; state lives in journals and relational contexts
//! - Invitation ceremonies are transactional: either fully succeed or cleanly fail
//! - Integration with guard chain ensures authorization checks before acceptance
//! - Metadata privacy through capability-scoped relationship visibility
//!
//! ## Key Protocols
//!
//! - **Device Invitation**: Onboarding new devices into an authority
//! - **Guardian Invitation**: Establishing guardian relationships
//! - **Relationship Formation**: Creating peer relationships between authorities
//! - **Acceptance Choreography**: Multi-party agreement on invitation terms

#![allow(missing_docs)]
#![forbid(unsafe_code)]

// =============================================================================
// Core Modules (New Architecture)
// =============================================================================

/// Guard types for invitation operations
///
/// Provides `GuardSnapshot`, `GuardOutcome`, `EffectCommand`, and related types
/// for guard chain integration following the pattern from `aura-rendezvous`.
pub mod guards;

/// Invitation service coordinator
///
/// Main service for invitation operations with guard chain integration.
/// All operations return `GuardOutcome` for the caller to execute.
pub mod service;

/// MPST choreography definitions for invitation protocols
///
/// Provides `InvitationExchange` and `GuardianInvitation` choreographies
/// with guard annotations for capability and flow budget enforcement.
pub mod protocol;

/// Domain fact types for invitation state changes
pub mod facts;

/// View delta and reducer for invitation facts
pub mod view;

// =============================================================================
// Legacy Modules (To be refactored in later phases)
// =============================================================================

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

// Re-export domain fact types
pub use facts::{InvitationFact, InvitationFactReducer, INVITATION_FACT_TYPE_ID};

// Re-export view delta types
pub use view::{InvitationDelta, InvitationViewReducer};

// Re-export protocol types
pub use protocol::{
    GuardianAccept, GuardianConfirm, GuardianDecline, GuardianInvitationState, GuardianRequest,
    InvitationAck, InvitationExchangeState, InvitationOffer, InvitationResponse,
    EXCHANGE_PROTOCOL_ID, GUARDIAN_PROTOCOL_ID, PROTOCOL_NAMESPACE, PROTOCOL_VERSION,
};

// Re-export guard types
pub use guards::{
    check_capability, check_flow_budget, EffectCommand, GuardDecision, GuardOutcome, GuardRequest,
    GuardSnapshot,
};

// Re-export service types
pub use service::{
    Invitation, InvitationConfig, InvitationResult, InvitationService, InvitationStatus,
    InvitationType,
};

// Re-export core types
pub use aura_core::{
    AccountId, AuraError, AuraResult, DeviceId, GuardianId, Journal, RelationshipId,
    RelationshipType, TrustLevel,
};

// Re-export WoT types (using Biscuit tokens instead of legacy capabilities)
pub use aura_wot::{AccountAuthority, BiscuitError, BiscuitTokenManager};
pub use biscuit_auth::Biscuit as BiscuitToken;

// Re-export auth types
pub use aura_authenticate::{
    AuthenticationError, AuthenticationResult, IdentityProof, VerifiedIdentity,
};

// Re-export core effect types
pub use aura_core::effects::{
    ConsoleEffects, CryptoEffects, JournalEffects, NetworkEffects, PhysicalTimeEffects,
};
