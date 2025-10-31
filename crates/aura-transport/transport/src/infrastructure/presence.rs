// Presence tickets for authenticated peer connections
//
// Reference: 080_architecture_protocol_integration.md - Part 5: Presence Ticket Structure
//
// Presence tickets are short-lived credentials that devices use to authenticate with each other.
// They are signed with the account's threshold key and include the session epoch to enable
// automatic revocation when the account configuration changes.

use crate::{TransportErrorBuilder, TransportResult};
use aura_crypto::{signature_serde, Ed25519Signature};
use aura_journal::serialization::{from_cbor_bytes, to_cbor_bytes};
use ed25519_dalek::{Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
// Removed current_timestamp import - will use effects when integrated
// Note: This module should use effects-based time when integrated with the agent layer

/// Presence ticket - short-lived credential for peer authentication
///
/// Reference: 080 spec Part 5: Presence Ticket Structure
///
/// A presence ticket proves that:
/// 1. The device is part of the account (threshold signature)
/// 2. The device's membership is current (session epoch matches)
/// 3. The ticket hasn't expired (issued_at + ttl > now)
/// 4. The device hasn't been revoked (not in tombstone set)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceTicket {
    /// Device that owns this ticket
    pub device_id: Uuid,

    /// Account this device belongs to
    pub account_id: Uuid,

    /// Session epoch when this ticket was issued
    /// Tickets from old epochs are automatically invalid
    pub session_epoch: u64,

    /// When this ticket was issued (unix timestamp)
    pub issued_at: u64,

    /// When this ticket expires (unix timestamp)
    pub expires_at: u64,

    /// Capabilities granted by this ticket (for future use)
    pub capabilities: Vec<String>,

    /// Threshold signature over the ticket
    /// Signed by M-of-N devices using FROST
    #[serde(with = "signature_serde")]
    pub signature: Ed25519Signature,
}

impl PresenceTicket {
    /// Create a new presence ticket (unsigned)
    ///
    /// The caller must sign this ticket using FROST threshold signing
    pub fn new(
        device_id: Uuid,
        account_id: Uuid,
        session_epoch: u64,
        ttl_seconds: u64,
    ) -> TransportResult<Self> {
        #[allow(clippy::disallowed_methods)]
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| TransportErrorBuilder::invalid_presence_ticket())?
            .as_secs();

