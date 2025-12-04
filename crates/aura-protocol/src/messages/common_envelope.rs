//! Generic message envelope and signing wrappers
//!
//! Provides unified types for message envelopes and cryptographic signing:
//!
//! - [`Signed<T>`]: Generic wrapper for cryptographically signed payloads
//! - [`WireEnvelope<T>`]: Protocol envelope with routing and versioning metadata
//!
//! # Usage
//!
//! ## Signing any payload
//! ```rust,ignore
//! use aura_protocol::messages::{Signed, WireEnvelope};
//!
//! // Wrap a payload with signature
//! let signed_msg: Signed<MyPayload> = Signed::new(payload, signature, sender_id);
//!
//! // Use with wire envelope for transport
//! let envelope: WireEnvelope<Signed<MyPayload>> = WireEnvelope::new(..., signed_msg);
//! ```

use aura_core::identifiers::{DeviceId, SessionId};
use aura_core::{AuthorityId, Ed25519Signature};
use serde::{Deserialize, Serialize};

use super::WIRE_FORMAT_VERSION;

/// Generic wrapper for cryptographically signed payloads
///
/// Use this to add a signature and sender identity to any serializable payload.
/// This pattern replaces protocol-specific signed wrapper types like `SignedProposal`,
/// `SignedCommit`, etc.
///
/// # Type Parameters
///
/// * `T` - The payload type being signed (must be Serialize + Deserialize)
///
/// # Example
///
/// ```rust,ignore
/// // Before: Protocol-specific signed types
/// struct SignedProposal {
///     proposal: Proposal,
///     signature: Ed25519Signature,
///     sender_id: AuthorityId,
/// }
///
/// // After: Generic Signed<T>
/// type SignedProposal = Signed<Proposal>;
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Signed<T> {
    /// The signed payload
    pub payload: T,
    /// Cryptographic signature over the serialized payload
    pub signature: Ed25519Signature,
    /// Identity of the signer
    pub sender_id: AuthorityId,
}

impl<T> Signed<T> {
    /// Create a new signed wrapper
    pub fn new(payload: T, signature: Ed25519Signature, sender_id: AuthorityId) -> Self {
        Self {
            payload,
            signature,
            sender_id,
        }
    }

    /// Get a reference to the payload
    pub fn payload(&self) -> &T {
        &self.payload
    }

    /// Get the signature
    pub fn signature(&self) -> &Ed25519Signature {
        &self.signature
    }

    /// Get the sender ID
    pub fn sender_id(&self) -> AuthorityId {
        self.sender_id
    }

    /// Unwrap into the inner payload
    pub fn into_payload(self) -> T {
        self.payload
    }

    /// Map the payload to a different type
    pub fn map<U, F>(self, f: F) -> Signed<U>
    where
        F: FnOnce(T) -> U,
    {
        Signed {
            payload: f(self.payload),
            signature: self.signature,
            sender_id: self.sender_id,
        }
    }
}

impl<T: Clone> Signed<T> {
    /// Clone the payload without consuming self
    pub fn clone_payload(&self) -> T {
        self.payload.clone()
    }
}

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
