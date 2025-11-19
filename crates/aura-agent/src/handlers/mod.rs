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
pub type AgentEffectSystemHandler = ();

/// Effect types for handler routing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectType {
    /// Network communication effects
    Network,
    /// Storage effects
    Storage,
    /// Cryptographic effects
    Crypto,
    /// Time effects
    Time,
    /// Journal effects
    Journal,
}

/// Execution context for effect handlers
#[derive(Debug, Clone)]
pub struct AuraContext {
    /// Device ID for this context
    pub device_id: crate::DeviceId,
}

impl AuraContext {
    /// Create a testing context
    pub fn for_testing(device_id: crate::DeviceId) -> Self {
        Self { device_id }
    }
}

impl Default for AuraContext {
    fn default() -> Self {
        Self {
            device_id: crate::DeviceId::new(),
        }
    }
}

/// Errors from Aura handler operations
#[derive(Debug, Clone)]
pub enum AuraHandlerError {
    /// Effect serialization failed
    EffectSerialization {
        /// Effect type that failed
        effect_type: EffectType,
        /// Operation that failed
        operation: String,
        /// Error source
        source: String,
    },
    /// Effect deserialization failed
    EffectDeserialization {
        /// Effect type that failed
        effect_type: EffectType,
        /// Operation that failed
        operation: String,
        /// Error source
        source: String,
    },
    /// Context error
    ContextError {
        /// Error message
        message: String,
    },
    /// Generic handler error
    Generic(String),
}

impl std::fmt::Display for AuraHandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EffectSerialization { effect_type, operation, source } => {
                write!(f, "Effect serialization failed for {:?} operation {}: {}", effect_type, operation, source)
            }
            Self::EffectDeserialization { effect_type, operation, source } => {
                write!(f, "Effect deserialization failed for {:?} operation {}: {}", effect_type, operation, source)
            }
            Self::ContextError { message } => write!(f, "Context error: {}", message),
            Self::Generic(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for AuraHandlerError {}

// Re-export local agent handlers
pub use invitations::InvitationOperations;
pub use ota::{OtaOperations, UpgradeProposalState, UpgradeStatus};
pub use recovery::RecoveryOperations;
pub use sessions::{SessionHandle, SessionOperations, SessionStats};
pub use storage::StorageOperations;
