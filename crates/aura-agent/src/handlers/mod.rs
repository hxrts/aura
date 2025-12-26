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
pub use sessions::service::SessionService;
pub use sessions::shared::{SessionHandle, SessionStats};

// Re-export auth types for public API
pub use auth::{AuthChallenge, AuthHandler, AuthMethod, AuthResponse, AuthResult};
pub use auth_service::AuthService;

// Re-export chat types for public API
pub use chat_service::ChatService;

// Re-export invitation types for public API
pub use invitation::{
    Invitation, InvitationHandler, InvitationResult, InvitationStatus, InvitationType,
    ShareableInvitation, ShareableInvitationError,
};
pub use invitation_service::InvitationService;

// Re-export recovery types for public API
pub use recovery::{
    GuardianApproval, RecoveryHandler, RecoveryOperation, RecoveryRequest, RecoveryResult,
    RecoveryState,
};
pub use recovery_service::RecoveryService;

// Re-export rendezvous types for public API
pub use rendezvous::{ChannelResult, RendezvousHandler, RendezvousResult};
pub use rendezvous_service::RendezvousServiceApi;

// Re-export OTA types for public API
pub use ota::{OtaHandler, UpdateInfo, UpdateResult, UpdateStatus};
