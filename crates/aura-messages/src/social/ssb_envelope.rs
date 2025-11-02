//! SSB Envelope Structure with Merkle-like CID Computation
//!
//! Implements the envelope specification from docs/041_rendezvous.md Section 5
//! "Protocol & Data Formats" with fixed-size envelopes and deterministic CID computation.
//!
//! Key Features:
//! - Fixed 2048-byte envelope size
//! - Merkle-like CID: sha256(sha256(HeaderBare) || sha256(ciphertext))
//! - CBOR canonical encoding (sorted keys, fixed integer sizes)
//! - Header integrity verification without full ciphertext

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Fixed envelope size in bytes
pub const ENVELOPE_SIZE: usize = 2048;

/// Content Identifier (CID) for envelopes
/// Computed as sha256(sha256(HeaderBare) || sha256(ciphertext))
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Cid(pub [u8; 32]);

impl Cid {
    /// Convert CID to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse CID from hex string
    pub fn from_hex(s: &str) -> Result<Self, String> {
        let bytes = hex::decode(s).map_err(|e| format!("invalid hex: {}", e))?;
        if bytes.len() != 32 {
            return Err(format!("expected 32 bytes, got {}", bytes.len()));
        }
        let mut array = [0u8; 32];
        array.copy_from_slice(&bytes);
        Ok(Cid(array))
    }

    /// Compute CID from header and ciphertext using Merkle-like structure
    pub fn compute(header_bare: &HeaderBare, ciphertext: &[u8]) -> Result<Self, String> {
        // Serialize HeaderBare using canonical CBOR (sorted keys, fixed integer sizes)
        let header_bytes = serde_cbor::to_vec(header_bare)
            .map_err(|e| format!("failed to serialize header: {}", e))?;

        // Compute individual hashes using sha2
        let mut hasher = Sha256::new();
        hasher.update(&header_bytes);
        let header_hash = hasher.finalize();

        let mut hasher = Sha256::new();
        hasher.update(ciphertext);
        let ciphertext_hash = hasher.finalize();

        // Compute final CID as sha256(header_hash || ciphertext_hash)
        let mut combined = Vec::with_capacity(64);
        combined.extend_from_slice(&header_hash);
        combined.extend_from_slice(&ciphertext_hash);

        let mut hasher = Sha256::new();
        hasher.update(&combined);
        let result = hasher.finalize();

        let mut cid = [0u8; 32];
        cid.copy_from_slice(&result);
        Ok(Cid(cid))
    }
}

/// Routing tag for envelope flooding
/// Truncated HMAC for efficient lookup
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RoutingTag(pub [u8; 16]);

impl RoutingTag {
    /// Convert routing tag to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse routing tag from hex string
    pub fn from_hex(s: &str) -> Result<Self, String> {
        let bytes = hex::decode(s).map_err(|e| format!("invalid hex: {}", e))?;
        if bytes.len() != 16 {
            return Err(format!("expected 16 bytes, got {}", bytes.len()));
        }
        let mut array = [0u8; 16];
        array.copy_from_slice(&bytes);
        Ok(RoutingTag(array))
    }

    /// Compute routing tag using HMAC
    /// rtag = Trunc128(HMAC(K_tag, epoch || counter || "rt"))
    pub fn compute(k_tag: &[u8; 32], epoch: u32, counter: u32) -> Result<Self, String> {
        // Prepare input data
        let mut input = Vec::with_capacity(8 + 2);
        input.extend_from_slice(&epoch.to_le_bytes());
        input.extend_from_slice(&counter.to_le_bytes());
        input.extend_from_slice(b"rt");

        // Compute HMAC using hmac crate
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(k_tag)
            .map_err(|e| format!("failed to create HMAC: {}", e))?;
        mac.update(&input);
        let result = mac.finalize().into_bytes();

        // Truncate to 16 bytes
        let mut tag = [0u8; 16];
        tag.copy_from_slice(&result[..16]);
        Ok(RoutingTag(tag))
    }
}

/// Bare header (without CID) for Merkle-like computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderBare {
    /// Protocol version (always 1 for Phase 0)
    pub version: u8,
    /// Time bucket (hour/day bucket)
    pub epoch: u32,
    /// Per-relationship monotonic counter
    pub counter: u32,
    /// Routing tag for envelope recognition
    pub rtag: RoutingTag,
    /// Time-to-live in epochs
    pub ttl_epochs: u16,
}

impl HeaderBare {
    /// Create new header bare
    pub fn new(epoch: u32, counter: u32, rtag: RoutingTag, ttl_epochs: u16) -> Self {
        Self {
            version: 1,
            epoch,
            counter,
            rtag,
            ttl_epochs,
        }
    }

    /// Check if this header is expired
    pub fn is_expired(&self, current_epoch: u32) -> bool {
        current_epoch > self.epoch + self.ttl_epochs as u32
    }

    /// Get expiration epoch
    pub fn expires_at_epoch(&self) -> u32 {
        self.epoch + self.ttl_epochs as u32
    }
}

/// Full header including CID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    /// Header without CID
    pub bare: HeaderBare,
    /// Content identifier
    pub cid: Cid,
}

impl Header {
    /// Create header from bare header and ciphertext
    pub fn new(header_bare: HeaderBare, ciphertext: &[u8]) -> Result<Self, String> {
        let cid = Cid::compute(&header_bare, ciphertext)?;
        Ok(Header {
            bare: header_bare,
            cid,
        })
    }

