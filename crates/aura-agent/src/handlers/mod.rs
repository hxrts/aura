//! Agent Handler Re-exports
//!
//! This module re-exports agent handler implementations from aura-protocol.
//! Agent handler implementations have been moved to aura-protocol as per the
//! unified architecture.

// Local agent handlers
pub mod invitations;
pub mod ota;
pub mod recovery;
pub mod sessions;
pub mod storage;

// Re-export agent handlers from aura-protocol
pub use aura_protocol::handlers::agent::{
    auth::AuthenticationHandler, session::MemorySessionHandler, system::AgentEffectSystemHandler,
};

// Re-export handler types from aura-protocol that agent needs
pub use aura_core::effects::ExecutionMode;
pub use aura_protocol::{AuraContext, AuraHandler, AuraHandlerError, EffectType};

// Re-export local agent handlers
pub use invitations::InvitationOperations;
pub use ota::{OtaOperations, UpgradeProposalState, UpgradeStatus};
pub use recovery::RecoveryOperations;
pub use sessions::{SessionHandle, SessionOperations, SessionStats};
pub use storage::StorageOperations;
