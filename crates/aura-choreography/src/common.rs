//! Common types and utilities for protocol implementations

use aura_types::DeviceId;
use std::collections::HashMap;
use uuid::Uuid;

/// Context for choreography execution
#[derive(Debug, Clone)]
pub struct ChoreographyContext {
    /// Session identifier
    pub session_id: Uuid,

    /// This device's ID
    pub device_id: DeviceId,

    /// Current epoch
    pub epoch: u64,

    /// Session metadata
    pub metadata: HashMap<String, String>,
}

impl ChoreographyContext {
    /// Create a new choreography context
    pub fn new(session_id: Uuid, device_id: DeviceId, epoch: u64) -> Self {
        Self {
            session_id,
            device_id,
            epoch,
            metadata: HashMap::new(),
        }
    }
}

/// Protocol execution errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum ProtocolError {
    #[error("Invalid participant")]
    InvalidParticipant,

    #[error("Execution failed: {message}")]
    ExecutionFailed { message: String },

    #[error("Timeout occurred")]
    Timeout,

    #[error("Byzantine behavior detected")]
    ByzantineBehavior,

    #[error("Insufficient participants")]
    InsufficientParticipants,

    #[error("Internal error: {message}")]
    Internal { message: String },
}

/// Result type for protocol operations
pub type ProtocolResult<T> = Result<T, ProtocolError>;
