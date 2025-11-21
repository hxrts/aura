//! SBB Envelope Types
//!
//! Content-addressed envelopes for Social Bulletin Board flooding.

use crate::crypto::encryption::EncryptedEnvelope;
use aura_core::hash::hasher;
use serde::{Deserialize, Serialize};

/// Content-addressed envelope ID (Blake3 hash)
pub type EnvelopeId = [u8; 32];

/// SBB message size for flow budget calculations
pub const SBB_MESSAGE_SIZE: u64 = 1024; // 1KB standard envelope size

/// Rendezvous envelope for peer discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendezvousEnvelope {
    /// Content-addressed envelope ID (Blake3 hash of payload)
    pub id: EnvelopeId,
    /// Time-to-live for flooding (max hops)
    pub ttl: u8,
    /// Creation timestamp (for cache expiration)
    pub created_at: u64,
    /// Transport offer payload (encrypted or plaintext for backward compatibility)
    pub payload: Vec<u8>,
}

/// Enhanced SBB envelope supporting both plaintext and encrypted variants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SbbEnvelope {
    /// Plaintext envelope (for backward compatibility and testing)
    Plaintext(RendezvousEnvelope),
    /// Encrypted envelope with relationship-based encryption
    Encrypted {
        /// Content-addressed envelope ID
        id: EnvelopeId,
        /// Time-to-live for flooding
        ttl: u8,
        /// Creation timestamp
        created_at: u64,
        /// Encrypted payload with padding
        encrypted_payload: EncryptedEnvelope,
    },
}

impl RendezvousEnvelope {
    /// Create new rendezvous envelope with content-addressed ID
    pub fn new(payload: Vec<u8>, ttl: Option<u8>) -> Self {
        let id = Self::compute_envelope_id(&payload);
        let ttl = ttl.unwrap_or(6); // Default 6 hops for friend networks
        let created_at = super::current_timestamp();

        Self {
            id,
            ttl,
            created_at,
            payload,
        }
    }

    /// Compute content-addressed envelope ID using Blake3
    fn compute_envelope_id(payload: &[u8]) -> EnvelopeId {
        let mut h = hasher();
        h.update(b"aura-sbb-envelope-v1");
        h.update(payload);
        h.finalize()
    }

    /// Decrement TTL for next hop
    pub fn decrement_ttl(mut self) -> Option<Self> {
        if self.ttl > 0 {
            self.ttl -= 1;
            Some(self)
        } else {
            None
        }
    }

    /// Check if envelope has expired based on creation time
    pub fn is_expired(&self, current_time: u64, max_age_seconds: u64) -> bool {
        current_time > self.created_at + max_age_seconds
    }
}

impl SbbEnvelope {
    /// Create new plaintext SBB envelope
    pub fn new_plaintext(payload: Vec<u8>, ttl: Option<u8>) -> Self {
        let envelope = RendezvousEnvelope::new(payload, ttl);
        SbbEnvelope::Plaintext(envelope)
    }

    /// Create new encrypted SBB envelope
    pub fn new_encrypted(encrypted_payload: EncryptedEnvelope, ttl: Option<u8>) -> Self {
        let ttl = ttl.unwrap_or(6);
        let created_at = super::current_timestamp();

        // Compute ID from encrypted payload for deduplication
        let id = Self::compute_encrypted_envelope_id(&encrypted_payload);

        SbbEnvelope::Encrypted {
            id,
            ttl,
            created_at,
            encrypted_payload,
        }
    }

    /// Get envelope ID for deduplication
    pub fn id(&self) -> EnvelopeId {
        match self {
            SbbEnvelope::Plaintext(envelope) => envelope.id,
            SbbEnvelope::Encrypted { id, .. } => *id,
        }
    }

    /// Get TTL for flooding control
    pub fn ttl(&self) -> u8 {
        match self {
            SbbEnvelope::Plaintext(envelope) => envelope.ttl,
            SbbEnvelope::Encrypted { ttl, .. } => *ttl,
        }
    }

