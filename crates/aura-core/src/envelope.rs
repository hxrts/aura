//! Unified envelope format for shareable Aura payloads.
//!
//! # Invariants
//!
//! - Version field enables forward compatibility
//! - `encode()` / `decode()` are symmetric (round-trip safe)
//! - Invite code format: `aura:v{version}:{base64(bincode(envelope))}`
//!
//! # Wire Format
//!
//! Bincode encoding with the following layout:
//! - version: u8
//! - kind: u8 (discriminant)
//! - payload_len: varint
//! - payload: [u8; payload_len]
//!
//! # Safety
//!
//! This module is `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};

/// Protocol version for envelope format.
pub const ENVELOPE_VERSION_CURRENT: u8 = 1;

/// Maximum payload size in bytes.
///
/// Prevents unbounded allocations during decode.
pub const PAYLOAD_BYTES_MAX: usize = 64 * 1024; // 64 KiB

/// Unified envelope for shareable Aura payloads.
///
/// Provides a common wrapper for:
/// - Invitation codes (base64 encoded for human sharing)
/// - LAN discovery packets (after magic bytes)
/// - Rendezvous flood packets (encrypted payload)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraEnvelope {
    /// Protocol version for forward compatibility.
    ///
    /// Decoders should accept versions >= current and handle
    /// gracefully (unknown kinds become errors, not panics).
    pub version: u8,

    /// Payload kind discriminator.
    pub kind: AuraPayloadKind,

    /// Payload bytes (format depends on kind).
    ///
    /// Limited to `PAYLOAD_BYTES_MAX` bytes.
    #[serde(with = "serde_bytes")]
    pub payload: Vec<u8>,
}

/// Discriminator for envelope payload types.
///
/// Values are stable and must not be reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum AuraPayloadKind {
    /// Invitation (contact, device, guardian, channel).
    Invite = 1,
    /// LAN discovery announcement.
    Discovery = 2,
    /// Rendezvous descriptor.
    RendezvousDescriptor = 3,
    // Future: HomeAnnouncement = 4, RelayAdvertisement = 5, etc.
}

impl AuraEnvelope {
    /// Create a new envelope with the current version.
    #[must_use]
    pub fn new(kind: AuraPayloadKind, payload: Vec<u8>) -> Self {
        debug_assert!(
            payload.len() <= PAYLOAD_BYTES_MAX,
            "payload exceeds {PAYLOAD_BYTES_MAX} bytes"
        );
        Self {
            version: ENVELOPE_VERSION_CURRENT,
            kind,
            payload,
        }
    }

    /// Create an invitation envelope.
    #[must_use]
    pub fn invite(payload: Vec<u8>) -> Self {
        Self::new(AuraPayloadKind::Invite, payload)
    }

    /// Create a discovery announcement envelope.
    #[must_use]
    pub fn discovery(payload: Vec<u8>) -> Self {
        Self::new(AuraPayloadKind::Discovery, payload)
    }

    /// Create a rendezvous descriptor envelope.
    #[must_use]
    pub fn rendezvous_descriptor(payload: Vec<u8>) -> Self {
        Self::new(AuraPayloadKind::RendezvousDescriptor, payload)
    }

    /// Encode to bytes for transport.
    ///
    /// Uses bincode for compact encoding. Infallible for valid envelopes.
    #[must_use]
    #[allow(clippy::expect_used)] // Serialization of this type is infallible
    pub fn encode(&self) -> Vec<u8> {
        // bincode serialization should not fail for these types
        bincode::serialize(self).expect("AuraEnvelope serialization is infallible")
    }

    /// Decode from bytes.
    ///
    /// # Errors
    ///
    /// Returns error string if:
    /// - Bytes are malformed
    /// - Payload exceeds `PAYLOAD_BYTES_MAX`
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let envelope: Self =
            bincode::deserialize(bytes).map_err(|e| format!("envelope decode: {e}"))?;

        if envelope.payload.len() > PAYLOAD_BYTES_MAX {
            return Err(format!(
                "payload exceeds {} bytes (got {})",
                PAYLOAD_BYTES_MAX,
                envelope.payload.len()
            ));
        }

