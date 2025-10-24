//! SSB Envelope Publishing
//!
//! Implements envelope publishing as specified in docs/051_rendezvous_ssb.md Section 4.2
//! "CRDT-based Publishing & Recognition".
//!
//! Publishing Flow:
//! 1. Reserve counter (coordination with other devices)
//! 2. Compute routing tag: Trunc128(HMAC(K_tag, epoch || counter || "rt"))
//! 3. Encrypt payload with K_box (XChaCha20-Poly1305)
//! 4. Create envelope with computed CID
//! 5. Add to local CRDT (will gossip to neighbors)

use crate::envelope::{Envelope, Header, HeaderBare, RoutingTag, ENVELOPE_SIZE};
use crate::{Result, TransportError};
use blake3::Hasher;
use serde::{Deserialize, Serialize};

/// Payload to be published via SSB
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopePayload {
    /// Content type identifier
    pub content_type: String,

    /// Actual payload data
    pub data: Vec<u8>,

    /// Optional metadata
    pub metadata: Vec<u8>,
}

/// Published envelope with metadata
#[derive(Debug, Clone)]
pub struct PublishedEnvelope {
    /// The envelope itself
    pub envelope: Envelope,

    /// Relationship ID this was published to
    pub relationship_id: Vec<u8>,

    /// Publishing timestamp
    pub published_at: u64,
}

/// SSB Envelope Publisher
pub struct SbbPublisher {
    /// Current session epoch
    epoch: u64,
}

impl SbbPublisher {
    /// Create a new SSB publisher
    pub fn new(epoch: u64) -> Self {
        SbbPublisher { epoch }
    }

    /// Update current epoch
    pub fn set_epoch(&mut self, epoch: u64) {
        self.epoch = epoch;
    }

    /// Compute routing tag: Trunc128(HMAC(K_tag, epoch || counter || "rt"))
    ///
    /// The routing tag is a truncated BLAKE3 keyed hash that allows recipients
    /// to efficiently check if an envelope might be for them without decryption.
    pub fn compute_routing_tag(k_tag: &[u8; 32], epoch: u64, counter: u64) -> Result<RoutingTag> {
        // Build the input: epoch || counter || "rt"
        let mut input = Vec::new();
        input.extend_from_slice(&epoch.to_le_bytes());
        input.extend_from_slice(&counter.to_le_bytes());
        input.extend_from_slice(b"rt");

        // BLAKE3 keyed hash (acts like HMAC)
        let mut hasher = Hasher::new_keyed(k_tag);
        hasher.update(&input);
        let hash = hasher.finalize();

        // Truncate to 128 bits (16 bytes)
        let mut rtag = [0u8; 16];
        rtag.copy_from_slice(&hash.as_bytes()[..16]);

        Ok(RoutingTag(rtag))
    }

    /// Encrypt payload using K_box (XChaCha20-Poly1305)
    ///
    /// Note: This is a placeholder implementation. In production, this would use
    /// proper XChaCha20-Poly1305 AEAD encryption.
    pub fn encrypt_payload(
        k_box: &[u8; 32],
        payload: &EnvelopePayload,
        nonce: &[u8; 24],
    ) -> Result<Vec<u8>> {
        // Serialize payload
        let serialized = bincode::serialize(payload)
            .map_err(|e| TransportError::Transport(format!("serialization failed: {}", e)))?;

        // TODO: Replace with actual XChaCha20-Poly1305 encryption
        // For now, just return serialized data (INSECURE - for structure only)
        // Real implementation would use chacha20poly1305 crate

        // Placeholder: XOR with key for basic obfuscation in tests
        let mut encrypted = serialized.clone();
        for (i, byte) in encrypted.iter_mut().enumerate() {
            *byte ^= k_box[i % 32];
        }

        Ok(encrypted)
    }

    /// Decrypt payload (inverse of encrypt_payload)
    pub fn decrypt_payload(
        k_box: &[u8; 32],
        ciphertext: &[u8],
        _nonce: &[u8; 24],
    ) -> Result<EnvelopePayload> {
        // TODO: Replace with actual XChaCha20-Poly1305 decryption

        // Placeholder: XOR with key (inverse of encryption)
        let mut decrypted = ciphertext.to_vec();
        for (i, byte) in decrypted.iter_mut().enumerate() {
            *byte ^= k_box[i % 32];
        }

        bincode::deserialize(&decrypted)
            .map_err(|e| TransportError::Transport(format!("deserialization failed: {}", e)))
    }

