//! Transport layer messages
//!
//! This module contains message types for transport-level communication:
//! - Presence and capability announcements
//! - Connection establishment and management
//! - Transport-specific metadata

pub mod capability;
pub mod presence;

// Re-export transport message types
pub use capability::*;
pub use presence::*;

use crate::serialization::WireSerializable;
use crate::versioning::{MessageVersion, VersionedMessage};
use aura_types::DeviceId;
use serde::{Deserialize, Serialize};

/// Base transport message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportMessage {
    /// Message format version
    pub version: MessageVersion,
    /// Device that sent this message
    pub sender_id: DeviceId,
    /// Message sequence number
    pub sequence: u64,
    /// Timestamp when message was created
    pub timestamp: u64,
    /// The transport-specific payload
    pub payload: TransportPayload,
}

/// Union of all transport message payloads
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransportPayload {
    /// Presence announcements
    Presence(PresenceMessage),
    /// Capability announcements
    Capability(CapabilityMessage),
}

impl VersionedMessage for TransportMessage {
    fn version(&self) -> &MessageVersion {
        &self.version
    }
}

impl WireSerializable for TransportMessage {}

impl TransportMessage {
    /// Create a new transport message
    pub fn new(
        sender_id: DeviceId,
        sequence: u64,
        timestamp: u64,
        payload: TransportPayload,
    ) -> Self {
        Self {
            version: MessageVersion::CURRENT,
            sender_id,
            sequence,
            timestamp,
            payload,
        }
    }

    /// Get the message type for this transport message
    pub fn message_type(&self) -> &'static str {
        match &self.payload {
            TransportPayload::Presence(_) => "presence",
            TransportPayload::Capability(_) => "capability",
        }
    }
}