        Ok(envelope)
    }

    /// Encode to human-shareable invite code string.
    ///
    /// Format: `aura:v{version}:{base64}`
    #[must_use]
    pub fn to_invite_code(&self) -> String {
        let bytes = self.encode();
        format!("aura:v{}:{}", self.version, BASE64.encode(&bytes))
    }

    /// Decode from invite code string.
    ///
    /// # Errors
    ///
    /// Returns error string if format is invalid or decode fails.
    pub fn from_invite_code(code: &str) -> Result<Self, String> {
        let code = code.trim();
        let code = code.strip_prefix("aura:").ok_or("missing 'aura:' prefix")?;

        let (version_part, payload_part) =
            code.split_once(':').ok_or("missing version delimiter")?;

        let _version: u8 = version_part
            .strip_prefix('v')
            .ok_or("missing 'v' in version")?
            .parse()
            .map_err(|_| "invalid version number")?;

        let bytes = BASE64
            .decode(payload_part)
            .map_err(|e| format!("base64 decode: {e}"))?;

        Self::decode(&bytes)
    }

    /// Check if this is an invitation envelope.
    #[must_use]
    pub fn is_invite(&self) -> bool {
        matches!(self.kind, AuraPayloadKind::Invite)
    }

    /// Check if this is a discovery envelope.
    #[must_use]
    pub fn is_discovery(&self) -> bool {
        matches!(self.kind, AuraPayloadKind::Discovery)
    }

    /// Check if this is a rendezvous descriptor envelope.
    #[must_use]
    pub fn is_rendezvous_descriptor(&self) -> bool {
        matches!(self.kind, AuraPayloadKind::RendezvousDescriptor)
    }

    /// Get the payload bytes.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Take ownership of the payload bytes.
    #[must_use]
    pub fn into_payload(self) -> Vec<u8> {
        self.payload
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)] // Tests use expect for simplicity
mod tests {
    use super::*;

    #[test]
    fn test_envelope_round_trip() {
        let original = AuraEnvelope::new(AuraPayloadKind::Invite, vec![1, 2, 3, 4]);
        let encoded = original.encode();
        let decoded = AuraEnvelope::decode(&encoded).expect("decode");
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_invite_code_round_trip() {
        let original = AuraEnvelope::new(AuraPayloadKind::Discovery, b"test payload".to_vec());
        let code = original.to_invite_code();
        assert!(code.starts_with("aura:v1:"));

        let decoded = AuraEnvelope::from_invite_code(&code).expect("decode");
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_invite_code_with_whitespace() {
        let original = AuraEnvelope::invite(b"test".to_vec());
        let code = original.to_invite_code();

        // Should handle leading/trailing whitespace
        let with_whitespace = format!("  {code}  \n");
        let decoded = AuraEnvelope::from_invite_code(&with_whitespace).expect("decode");
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_rejects_oversized_payload() {
        let huge = vec![0u8; PAYLOAD_BYTES_MAX + 1];
        // Direct construction bypasses limit (debug_assert only)
        // But decode should reject it
        let mut envelope = AuraEnvelope::new(AuraPayloadKind::Invite, vec![]);
        envelope.payload = huge;
        let bytes = envelope.encode();
        assert!(AuraEnvelope::decode(&bytes).is_err());
    }

    #[test]
    fn test_invalid_invite_code_formats() {
        // Missing prefix
        assert!(AuraEnvelope::from_invite_code("v1:abc").is_err());

        // Missing version delimiter
        assert!(AuraEnvelope::from_invite_code("aura:abc").is_err());

        // Invalid version
        assert!(AuraEnvelope::from_invite_code("aura:vX:abc").is_err());

        // Invalid base64
        assert!(AuraEnvelope::from_invite_code("aura:v1:!!!").is_err());
    }

    #[test]
    fn test_convenience_constructors() {
        let invite = AuraEnvelope::invite(vec![1, 2, 3]);
        assert!(invite.is_invite());
        assert!(!invite.is_discovery());
        assert!(!invite.is_rendezvous_descriptor());

        let discovery = AuraEnvelope::discovery(vec![4, 5, 6]);
        assert!(discovery.is_discovery());
        assert!(!discovery.is_invite());
        assert!(!discovery.is_rendezvous_descriptor());

        let rendezvous = AuraEnvelope::rendezvous_descriptor(vec![7, 8, 9]);
        assert!(rendezvous.is_rendezvous_descriptor());
        assert!(!rendezvous.is_invite());
        assert!(!rendezvous.is_discovery());
    }

    #[test]
    fn test_payload_access() {
        let envelope = AuraEnvelope::invite(vec![1, 2, 3, 4]);
        assert_eq!(envelope.payload(), &[1, 2, 3, 4]);

        let payload = envelope.into_payload();
        assert_eq!(payload, vec![1, 2, 3, 4]);
    }
}
