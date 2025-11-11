//! Agent Handler Re-exports
//!
//! This module re-exports agent handler implementations from aura-protocol.
//! Agent handler implementations have been moved to aura-protocol as per the
//! unified architecture.

// Local agent handlers
pub mod invitations;
pub mod journal;
pub mod recovery;
pub mod sessions;
pub mod storage;
pub mod auth;

// Re-export agent handlers from aura-protocol
pub use aura_protocol::handlers::agent::{
    auth::AuthenticationHandler, session::MemorySessionHandler, system::AgentEffectSystemHandler,
};

// Re-export local agent handlers
pub use invitations::InvitationOperations;
pub use recovery::{RecoveryOperations, RecoveryStatus};
pub use sessions::SessionOperations;
pub use storage::StorageOperations;
