//! Generic message envelope wrapper
//!
//! Provides a unified envelope format for all message types.

use aura_core::identifiers::{DeviceId, SessionId};
use serde::{Deserialize, Serialize};

use super::WIRE_FORMAT_VERSION;

/// Generic message envelope for wire protocol communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireEnvelope<T> {
    /// Message format version
    pub version: u16,
    /// Session this message belongs to (optional for some protocols)
    pub session_id: Option<SessionId>,
    /// Device that sent this message
    pub sender_id: DeviceId,
    /// Message sequence number
    pub sequence: u64,
    /// Timestamp when message was created
    pub timestamp: u64,
    /// The actual message payload
    pub payload: T,
}

impl<T> WireEnvelope<T> {
    /// Create a new message envelope
    pub fn new(
        session_id: Option<SessionId>,
        sender_id: DeviceId,
        sequence: u64,
        timestamp: u64,
        payload: T,
    ) -> Self {
        Self {
            version: WIRE_FORMAT_VERSION,
            session_id,
            sender_id,
            sequence,
            timestamp,
            payload,
        }
    }

    /// Check if the message version is compatible
    pub fn is_version_compatible(&self, max_supported: u16) -> bool {
        self.version <= max_supported
    }
}
