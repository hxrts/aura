//! Agent Handlers - Domain-Specific Effect Handlers
//!
//! This module contains domain-specific handlers that implement multi-party
//! protocols and workflows using shared utilities.

pub mod auth;
pub mod auth_service;
pub mod chat_service;
pub mod device_epoch_rotation;
pub mod invitation;
pub mod invitation_service;
pub mod ota_activation_service;
pub mod recovery;
pub mod recovery_service;
pub mod rendezvous;
pub(crate) mod rendezvous_identity;
pub mod rendezvous_service;
pub mod sessions;
pub mod shared;

// Re-export session types for public API
pub use sessions::coordination::SessionOperations;
pub use sessions::service::SessionServiceApi;
pub use sessions::shared::{SessionHandle, SessionStats};
pub use sessions::SessionCoordinationRole;

// Re-export auth types for public API
pub use auth::{
    AuthChallenge, AuthHandler, AuthMethod, AuthResponse, AuthResult, AuthenticationStatus,
};
pub use auth_service::AuthServiceApi;

// Re-export chat types for public API
pub use aura_chat::{ChatGroupId, ChatMessageId};
pub use chat_service::ChatServiceApi;

// Re-export invitation types for public API
pub use invitation::{
    Invitation, InvitationHandler, InvitationResult, InvitationStatus, InvitationType,
    ShareableInvitation, ShareableInvitationError,
};
pub use invitation_service::InvitationServiceApi;
pub use ota_activation_service::OtaActivationServiceApi;

// Re-export recovery types for public API
pub use recovery::{
    recovery_guardian_public_key_storage_key, GuardianApproval, RecoveryHandler, RecoveryOperation,
    RecoveryRequest, RecoveryResult, RecoveryState,
};
pub use recovery_service::RecoveryServiceApi;

// Re-export rendezvous types for public API
pub use rendezvous::{ChannelResult, RendezvousHandler, RendezvousResult};
pub use rendezvous_service::RendezvousServiceApi;

// Re-export sync/maintenance types for CLI/tooling usage
pub use aura_sync::protocols::ota::UpgradeKind;
pub use aura_sync::services::HealthStatus;
pub use aura_sync::services::UpgradeProposal;

// Re-export authentication types for CLI/tooling usage
pub use aura_authentication::{DkdConfig, DkdProtocol, RecoveryContext, RecoveryOperationType};

// Re-export recovery types for CLI/tooling usage
pub use aura_recovery::guardian_key_recovery::GuardianKeyApproval;
pub use aura_recovery::guardian_setup::GuardianSetupCoordinator;
pub use aura_recovery::recovery_protocol::{
    RecoveryOperation as ProtocolRecoveryOperation, RecoveryProtocol, RecoveryProtocolHandler,
    RecoveryRequest as ProtocolRecoveryRequest,
};
pub use aura_recovery::types::{
    GuardianProfile, GuardianSet, RecoveryDispute, RecoveryEvidence, RecoveryShare,
};
pub use aura_recovery::RecoveryResponse;
