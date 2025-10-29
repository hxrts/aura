//! Protocol messages for distributed cryptographic operations
//!
//! This module contains all message types used in Aura's distributed protocols:
//! - DKD (Deterministic Key Derivation)
//! - FROST threshold signatures
//! - Key resharing
//! - Account recovery

pub mod dkd;
pub mod frost;
pub mod recovery;
pub mod rendezvous;
pub mod resharing;

// Re-export all protocol message types
pub use dkd::*;
pub use frost::*;
pub use recovery::*;
pub use rendezvous::*;
pub use resharing::*;

use crate::serialization::WireSerializable;
use crate::versioning::{MessageVersion, VersionedMessage};
use aura_types::{DeviceId, SessionId};
use serde::{Deserialize, Serialize};

/// Base protocol message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMessage {
    /// Message format version
    pub version: MessageVersion,
    /// Session this message belongs to
    pub session_id: SessionId,
    /// Device that sent this message
    pub sender_id: DeviceId,
    /// Message sequence number within session
    pub sequence: u64,
    /// Timestamp when message was created
    pub timestamp: u64,
    /// The actual protocol-specific payload
    pub payload: ProtocolPayload,
}

/// Union of all protocol message payloads
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProtocolPayload {
    /// DKD protocol messages
    Dkd(DkdMessage),
    /// FROST protocol messages
    Frost(FrostMessage),
    /// Resharing protocol messages
    Resharing(ResharingMessage),
    /// Recovery protocol messages
    Recovery(RecoveryMessage),
    /// Rendezvous protocol messages
    Rendezvous(RendezvousEnvelope),
}

impl VersionedMessage for ProtocolMessage {
    fn version(&self) -> &MessageVersion {
        &self.version
    }
}

impl WireSerializable for ProtocolMessage {}

impl ProtocolMessage {
    /// Create a new protocol message
    pub fn new(
        session_id: SessionId,
        sender_id: DeviceId,
        sequence: u64,
        timestamp: u64,
        payload: ProtocolPayload,
    ) -> Self {
        Self {
            version: MessageVersion::CURRENT,
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
            ProtocolPayload::Dkd(_) => "dkd",
            ProtocolPayload::Frost(_) => "frost",
            ProtocolPayload::Resharing(_) => "resharing",
            ProtocolPayload::Recovery(_) => "recovery",
            ProtocolPayload::Rendezvous(_) => "rendezvous",
        }
    }
}
