#![allow(
    missing_docs,
    unused_variables,
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    clippy::match_like_matches_macro,
    clippy::type_complexity,
    clippy::while_let_loop,
    clippy::redundant_closure,
    clippy::large_enum_variant,
    clippy::unused_unit,
    clippy::get_first,
    clippy::single_range_in_vec_init,
    clippy::disallowed_methods, // Orchestration layer coordinates time/random effects
    deprecated // Deprecated time/random functions used intentionally for effect coordination
)]
//! Layer 4: Protocol Message Types - Domain-Separated, Versioned, Typed
//!
//! Message type definitions organized by domain with version compatibility.
//! Enables choreographic message routing with type-safe domain separation.
//!
//! **Message Domains**:
//! - **crypto**: Threshold cryptography (FROST, resharing, key derivation)
//! - **social_types**: Social coordination (peer discovery, rendezvous)
//! - **common_envelope**: Message envelope infrastructure (versioning, wire format)
//!
//! **Design Principles** (per docs/001_system_architecture.md, docs/107_mpst_and_choreography.md):
//! - **Domain separation**: Each protocol namespace isolated (prevents message confusion)
//! - **Type safety**: Strong typing enables compile-time message validation
//! - **Versioning**: WIRE_FORMAT_VERSION for forward/backward compatibility
//! - **Unified envelope**: Single AuraMessage enum routes messages to correct handler
//! - **Serialization**: serde bincode for deterministic wire format (enables commitment verification)
//! - **Choreography integration**: Messages typed for session type matching (MPST)
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
//! let wire_bytes = aura_core::util::serialization::to_vec(&unified_msg)?;
//! ```

// Domain-specific message modules
pub mod crypto;
pub mod social_rendezvous;
pub mod social_types;

// Shared infrastructure
pub mod common_envelope;

// Re-export main message types organized by domain
pub use common_envelope::{Signed, WireEnvelope};
pub use crypto::*;
pub use social_types::*;

/// Current wire format version
pub const WIRE_FORMAT_VERSION: u16 = crate::config::DEFAULT_WIRE_FORMAT_VERSION;

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