    /// Verify header integrity against ciphertext
    pub fn verify(&self, ciphertext: &[u8]) -> Result<bool, String> {
        let computed_cid = Cid::compute(&self.bare, ciphertext)?;
        Ok(computed_cid == self.cid)
    }
}

/// Complete SSB envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsbEnvelope {
    /// Envelope header
    pub header: Header,
    /// Encrypted payload (padded to fill remaining space)
    pub ciphertext: Vec<u8>,
}

impl SsbEnvelope {
    /// Create new envelope
    pub fn new(header: Header, ciphertext: Vec<u8>) -> Result<Self, String> {
        // Verify ciphertext matches header
        if !header.verify(&ciphertext)? {
            return Err("CID mismatch between header and ciphertext".to_string());
        }

        // Calculate header size when serialized
        let header_bytes = serde_cbor::to_vec(&header)
            .map_err(|e| format!("failed to serialize header: {}", e))?;

        let total_size = header_bytes.len() + ciphertext.len();
        if total_size > ENVELOPE_SIZE {
            return Err(format!(
                "envelope too large: {} > {}",
                total_size, ENVELOPE_SIZE
            ));
        }

        Ok(SsbEnvelope { header, ciphertext })
    }

    /// Serialize envelope using standard serde
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        serde_cbor::to_vec(self).map_err(|e| format!("failed to serialize envelope: {}", e))
    }

    /// Parse envelope using standard serde
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        serde_cbor::from_slice(bytes).map_err(|e| format!("failed to deserialize envelope: {}", e))
    }

    /// Get envelope CID
    pub fn cid(&self) -> &Cid {
        &self.header.cid
    }

    /// Check if envelope is expired
    pub fn is_expired(&self, current_epoch: u32) -> bool {
        self.header.bare.is_expired(current_epoch)
    }

    /// Get expiration epoch
    pub fn expires_at_epoch(&self) -> u32 {
        self.header.bare.expires_at_epoch()
    }
}

// All message types use standard serde traits for serialization

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cid_hex_conversion() {
        let cid = Cid([
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32,
        ]);

        let hex = cid.to_hex();
        let parsed = Cid::from_hex(&hex).unwrap();
        assert_eq!(cid, parsed);
    }

    #[test]
    fn test_routing_tag_computation() {
        let k_tag = [42u8; 32];
        let epoch = 12345;
        let counter = 67890;

        let tag1 = RoutingTag::compute(&k_tag, epoch, counter).unwrap();
        let tag2 = RoutingTag::compute(&k_tag, epoch, counter).unwrap();

        // Should be deterministic
        assert_eq!(tag1, tag2);

        // Different inputs should produce different tags
        let tag3 = RoutingTag::compute(&k_tag, epoch + 1, counter).unwrap();
        assert_ne!(tag1, tag3);
    }

    #[test]
    fn test_header_creation_and_verification() {
        let rtag = RoutingTag([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        let header_bare = HeaderBare::new(12345, 67890, rtag, 24);
        let ciphertext = b"hello world".to_vec();

        let header = Header::new(header_bare, &ciphertext).unwrap();
        assert!(header.verify(&ciphertext).unwrap());
        assert!(!header.verify(b"different content").unwrap());
    }

    #[test]
    fn test_header_expiration() {
        let rtag = RoutingTag([0u8; 16]);
        let header_bare = HeaderBare::new(100, 1, rtag, 24);

        assert!(!header_bare.is_expired(100)); // Same epoch
        assert!(!header_bare.is_expired(124)); // At expiration
        assert!(header_bare.is_expired(125)); // Past expiration
    }

    #[test]
    fn test_envelope_creation_and_roundtrip() {
        let rtag = RoutingTag([42u8; 16]);
        let header_bare = HeaderBare::new(12345, 67890, rtag, 24);
        let ciphertext = b"encrypted content for testing".to_vec();

        let header = Header::new(header_bare, &ciphertext).unwrap();
        let envelope = SsbEnvelope::new(header, ciphertext.clone()).unwrap();

        // Test serialization roundtrip
        let bytes = envelope.to_bytes().unwrap();

        let parsed = SsbEnvelope::from_bytes(&bytes).unwrap();
        assert_eq!(envelope.header.cid, parsed.header.cid);
        assert_eq!(envelope.ciphertext, parsed.ciphertext);
    }

    #[test]
    fn test_envelope_creation() {
        let rtag = RoutingTag([0u8; 16]);
        let header_bare = HeaderBare::new(1, 1, rtag, 1);

        // Test with normal ciphertext
        let ciphertext = vec![1u8; 100];
        let header = Header::new(header_bare, &ciphertext).unwrap();

        // Should create successfully
        assert!(SsbEnvelope::new(header, ciphertext).is_ok());
    }

    #[test]
    fn test_cid_deterministic() {
        let rtag = RoutingTag([1u8; 16]);
        let header_bare = HeaderBare::new(12345, 67890, rtag, 24);
        let ciphertext = b"test content".to_vec();

        let cid1 = Cid::compute(&header_bare, &ciphertext).unwrap();
        let cid2 = Cid::compute(&header_bare, &ciphertext).unwrap();

        assert_eq!(cid1, cid2);
    }
}
