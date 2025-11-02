//! Threshold cryptography protocol messages
//!
//! This module contains message types for distributed cryptographic protocols:
//! - DKD (Deterministic Key Derivation)
//! - FROST threshold signatures
//! - Key resharing and rotation

pub mod dkd;
pub mod frost;
pub mod resharing;

// Re-export crypto message types
pub use dkd::*;
pub use frost::*;
pub use resharing::*;

use aura_types::{DeviceId, SessionId};
use serde::{Deserialize, Serialize};

/// Unified cryptographic protocol message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoMessage {
    /// Session this message belongs to
    pub session_id: SessionId,
    /// Device that sent this message
    pub sender_id: DeviceId,
    /// Message sequence number within session
    pub sequence: u64,
    /// Timestamp when message was created
    pub timestamp: u64,
    /// The actual crypto protocol payload
    pub payload: CryptoPayload,
}

/// Union of all cryptographic protocol payloads
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CryptoPayload {
    /// DKD protocol messages
    Dkd(DkdMessage),
    /// FROST protocol messages
    Frost(FrostMessage),
    /// Resharing protocol messages
    Resharing(ResharingMessage),
}

impl CryptoMessage {
    /// Create a new crypto message
    pub fn new(
        session_id: SessionId,
        sender_id: DeviceId,
        sequence: u64,
        timestamp: u64,
        payload: CryptoPayload,
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
            CryptoPayload::Dkd(_) => "dkd",
            CryptoPayload::Frost(_) => "frost",
            CryptoPayload::Resharing(_) => "resharing",
        }
    }
}