    /// Publish an envelope to the SSB
    ///
    /// This coordinates counter reservation → encryption → envelope creation.
    /// The envelope is added to the local CRDT which will gossip to neighbors.
    pub fn publish_envelope(
        &self,
        payload: EnvelopePayload,
        k_box: &[u8; 32],
        k_tag: &[u8; 32],
        counter: u64,
        ttl_epochs: u16,
        relationship_id: Vec<u8>,
        published_at: u64,
    ) -> Result<PublishedEnvelope> {
        // 1. Compute routing tag
        let rtag = Self::compute_routing_tag(k_tag, self.epoch, counter)?;

        // 2. Encrypt payload
        // Generate nonce from epoch and counter (deterministic for testing)
        let mut nonce = [0u8; 24];
        nonce[..8].copy_from_slice(&self.epoch.to_le_bytes());
        nonce[8..16].copy_from_slice(&counter.to_le_bytes());

        let ciphertext = Self::encrypt_payload(k_box, &payload, &nonce)?;

        // 3. Verify size constraints
        // The ciphertext must fit within ENVELOPE_SIZE minus header overhead
        // Rough estimate: header ~150 bytes, so max ciphertext ~1890 bytes
        const MAX_CIPHERTEXT_SIZE: usize = 1800; // Conservative estimate
        if ciphertext.len() > MAX_CIPHERTEXT_SIZE {
            return Err(TransportError::Transport(format!(
                "ciphertext too large: {} bytes (max {})",
                ciphertext.len(),
                MAX_CIPHERTEXT_SIZE
            )));
        }

        // 4. Create envelope header
        let bare = HeaderBare::new(self.epoch, counter, rtag, ttl_epochs);
        let header = Header::new(bare, &ciphertext)
            .map_err(|e| TransportError::Transport(format!("header creation failed: {}", e)))?;

        // 5. Create complete envelope
        let envelope = Envelope::new(header, ciphertext)
            .map_err(|e| TransportError::Transport(format!("envelope creation failed: {}", e)))?;

        // 6. Verify envelope size
        let envelope_bytes = envelope.to_bytes().map_err(|e| {
            TransportError::Transport(format!("envelope serialization failed: {}", e))
        })?;

        if envelope_bytes.len() != ENVELOPE_SIZE {
            return Err(TransportError::Transport(format!(
                "envelope size incorrect: {} bytes (expected {})",
                envelope_bytes.len(),
                ENVELOPE_SIZE
            )));
        }

        Ok(PublishedEnvelope {
            envelope,
            relationship_id,
            published_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routing_tag_computation() {
        let k_tag = [0x42; 32];
        let epoch = 100;
        let counter = 42;

        let rtag1 = SbbPublisher::compute_routing_tag(&k_tag, epoch, counter).unwrap();
        let rtag2 = SbbPublisher::compute_routing_tag(&k_tag, epoch, counter).unwrap();

        // Should be deterministic
        assert_eq!(rtag1, rtag2);

        // Different epoch should produce different tag
        let rtag3 = SbbPublisher::compute_routing_tag(&k_tag, epoch + 1, counter).unwrap();
        assert_ne!(rtag1, rtag3);

        // Different counter should produce different tag
        let rtag4 = SbbPublisher::compute_routing_tag(&k_tag, epoch, counter + 1).unwrap();
        assert_ne!(rtag1, rtag4);
    }

    #[test]
    fn test_payload_encryption_decryption() {
        let k_box = [0x42; 32];
        let nonce = [0x12; 24];

        let payload = EnvelopePayload {
            content_type: "text/plain".to_string(),
            data: b"Hello, SSB!".to_vec(),
            metadata: vec![],
        };

        let encrypted = SbbPublisher::encrypt_payload(&k_box, &payload, &nonce).unwrap();
        let decrypted = SbbPublisher::decrypt_payload(&k_box, &encrypted, &nonce).unwrap();

        assert_eq!(payload.content_type, decrypted.content_type);
        assert_eq!(payload.data, decrypted.data);
        assert_eq!(payload.metadata, decrypted.metadata);
    }

    #[test]
    fn test_publish_envelope() {
        let publisher = SbbPublisher::new(100);

        let payload = EnvelopePayload {
            content_type: "text/plain".to_string(),
            data: b"Test message".to_vec(),
            metadata: vec![],
        };

        let k_box = [0x42; 32];
        let k_tag = [0x43; 32];
        let counter = 42;
        let ttl_epochs = 10;
        let relationship_id = vec![1, 2, 3, 4];
        let published_at = 1000;

        let published = publisher
            .publish_envelope(
                payload,
                &k_box,
                &k_tag,
                counter,
                ttl_epochs,
                relationship_id.clone(),
                published_at,
            )
            .unwrap();

        // Verify envelope properties
        assert_eq!(published.envelope.header.bare.epoch, 100);
        assert_eq!(published.envelope.header.bare.counter, counter);
        assert_eq!(published.envelope.header.bare.ttl_epochs, ttl_epochs);
        assert_eq!(published.relationship_id, relationship_id);
        assert_eq!(published.published_at, published_at);

        // Verify envelope serializes to correct size
        let bytes = published.envelope.to_bytes().unwrap();
        assert_eq!(bytes.len(), ENVELOPE_SIZE);
    }

    #[test]
    fn test_publish_with_large_payload() {
        let publisher = SbbPublisher::new(100);

        // Create payload that's too large
        let payload = EnvelopePayload {
            content_type: "application/octet-stream".to_string(),
            data: vec![0x42; 2000], // Too large
            metadata: vec![],
        };

        let k_box = [0x42; 32];
        let k_tag = [0x43; 32];

        let result =
            publisher.publish_envelope(payload, &k_box, &k_tag, 42, 10, vec![1, 2, 3, 4], 1000);

        // Should fail due to size constraint
        assert!(result.is_err());
    }

    #[test]
    fn test_epoch_update() {
        let mut publisher = SbbPublisher::new(100);

        let payload = EnvelopePayload {
            content_type: "text/plain".to_string(),
            data: b"Test".to_vec(),
            metadata: vec![],
        };

        let k_box = [0x42; 32];
        let k_tag = [0x43; 32];

        let published1 = publisher
            .publish_envelope(
                payload.clone(),
                &k_box,
                &k_tag,
                42,
                10,
                vec![1, 2, 3, 4],
                1000,
            )
            .unwrap();

        assert_eq!(published1.envelope.header.bare.epoch, 100);

        // Update epoch
        publisher.set_epoch(200);

        let published2 = publisher
            .publish_envelope(payload, &k_box, &k_tag, 43, 10, vec![1, 2, 3, 4], 2000)
            .unwrap();

        assert_eq!(published2.envelope.header.bare.epoch, 200);

        // Routing tags should be different due to different epoch
        assert_ne!(
            published1.envelope.header.bare.rtag,
            published2.envelope.header.bare.rtag
        );
    }
}
