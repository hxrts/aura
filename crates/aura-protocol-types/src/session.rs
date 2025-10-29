//! Session state and transitions

use serde::{Deserialize, Serialize};

/// Session status enumeration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    /// Session is initializing
    Initializing,
    /// Session is actively running
    Active,
    /// Session completed successfully
    Completed,
    /// Session failed with error
    Failed(String),
    /// Session was terminated
    Terminated,
}

/// Information about a session's current status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatusInfo {
    pub session_id: uuid::Uuid,
    pub status: SessionStatus,
    pub protocol_type: SessionProtocolType,
    pub participants: Vec<aura_types::DeviceId>,
    pub is_final: bool,
}

/// Type of session protocol being run
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionProtocolType {
    Dkd,
    Recovery,
    Resharing,
    FrostDkg,
    FrostSigning,
    Locking,
    Agent,
}
