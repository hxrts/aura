//! Rendezvous coordination protocol messages
//!
//! This module contains message types for rendezvous protocols:
//! - Rendezvous and peer discovery

use aura_core::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};

use crate::protocols::rendezvous_constants::PROTOCOL_RENDEZVOUS;

// Re-export rendezvous message types
pub use crate::messages::rendezvous::*;

/// Rendezvous coordination message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendezvousEnvelope {
    /// Authority that sent this message
    pub sender: AuthorityId,
    /// Message sequence number
    pub sequence: u64,
    /// Timestamp when message was created
    pub timestamp: u64,
    /// The actual rendezvous protocol payload
    pub payload: RendezvousPayload,
}

/// Union of all rendezvous protocol payloads
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RendezvousPayload {
    /// Rendezvous and peer discovery messages
    Rendezvous(RendezvousMessage),
}

impl RendezvousEnvelope {
    /// Create a new rendezvous message
    pub fn new(sender: AuthorityId, sequence: u64, timestamp: u64, payload: RendezvousPayload) -> Self {
        Self {
            sender,
            sequence,
            timestamp,
            payload,
        }
    }

    /// Get the protocol type for this message
    pub fn protocol_type(&self) -> &'static str {
        match &self.payload {
            RendezvousPayload::Rendezvous(_) => PROTOCOL_RENDEZVOUS,
        }
    }
}
