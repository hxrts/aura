//! Agent Handlers - Domain-Specific Effect Handlers
//!
//! This module contains domain-specific handlers that implement multi-party
//! protocols and workflows using shared utilities.

pub mod auth;
pub mod auth_service;
pub mod chat_service;
pub mod invitation;
pub mod invitation_bridge;
pub mod invitation_service;
pub mod logical_clock_service;
pub mod ota;
pub mod recovery;
pub mod recovery_service;
pub mod rendezvous;
pub mod rendezvous_bridge;
pub mod rendezvous_service;
pub mod sessions;
pub mod shared;

// Re-export session types for public API
pub use sessions::coordination::SessionOperations;
pub use sessions::service::SessionServiceApi;
pub use sessions::shared::{SessionHandle, SessionStats};

// Re-export auth types for public API
pub use auth::{AuthChallenge, AuthHandler, AuthMethod, AuthResponse, AuthResult};
pub use auth_service::AuthServiceApi;

// Re-export chat types for public API
pub use chat_service::ChatServiceApi;
pub use aura_chat::{ChatGroupId, ChatMessageId};

// Re-export invitation types for public API
pub use invitation::{
    Invitation, InvitationHandler, InvitationResult, InvitationStatus, InvitationType,
    ShareableInvitation, ShareableInvitationError,
};
pub use invitation_service::InvitationServiceApi;

// Re-export recovery types for public API
pub use recovery::{
    GuardianApproval, RecoveryHandler, RecoveryOperation, RecoveryRequest, RecoveryResult,
    RecoveryState,
};
pub use recovery_service::RecoveryServiceApi;

// Backwards-compatible aliases (prefer *ServiceApi names for clarity)
#[deprecated(note = "Use AuthServiceApi instead.")]
pub type AuthService = AuthServiceApi;
#[deprecated(note = "Use ChatServiceApi instead.")]
pub type ChatService = ChatServiceApi;
#[deprecated(note = "Use InvitationServiceApi instead.")]
pub type InvitationService = InvitationServiceApi;
#[deprecated(note = "Use RecoveryServiceApi instead.")]
pub type RecoveryService = RecoveryServiceApi;
#[deprecated(note = "Use SessionServiceApi instead.")]
pub type SessionService = SessionServiceApi;

// Re-export rendezvous types for public API
pub use rendezvous::{ChannelResult, RendezvousHandler, RendezvousResult};
pub use rendezvous_service::RendezvousServiceApi;

// Re-export OTA types for public API
pub use ota::{OtaHandler, UpdateInfo, UpdateResult, UpdateStatus};

// Re-export sync/maintenance types for CLI/tooling usage
pub use aura_sync::maintenance::UpgradeProposal;
pub use aura_sync::protocols::ota::UpgradeKind;
pub use aura_sync::services::HealthStatus;

// Re-export authentication types for CLI/tooling usage
pub use aura_authentication::{DkdConfig, DkdProtocol, RecoveryContext, RecoveryOperationType};

// Re-export recovery types for CLI/tooling usage
pub use aura_recovery::guardian_setup::GuardianSetupCoordinator;
pub use aura_recovery::guardian_key_recovery::GuardianKeyApproval;
pub use aura_recovery::recovery_protocol::{
    RecoveryOperation as ProtocolRecoveryOperation, RecoveryProtocol, RecoveryProtocolHandler,
    RecoveryRequest as ProtocolRecoveryRequest,
};
pub use aura_recovery::types::{
    GuardianProfile, GuardianSet, RecoveryDispute, RecoveryEvidence, RecoveryShare,
};
pub use aura_recovery::RecoveryResponse;
