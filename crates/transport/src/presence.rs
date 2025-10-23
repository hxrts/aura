// Presence tickets for authenticated peer connections
//
// Reference: 080_architecture_protocol_integration.md - Part 5: Presence Ticket Structure
//
// Presence tickets are short-lived credentials that devices use to authenticate with each other.
// They are signed with the account's threshold key and include the session epoch to enable
// automatic revocation when the account configuration changes.

use crate::{TransportError, Result};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    pub signature: Signature,
}

mod signature_serde {
    use ed25519_dalek::Signature;
    use serde::{Deserializer, Serializer};
    
    pub fn serialize<S>(sig: &Signature, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&sig.to_bytes())
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Signature, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Handle both byte arrays and sequences
        struct SignatureVisitor;
        
        impl<'de> serde::de::Visitor<'de> for SignatureVisitor {
            type Value = Vec<u8>;
            
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a byte array or sequence of bytes")
            }
            
            fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(value.to_vec())
            }
            
            fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(value)
            }
            
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut bytes = Vec::new();
                while let Some(byte) = seq.next_element()? {
                    bytes.push(byte);
                }
                Ok(bytes)
            }
        }
        
        let bytes = deserializer.deserialize_any(SignatureVisitor)?;
        Signature::from_slice(&bytes).map_err(serde::de::Error::custom)
    }
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
    ) -> Result<Self> {
        let now = current_timestamp()?;
        
        Ok(PresenceTicket {
            device_id,
            account_id,
            session_epoch,
            issued_at: now,
            expires_at: now + ttl_seconds,
            capabilities: vec!["read".to_string(), "write".to_string()],
            // Signature must be filled in by caller
            signature: Signature::from_bytes(&[0u8; 64]),
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
    ) -> Result<()> {
        let now = current_timestamp()?;
        
        // Check expiry
        if now > self.expires_at {
            return Err(TransportError::InvalidPresenceTicket);
        }
        
        // Check session epoch
        if self.session_epoch != current_epoch {
            return Err(TransportError::InvalidPresenceTicket);
        }
        
        // Verify signature
        let hash = self.compute_signable_hash();
        group_public_key
            .verify(&hash, &self.signature)
            .map_err(|_| TransportError::InvalidPresenceTicket)?;
        
        Ok(())
    }
    
    /// Check if ticket is expired (without full verification)
    pub fn is_expired(&self) -> Result<bool> {
        Ok(current_timestamp()? > self.expires_at)
    }
    
    /// Check if ticket matches expected session epoch (without full verification)
    pub fn has_epoch(&self, expected_epoch: u64) -> bool {
        self.session_epoch == expected_epoch
    }
    
    /// Serialize to bytes for transport
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        serde_cbor::to_vec(self)
            .map_err(|e| TransportError::Transport(format!("Serialization failed: {}", e)))
    }
    
    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        serde_cbor::from_slice(bytes)
            .map_err(|e| TransportError::Transport(format!("Deserialization failed: {}", e)))
    }
}

/// Get current unix timestamp in seconds
fn current_timestamp() -> Result<u64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| TransportError::Transport(format!(
            "System time is before UNIX epoch: {}",
            e
        )))
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
        ).unwrap();
        
        assert_eq!(ticket.session_epoch, 1);
        assert!(ticket.expires_at > ticket.issued_at);
        assert_eq!(ticket.capabilities.len(), 2);
    }
    
    #[test]
    fn test_ticket_expiry_check() {
        let mut ticket = PresenceTicket::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            3600,
        ).unwrap();
        
        // Ticket should not be expired
        assert!(!ticket.is_expired().unwrap());
        
        // Force expiry
        ticket.expires_at = current_timestamp().unwrap() - 100;
        assert!(ticket.is_expired().unwrap());
    }
    
    #[test]
    fn test_ticket_epoch_check() {
        let ticket = PresenceTicket::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            5,
            3600,
        ).unwrap();
        
        assert!(ticket.has_epoch(5));
        assert!(!ticket.has_epoch(4));
        assert!(!ticket.has_epoch(6));
    }
    
    #[test]
    fn test_ticket_hash_computation() {
        let ticket = PresenceTicket::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            3600,
        ).unwrap();
        
        let hash1 = ticket.compute_signable_hash();
        let hash2 = ticket.compute_signable_hash();
        
        // Hash should be deterministic
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 32);
    }
    
    #[test]
    fn test_ticket_serialization() {
        let ticket = PresenceTicket::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            3600,
        ).unwrap();
        
        let bytes = ticket.to_bytes().unwrap();
        let restored = PresenceTicket::from_bytes(&bytes).unwrap();
        
        assert_eq!(ticket.device_id, restored.device_id);
        assert_eq!(ticket.account_id, restored.account_id);
        assert_eq!(ticket.session_epoch, restored.session_epoch);
    }
    
    #[test]
    fn test_ticket_verification() {
        let signing_key = SigningKey::from_bytes(&rand::random());
        let verifying_key = signing_key.verifying_key();
        
        let mut ticket = PresenceTicket::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            3600,
        ).unwrap();
        
        // Sign the ticket
        let hash = ticket.compute_signable_hash();
        ticket.signature = signing_key.sign(&hash);
        
        // Verification should succeed
        let result = ticket.verify(&verifying_key, 1);
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_ticket_verification_wrong_epoch() {
        let signing_key = SigningKey::from_bytes(&rand::random());
        let verifying_key = signing_key.verifying_key();
        
        let mut ticket = PresenceTicket::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            3600,
        ).unwrap();
        
        let hash = ticket.compute_signable_hash();
        ticket.signature = signing_key.sign(&hash);
        
        // Verification with wrong epoch should fail
        let result = ticket.verify(&verifying_key, 2);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_ticket_verification_expired() {
        let signing_key = SigningKey::from_bytes(&rand::random());
        let verifying_key = signing_key.verifying_key();
        
        let mut ticket = PresenceTicket::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            3600,
        ).unwrap();
        
        // Force expiry
        ticket.expires_at = current_timestamp().unwrap() - 100;
        
        let hash = ticket.compute_signable_hash();
        ticket.signature = signing_key.sign(&hash);
        
        // Verification should fail (expired)
        let result = ticket.verify(&verifying_key, 1);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_ticket_verification_invalid_signature() {
        let signing_key1 = SigningKey::from_bytes(&rand::random());
        let signing_key2 = SigningKey::from_bytes(&rand::random());
        let verifying_key2 = signing_key2.verifying_key();
        
        let mut ticket = PresenceTicket::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            3600,
        ).unwrap();
        
        // Sign with key1
        let hash = ticket.compute_signable_hash();
        ticket.signature = signing_key1.sign(&hash);
        
        // Try to verify with key2 - should fail
        let result = ticket.verify(&verifying_key2, 1);
        assert!(result.is_err());
    }
}

