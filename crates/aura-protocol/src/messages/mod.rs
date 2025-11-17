//! Protocol Message Types
//!
//! This module provides message type definitions for all Aura distributed protocols,
//! organized by domain for clear separation of concerns.
//!
//! ## Message Domains
//!
//! - **Crypto**: Threshold cryptography protocols (DKD, FROST, resharing)
//!   - Key derivation coordination messages
//!   - Threshold signature orchestration
//!   - Key resharing protocol messages
//!
//! - **Social**: Peer coordination protocols (rendezvous, discovery)
//!   - Transport offer/answer messages
//!   - PSK handshake transcripts
//!   - Capability announcements
//!
//! - **Common**: Shared message infrastructure
//!   - Generic message envelopes
//!   - Protocol error types
//!   - Version compatibility headers
//!
//! ## Design Principles
//!
//! - **Domain Separation**: Each protocol domain has its own message namespace
//! - **Consistent Serialization**: All messages use serde traits for wire format
//! - **Version Compatibility**: Forward and backward compatibility through versioning
//! - **Type Safety**: Strong typing prevents message type confusion
//! - **Unified Envelope**: Single `AuraMessage` enum for transport layer
//!
//! ## Usage Pattern
//!
//! ```rust,ignore
//! use crate::messages::{AuraMessage, crypto::CryptoMessage, social::SocialMessage};
//!
//! // Create protocol-specific message
//! let crypto_msg = CryptoMessage::new(sender_id, sequence, timestamp, payload);
//! let unified_msg = AuraMessage::Crypto(crypto_msg);
//!
//! // Serialize for transport
//! let wire_bytes = bincode::serialize(&unified_msg)?;
//! ```

// Domain-specific message modules
pub mod crypto;
pub mod social_rendezvous;
pub mod social_types;

// Shared infrastructure
pub mod common_envelope;
pub mod common_error;

// Re-export main message types organized by domain
pub use common_envelope::*;
pub use common_error::*;
pub use crypto::*;
pub use social_types::*;

/// Current wire format version
pub const WIRE_FORMAT_VERSION: u16 = 1;

/// Unified message envelope for all protocols
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AuraMessage {
    /// Threshold cryptography protocol messages
    Crypto(crypto::CryptoMessage),
    /// Social coordination protocol messages
    Social(social_types::SocialMessage),
}

impl AuraMessage {
    /// Get the protocol domain for this message
    pub fn domain(&self) -> &'static str {
        match self {
            AuraMessage::Crypto(_) => "crypto",
            AuraMessage::Social(_) => "social",
        }
    }

    /// Get the specific protocol type for this message
    pub fn protocol_type(&self) -> &'static str {
        match self {
            AuraMessage::Crypto(msg) => msg.protocol_type(),
            AuraMessage::Social(msg) => msg.protocol_type(),
        }
    }
}
