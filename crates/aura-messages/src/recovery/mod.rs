//! Account recovery protocol messages
//!
//! This module contains message types for account recovery protocols:
//! - Guardian coordination
//! - Emergency recovery procedures

pub mod guardian;

// Re-export recovery message types
pub use guardian::*;

use aura_types::{DeviceId, SessionId};
use serde::{Deserialize, Serialize};

/// Recovery protocol message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryMessage {
    /// Session this message belongs to
    pub session_id: SessionId,
    /// Device that sent this message
    pub sender_id: DeviceId,
    /// Message sequence number within session
    pub sequence: u64,
    /// Timestamp when message was created
    pub timestamp: u64,
    /// The actual recovery protocol payload
    pub payload: RecoveryPayload,
}

/// Union of all recovery protocol payloads
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryPayload {
    /// Guardian coordination messages
    Guardian(GuardianMessage),
}

impl RecoveryMessage {
    /// Create a new recovery message
    pub fn new(
        session_id: SessionId,
        sender_id: DeviceId,
        sequence: u64,
        timestamp: u64,
        payload: RecoveryPayload,
    ) -> Self {
        Self {
            session_id,
            sender_id,
            sequence,
            timestamp,
            payload,
        }
    }

    /// Get the protocol type for this message
    pub fn protocol_type(&self) -> &'static str {
        match &self.payload {
            RecoveryPayload::Guardian(_) => "guardian",
        }
    }
}
