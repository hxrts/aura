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

// TODO: Re-export agent handlers from aura-protocol once refactored
// pub use aura_protocol::handlers::agent::{
//     auth::AuthenticationHandler, session::MemorySessionHandler, system::AgentEffectSystemHandler,
// };

// Re-export handler types from aura-protocol that agent needs
pub use aura_core::effects::ExecutionMode;
// TODO: Re-enable once aura-protocol exports are refactored
// pub use aura_protocol::composition::{AuraHandler, EffectType};
// pub use aura_protocol::handlers::context_immutable::AuraContext;
// pub use aura_protocol::internal::AuraHandlerError;

// Temporary stubs until refactored
pub type AuraHandler = ();
pub type EffectType = ();
pub type AuraContext = ();
#[derive(Debug, Clone)]
pub struct AuraHandlerError(pub String);
impl std::fmt::Display for AuraHandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for AuraHandlerError {}

// Re-export local agent handlers
pub use invitations::InvitationOperations;
pub use ota::{OtaOperations, UpgradeProposalState, UpgradeStatus};
pub use recovery::RecoveryOperations;
pub use sessions::{SessionHandle, SessionOperations, SessionStats};
pub use storage::StorageOperations;