        Ok(PresenceTicket {
            device_id,
            account_id,
            session_epoch,
            issued_at: now,
            expires_at: now + ttl_seconds,
            capabilities: vec!["read".to_string(), "write".to_string()],
            // Signature must be filled in by caller
            signature: Ed25519Signature::default(),
        })
    }

    /// Compute the hash that should be signed
    ///
    /// This is what gets signed by the threshold key
    pub fn compute_signable_hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();

        hasher.update(self.device_id.as_bytes());
        hasher.update(self.account_id.as_bytes());
        hasher.update(&self.session_epoch.to_le_bytes());
        hasher.update(&self.issued_at.to_le_bytes());
        hasher.update(&self.expires_at.to_le_bytes());

        for cap in &self.capabilities {
            hasher.update(cap.as_bytes());
        }

        *hasher.finalize().as_bytes()
    }

    /// Verify this ticket
    ///
    /// Checks:
    /// 1. Signature is valid (threshold signature)
    /// 2. Ticket hasn't expired
    /// 3. Session epoch matches expected
    ///
    /// Reference: 080 spec Part 5: Transport Handshake Specification
    pub fn verify(
        &self,
        group_public_key: &VerifyingKey,
        current_epoch: u64,
    ) -> TransportResult<()> {
        #[allow(clippy::disallowed_methods)]
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| TransportErrorBuilder::invalid_presence_ticket())?
            .as_secs();

        // Check expiry
        if now > self.expires_at {
            return Err(TransportErrorBuilder::invalid_presence_ticket());
        }

        // Check session epoch
        if self.session_epoch != current_epoch {
            return Err(TransportErrorBuilder::invalid_presence_ticket());
        }

        // Verify signature
        let hash = self.compute_signable_hash();
        group_public_key
            .verify(&hash, &self.signature.0)
            .map_err(|_| TransportErrorBuilder::invalid_presence_ticket())?;

        Ok(())
    }

    /// Check if ticket is expired (without full verification)
    /// Note: This should be updated to use effects for deterministic testing
    pub fn is_expired(&self) -> TransportResult<bool> {
        #[allow(clippy::disallowed_methods)]
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| TransportErrorBuilder::invalid_presence_ticket())?
            .as_secs();
        Ok(now > self.expires_at)
    }

    /// Check if ticket matches expected session epoch (without full verification)
    pub fn has_epoch(&self, expected_epoch: u64) -> bool {
        self.session_epoch == expected_epoch
    }

    /// Serialize to bytes for transport
    pub fn to_bytes(&self) -> TransportResult<Vec<u8>> {
        to_cbor_bytes(self)
            .map_err(|e| TransportErrorBuilder::transport(format!("Serialization failed: {}", e)))
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> TransportResult<Self> {
        from_cbor_bytes(bytes)
            .map_err(|e| TransportErrorBuilder::transport(format!("Deserialization failed: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn test_ticket_creation() {
        let ticket = PresenceTicket::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            3600, // 1 hour TTL
        )
        .unwrap();

        assert_eq!(ticket.session_epoch, 1);
        assert!(ticket.expires_at > ticket.issued_at);
        assert_eq!(ticket.capabilities.len(), 2);
    }

    #[test]
    fn test_ticket_expiry_check() {
        let mut ticket = PresenceTicket::new(Uuid::new_v4(), Uuid::new_v4(), 1, 3600).unwrap();

        // Ticket should not be expired
        assert!(!ticket.is_expired().unwrap());

        // Force expiry
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        ticket.expires_at = now - 100;
        assert!(ticket.is_expired().unwrap());
    }

    #[test]
    fn test_ticket_epoch_check() {
        let ticket = PresenceTicket::new(Uuid::new_v4(), Uuid::new_v4(), 5, 3600).unwrap();

        assert!(ticket.has_epoch(5));
        assert!(!ticket.has_epoch(4));
        assert!(!ticket.has_epoch(6));
    }

    #[test]
    fn test_ticket_hash_computation() {
        let ticket = PresenceTicket::new(Uuid::new_v4(), Uuid::new_v4(), 1, 3600).unwrap();

        let hash1 = ticket.compute_signable_hash();
        let hash2 = ticket.compute_signable_hash();

        // Hash should be deterministic
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 32);
    }

    #[test]
    fn test_ticket_serialization() {
        // Test serialization of all fields including signature
        let ticket = PresenceTicket::new(Uuid::new_v4(), Uuid::new_v4(), 1, 3600).unwrap();

        // Test direct serde_cbor serialization/deserialization to match production usage
        let cbor_bytes = serde_cbor::to_vec(&ticket).expect("CBOR serialization should work");
        let restored: PresenceTicket =
            serde_cbor::from_slice(&cbor_bytes).expect("CBOR deserialization should work");

        assert_eq!(ticket.device_id, restored.device_id);
        assert_eq!(ticket.account_id, restored.account_id);
        assert_eq!(ticket.session_epoch, restored.session_epoch);
        assert_eq!(ticket.issued_at, restored.issued_at);
        assert_eq!(ticket.expires_at, restored.expires_at);
        assert_eq!(ticket.capabilities, restored.capabilities);
        // Signature comparison now works with Default implementation
        assert_eq!(
            ticket.signature.0.to_bytes(),
            restored.signature.0.to_bytes()
        );
    }

    #[test]
    fn test_ticket_verification() {
        let effects = aura_crypto::Effects::for_test("test_ticket");
        let random_bytes: [u8; 32] = effects.random_bytes();
        let signing_key = SigningKey::from_bytes(&random_bytes);
        let verifying_key = signing_key.verifying_key();

        let mut ticket = PresenceTicket::new(Uuid::new_v4(), Uuid::new_v4(), 1, 3600).unwrap();

        // Sign the ticket
        let hash = ticket.compute_signable_hash();
        ticket.signature = Ed25519Signature(signing_key.sign(&hash));

        // Verification should succeed
        let result = ticket.verify(&verifying_key, 1);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ticket_verification_wrong_epoch() {
        let effects = aura_crypto::Effects::for_test("test_ticket");
        let random_bytes: [u8; 32] = effects.random_bytes();
        let signing_key = SigningKey::from_bytes(&random_bytes);
        let verifying_key = signing_key.verifying_key();

        let mut ticket = PresenceTicket::new(Uuid::new_v4(), Uuid::new_v4(), 1, 3600).unwrap();

        let hash = ticket.compute_signable_hash();
        ticket.signature = Ed25519Signature(signing_key.sign(&hash));

        // Verification with wrong epoch should fail
        let result = ticket.verify(&verifying_key, 2);
        assert!(result.is_err());
    }

    #[test]
    fn test_ticket_verification_expired() {
        let effects = aura_crypto::Effects::for_test("test_ticket");
        let random_bytes: [u8; 32] = effects.random_bytes();
        let signing_key = SigningKey::from_bytes(&random_bytes);
        let verifying_key = signing_key.verifying_key();

        let mut ticket = PresenceTicket::new(Uuid::new_v4(), Uuid::new_v4(), 1, 3600).unwrap();

        // Force expiry
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        ticket.expires_at = now - 100;

        let hash = ticket.compute_signable_hash();
        ticket.signature = Ed25519Signature(signing_key.sign(&hash));

        // Verification should fail (expired)
        let result = ticket.verify(&verifying_key, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_ticket_verification_invalid_signature() {
        let signing_key1 = SigningKey::from_bytes(&rand::random());
        let signing_key2 = SigningKey::from_bytes(&rand::random());
        let verifying_key2 = signing_key2.verifying_key();

        let mut ticket = PresenceTicket::new(Uuid::new_v4(), Uuid::new_v4(), 1, 3600).unwrap();

        // Sign with key1
        let hash = ticket.compute_signable_hash();
        ticket.signature = Ed25519Signature(signing_key1.sign(&hash));

        // Try to verify with key2 - should fail
        let result = ticket.verify(&verifying_key2, 1);
        assert!(result.is_err());
    }
}
