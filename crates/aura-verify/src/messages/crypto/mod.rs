//! Layer 2: Threshold Cryptography Protocol Messages
//!
//! Message types for distributed cryptographic protocols: FROST threshold signatures,
//! key resharing, and key rotation. Provides typed message envelope (CryptoMessage) with
//! session tracking and sequence numbering.
//!
//! **Message Types**:
//! - **Resharing**: Key resharing/rotation protocol (per docs/001_system_architecture.md)
//! - **FROST**: Threshold signature coordination (use aura-frost crate for ceremonies)
//! - **(Future) DKD**: Deterministic key derivation (dedicated aura-dkd feature crate)
//!
//! **Design**: Message envelope includes session_id, sender_id, sequence, timestamp
//! for replay detection and distributed protocol coordination.

// pub mod dkd; // REMOVED: DKD messages moved to future aura-dkd feature crate
pub mod resharing;

// Re-export crypto message types
// pub use dkd::*; // REMOVED: DKD messages moved to future aura-dkd feature crate
pub use resharing::*;

use aura_core::identifiers::{DeviceId, SessionId};
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
    /// DKD protocol messages - Use future aura-dkd crate for DKD operations
    // Dkd(DkdMessage), // REMOVED: DKD messages moved to future aura-dkd feature crate
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
            // CryptoPayload::Dkd(_) => "dkd", // REMOVED: DKD messages moved to future aura-dkd feature crate
            CryptoPayload::Resharing(_) => "resharing",
        }
    }
}
