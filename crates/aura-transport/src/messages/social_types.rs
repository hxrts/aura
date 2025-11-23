//! Social coordination protocol messages
//!
//! This module contains message types for social protocols:
//! - Rendezvous and peer discovery

use aura_core::identifiers::DeviceId;
use serde::{Deserialize, Serialize};

// Re-export social message types
pub use crate::messages::social_rendezvous::*;

/// Social coordination message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialMessage {
    /// Device that sent this message
    pub sender_id: DeviceId,
    /// Message sequence number
    pub sequence: u64,
    /// Timestamp when message was created
    pub timestamp: u64,
    /// The actual social protocol payload
    pub payload: SocialPayload,
}

/// Union of all social protocol payloads
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SocialPayload {
    /// Rendezvous and peer discovery messages
    Rendezvous(RendezvousMessage),
}

impl SocialMessage {
    /// Create a new social message
    pub fn new(sender_id: DeviceId, sequence: u64, timestamp: u64, payload: SocialPayload) -> Self {
        Self {
            sender_id,
            sequence,
            timestamp,
            payload,
        }
    }

    /// Get the protocol type for this message
    pub fn protocol_type(&self) -> &'static str {
        match &self.payload {
            SocialPayload::Rendezvous(_) => "rendezvous",
        }
    }
}