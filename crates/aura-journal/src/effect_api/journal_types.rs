//! Journal-specific types for effect_api operations

use super::intent::IntentId;
use aura_core::types::Epoch;
use serde::{Deserialize, Serialize};

/// Journal operation errors
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum JournalError {
    /// Tree operation failed at specific epoch
    #[error("Tree operation failed at epoch {epoch}: {reason}")]
    TreeOperationFailed {
        /// Epoch where failure occurred
        epoch: Epoch,
        /// Description of failure
        reason: String,
    },

    /// Invalid threshold signature
    #[error("Invalid signature for epoch {epoch}: {reason}")]
    InvalidSignature {
        /// Epoch of invalid signature
        epoch: Epoch,
        /// Reason for invalidity
        reason: String,
    },

    /// Invalid intent operation
    #[error("Invalid intent operation: {0}")]
    InvalidIntentOperation(String),

    /// Intent not found in pool
    #[error("Intent {0} not found")]
    IntentNotFound(IntentId),

    /// Intent is retracted (already completed)
    #[error("Intent {0} is retracted")]
    IntentRetracted(IntentId),

    /// Capability validation failed
    #[error("Capability validation failed: {0}")]
    CapabilityValidationFailed(String),

    /// Capability expired
    #[error("Capability {0} has expired")]
    CapabilityExpired(String),

    /// Tree not found at epoch
    #[error("Tree state not found at epoch {0}")]
    TreeNotFound(Epoch),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// CRDT merge failed
    #[error("CRDT merge failed: {0}")]
    CrdtMergeFailed(String),
}

/// Journal statistics
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalStats {
    /// Number of tree operations
    pub num_ops: u64,
    /// Number of pending intents
    pub num_intents: u32,
    /// Number of retracted intents
    pub num_retractions: u32,
    /// Latest epoch
    pub latest_epoch: Option<Epoch>,
    /// Number of devices in tree
    pub num_devices: u32,
    /// Number of guardians in tree
    pub num_guardians: u32,
}