    /// Get creation timestamp
    pub fn created_at(&self) -> u64 {
        match self {
            SbbEnvelope::Plaintext(envelope) => envelope.created_at,
            SbbEnvelope::Encrypted { created_at, .. } => *created_at,
        }
    }

    /// Decrement TTL for next hop
    pub fn decrement_ttl(self) -> Option<Self> {
        match self {
            SbbEnvelope::Plaintext(envelope) => {
                envelope.decrement_ttl().map(SbbEnvelope::Plaintext)
            }
            SbbEnvelope::Encrypted {
                id,
                ttl,
                created_at,
                encrypted_payload,
            } => {
                if ttl > 0 {
                    Some(SbbEnvelope::Encrypted {
                        id,
                        ttl: ttl - 1,
                        created_at,
                        encrypted_payload,
                    })
                } else {
                    None
                }
            }
        }
    }

    /// Check if envelope has expired
    pub fn is_expired(&self, current_time: u64, max_age_seconds: u64) -> bool {
        current_time > self.created_at() + max_age_seconds
    }

    /// Get envelope size for flow budget calculations
    pub fn size(&self) -> usize {
        match self {
            SbbEnvelope::Plaintext(envelope) => {
                32 + 1 + 8 + envelope.payload.len() // id + ttl + created_at + payload
            }
            SbbEnvelope::Encrypted {
                encrypted_payload, ..
            } => {
                32 + 1 + 8 + encrypted_payload.size() // id + ttl + created_at + encrypted_payload
            }
        }
    }

    /// Compute content-addressed ID for encrypted envelope
    fn compute_encrypted_envelope_id(encrypted_payload: &EncryptedEnvelope) -> EnvelopeId {
        let mut h = hasher();
        h.update(b"aura-sbb-encrypted-envelope-v1");
        h.update(&encrypted_payload.nonce);
        h.update(&encrypted_payload.ciphertext);
        if let Some(hint) = &encrypted_payload.key_hint {
            h.update(hint);
        }

        let hash = h.finalize();
        let mut id = [0u8; 32];
        id.copy_from_slice(&hash);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envelope_creation() {
        let payload = b"test rendezvous offer".to_vec();
        let envelope = RendezvousEnvelope::new(payload.clone(), Some(4));

        assert_eq!(envelope.ttl, 4);
        assert_eq!(envelope.payload, payload);
        assert_ne!(envelope.id, [0u8; 32]);
    }

    #[test]
    fn test_content_addressed_id() {
        let payload = b"identical payload".to_vec();
        let envelope1 = RendezvousEnvelope::new(payload.clone(), None);
        let envelope2 = RendezvousEnvelope::new(payload, None);

        assert_eq!(envelope1.id, envelope2.id);
    }

    #[test]
    fn test_ttl_decrement() {
        let payload = b"test".to_vec();
        let envelope = RendezvousEnvelope::new(payload, Some(2));

        let decremented = envelope.decrement_ttl().unwrap();
        assert_eq!(decremented.ttl, 1);

        let final_envelope = decremented.decrement_ttl().unwrap();
        assert_eq!(final_envelope.ttl, 0);

        assert!(final_envelope.decrement_ttl().is_none());
    }

    #[test]
    fn test_sbb_envelope_variants() {
        let payload = b"test".to_vec();
        let plaintext_env = SbbEnvelope::new_plaintext(payload, Some(4));
        assert_eq!(plaintext_env.ttl(), 4);

        let encrypted_payload = crate::crypto::encryption::EncryptedEnvelope::new(
            [1; 12],
            vec![0; 1024],
            Some([1, 2, 3, 4]),
        );
        let encrypted_env = SbbEnvelope::new_encrypted(encrypted_payload, Some(3));
        assert_eq!(encrypted_env.ttl(), 3);
    }
}
