//! Aura Protocol Messages
//!
//! This crate provides message types for all Aura distributed protocols:
//!
//! - **Crypto**: Threshold cryptography protocols (DKD, FROST, resharing)
//! - **Social**: Social coordination protocols (rendezvous, SSB)
//! - **Recovery**: Account recovery protocols (guardian coordination)
//! - **Common**: Shared message infrastructure (envelopes, errors)
//!
//! # Architecture
//!
//! All message types are organized by domain with clean separation of concerns:
//! - Protocol-specific messages in domain modules
//! - Shared infrastructure in common module
//! - Consistent serialization using serde traits
//! - Version compatibility checking

#![allow(missing_docs)] // TODO: Add comprehensive documentation

// Domain-specific message modules
pub mod crypto;
pub mod recovery;
pub mod social;

// Shared infrastructure
pub mod common;

// Legacy modules (will be removed)
mod versioning;

// Integration examples and guidelines
// TODO: Re-enable when error API is available
// #[cfg(test)]
// pub mod integration_example;

// Re-export main message types organized by domain
pub use common::*;
pub use crypto::*;
pub use recovery::*;
pub use social::*;

// Legacy re-exports for compatibility (will be removed)
pub use versioning::*;

/// Current wire format version
pub const WIRE_FORMAT_VERSION: u16 = 1;

/// Unified message envelope for all protocols
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AuraMessage {
    /// Threshold cryptography protocol messages
    Crypto(crypto::CryptoMessage),
    /// Social coordination protocol messages  
    Social(social::SocialMessage),
    /// Account recovery protocol messages
    Recovery(recovery::RecoveryMessage),
}

impl AuraMessage {
    /// Get the protocol domain for this message
    pub fn domain(&self) -> &'static str {
        match self {
            AuraMessage::Crypto(_) => "crypto",
            AuraMessage::Social(_) => "social",
            AuraMessage::Recovery(_) => "recovery",
        }
    }

    /// Get the specific protocol type for this message
    pub fn protocol_type(&self) -> &'static str {
        match self {
            AuraMessage::Crypto(msg) => msg.protocol_type(),
            AuraMessage::Social(msg) => msg.protocol_type(),
            AuraMessage::Recovery(msg) => msg.protocol_type(),
        }
    }
}
