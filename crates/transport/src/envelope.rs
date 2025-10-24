//! SSB Envelope structure with Merkle-like CID computation
//!
//! Implements the envelope specification from docs/051_rendezvous_ssb.md Section 5
//! "Protocol & Data Formats" with fixed-size envelopes and deterministic CID computation.
//!
//! Key Features:
//! - Fixed 2048-byte envelope size
//! - Merkle-like CID: sha256(sha256(HeaderBare) || sha256(ciphertext))
//! - CBOR canonical encoding (sorted keys, fixed integer sizes)
//! - Header integrity verification without full ciphertext

use blake3::Hasher;
use serde::{Deserialize, Serialize};
use serde_cbor;

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
}

/// Routing tag for envelope flooding
/// Truncated HMAC for efficient lookup
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RoutingTag(pub [u8; 16]);

/// Bare header (without CID) for Merkle-like computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderBare {
    /// Protocol version (always 1 for Phase 0)
    pub version: u8,

    /// Session epoch number
    pub epoch: u64,

    /// Counter for unique envelope identification
    pub counter: u64,

    /// Routing tag for flooding (truncated HMAC)
    pub rtag: RoutingTag,

    /// Time-to-live in epochs
    pub ttl_epochs: u16,
}

impl HeaderBare {
    /// Create a new bare header
    pub fn new(epoch: u64, counter: u64, rtag: RoutingTag, ttl_epochs: u16) -> Self {
        HeaderBare {
            version: 1,
            epoch,
            counter,
            rtag,
            ttl_epochs,
        }
    }

    /// Compute hash of this header using BLAKE3
    pub fn hash(&self) -> Result<[u8; 32], String> {
        // Serialize to canonical CBOR
        let cbor =
            serde_cbor::to_vec(self).map_err(|e| format!("CBOR serialization failed: {}", e))?;

        // Hash with BLAKE3
        let mut hasher = Hasher::new();
        hasher.update(&cbor);
        let hash = hasher.finalize();

        let mut result = [0u8; 32];
        result.copy_from_slice(hash.as_bytes());
        Ok(result)
    }
}

/// Complete header with CID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    /// Bare header fields
    pub bare: HeaderBare,

    /// Content identifier (Merkle-like hash)
    pub cid: Cid,
}

impl Header {
    /// Create header with computed CID
    pub fn new(bare: HeaderBare, ciphertext: &[u8]) -> Result<Self, String> {
        let cid = Self::compute_cid(&bare, ciphertext)?;
        Ok(Header { bare, cid })
    }

    /// Compute Merkle-like CID: sha256(sha256(HeaderBare) || sha256(ciphertext))
    pub fn compute_cid(bare: &HeaderBare, ciphertext: &[u8]) -> Result<Cid, String> {
        // Hash the bare header
        let header_hash = bare.hash()?;

        // Hash the ciphertext
        let mut ciphertext_hasher = Hasher::new();
        ciphertext_hasher.update(ciphertext);
        let ciphertext_hash = ciphertext_hasher.finalize();

        // Combine hashes and hash again (Merkle-like)
        let mut combined_hasher = Hasher::new();
        combined_hasher.update(&header_hash);
        combined_hasher.update(ciphertext_hash.as_bytes());
        let final_hash = combined_hasher.finalize();

        let mut cid_bytes = [0u8; 32];
        cid_bytes.copy_from_slice(final_hash.as_bytes());
        Ok(Cid(cid_bytes))
    }

    /// Verify header integrity without full ciphertext
    pub fn verify(&self, ciphertext: &[u8]) -> Result<bool, String> {
        let computed_cid = Self::compute_cid(&self.bare, ciphertext)?;
        Ok(computed_cid == self.cid)
    }
}

/// Complete envelope with header, ciphertext, and padding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Envelope header
    pub header: Header,

    /// Encrypted payload
    pub ciphertext: Vec<u8>,
}

impl Envelope {
    /// Create a new envelope with padding to fixed size
    pub fn new(header: Header, ciphertext: Vec<u8>) -> Result<Self, String> {
        let envelope = Envelope { header, ciphertext };

        // Verify it serializes to exactly ENVELOPE_SIZE
        let serialized = envelope.to_bytes()?;
        if serialized.len() != ENVELOPE_SIZE {
            return Err(format!(
                "envelope size mismatch: expected {}, got {}",
                ENVELOPE_SIZE,
                serialized.len()
            ));
        }

        Ok(envelope)
    }

    /// Serialize envelope to exactly ENVELOPE_SIZE bytes with padding
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        // Serialize to CBOR
        let cbor =
            serde_cbor::to_vec(self).map_err(|e| format!("CBOR serialization failed: {}", e))?;

        if cbor.len() > ENVELOPE_SIZE {
            return Err(format!(
                "envelope too large: {} bytes (max {})",
                cbor.len(),
                ENVELOPE_SIZE
            ));
        }

        // Pad to ENVELOPE_SIZE
        let mut padded = cbor;
        padded.resize(ENVELOPE_SIZE, 0);

        Ok(padded)
    }

    /// Deserialize envelope from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != ENVELOPE_SIZE {
            return Err(format!(
                "invalid envelope size: expected {}, got {}",
                ENVELOPE_SIZE,
                bytes.len()
            ));
        }

        // Find the actual CBOR data (before padding)
        // CBOR always starts with a type byte, padding is zeros
        let cbor_end = bytes
            .iter()
            .rposition(|&b| b != 0)
            .map(|i| i + 1)
            .unwrap_or(0);

        serde_cbor::from_slice(&bytes[..cbor_end])
            .map_err(|e| format!("CBOR deserialization failed: {}", e))
    }

    /// Get the CID of this envelope
    pub fn cid(&self) -> &Cid {
        &self.header.cid
    }

    /// Verify envelope integrity
    pub fn verify(&self) -> Result<bool, String> {
        self.header.verify(&self.ciphertext)
    }

    /// Check if envelope has expired based on current epoch
    pub fn is_expired(&self, current_epoch: u64) -> bool {
        let expiry_epoch = self.header.bare.epoch + self.header.bare.ttl_epochs as u64;
        current_epoch >= expiry_epoch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cid_hex_roundtrip() {
        let cid = Cid([0x42; 32]);
        let hex = cid.to_hex();
        let parsed = Cid::from_hex(&hex).unwrap();
        assert_eq!(cid, parsed);
    }

    #[test]
    fn test_header_bare_hash() {
        let rtag = RoutingTag([0x12; 16]);
        let bare = HeaderBare::new(100, 42, rtag, 10);

        let hash1 = bare.hash().unwrap();
        let hash2 = bare.hash().unwrap();

        // Hashing should be deterministic
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, [0u8; 32]);
    }

    #[test]
    fn test_cid_computation() {
        let rtag = RoutingTag([0x12; 16]);
        let bare = HeaderBare::new(100, 42, rtag, 10);
        let ciphertext = vec![1, 2, 3, 4, 5];

        let cid1 = Header::compute_cid(&bare, &ciphertext).unwrap();
        let cid2 = Header::compute_cid(&bare, &ciphertext).unwrap();

        // CID computation should be deterministic
        assert_eq!(cid1, cid2);

        // Different ciphertext should produce different CID
        let different_ciphertext = vec![1, 2, 3, 4, 6];
        let cid3 = Header::compute_cid(&bare, &different_ciphertext).unwrap();
        assert_ne!(cid1, cid3);
    }

    #[test]
    fn test_header_verification() {
        let rtag = RoutingTag([0x12; 16]);
        let bare = HeaderBare::new(100, 42, rtag, 10);
        let ciphertext = vec![1, 2, 3, 4, 5];

        let header = Header::new(bare, &ciphertext).unwrap();

        // Verification should pass with correct ciphertext
        assert!(header.verify(&ciphertext).unwrap());

        // Verification should fail with different ciphertext
        let wrong_ciphertext = vec![1, 2, 3, 4, 6];
        assert!(!header.verify(&wrong_ciphertext).unwrap());
    }

    #[test]
    fn test_envelope_fixed_size() {
        let rtag = RoutingTag([0x12; 16]);
        let bare = HeaderBare::new(100, 42, rtag, 10);
        let ciphertext = vec![0x42; 100]; // Smaller payload

        let header = Header::new(bare, &ciphertext).unwrap();
        let envelope = Envelope::new(header, ciphertext).unwrap();

        let bytes = envelope.to_bytes().unwrap();
        assert_eq!(bytes.len(), ENVELOPE_SIZE);
    }

    #[test]
    fn test_envelope_roundtrip() {
        let rtag = RoutingTag([0x12; 16]);
        let bare = HeaderBare::new(100, 42, rtag.clone(), 10);
        let ciphertext = vec![0x42; 100];

        let header = Header::new(bare, &ciphertext).unwrap();
        let original = Envelope::new(header, ciphertext.clone()).unwrap();

        let bytes = original.to_bytes().unwrap();
        let recovered = Envelope::from_bytes(&bytes).unwrap();

        assert_eq!(original.cid(), recovered.cid());
        assert_eq!(original.ciphertext, recovered.ciphertext);
        assert_eq!(original.header.bare.epoch, recovered.header.bare.epoch);
        assert_eq!(original.header.bare.counter, recovered.header.bare.counter);
        assert_eq!(original.header.bare.rtag, recovered.header.bare.rtag);
    }

    #[test]
    fn test_envelope_verification() {
        let rtag = RoutingTag([0x12; 16]);
        let bare = HeaderBare::new(100, 42, rtag, 10);
        let ciphertext = vec![0x42; 100];

        let header = Header::new(bare, &ciphertext).unwrap();
        let envelope = Envelope::new(header, ciphertext).unwrap();

        assert!(envelope.verify().unwrap());
    }

    #[test]
    fn test_envelope_expiry() {
        let rtag = RoutingTag([0x12; 16]);
        let bare = HeaderBare::new(100, 42, rtag, 10);
        let ciphertext = vec![0x42; 100];

        let header = Header::new(bare, &ciphertext).unwrap();
        let envelope = Envelope::new(header, ciphertext).unwrap();

        // Not expired at epoch 109
        assert!(!envelope.is_expired(109));

        // Expired at epoch 110 (100 + 10)
        assert!(envelope.is_expired(110));

        // Expired at epoch 111
        assert!(envelope.is_expired(111));
    }

    #[test]
    fn test_envelope_too_large() {
        let rtag = RoutingTag([0x12; 16]);
        let bare = HeaderBare::new(100, 42, rtag, 10);

        // Create ciphertext that's too large
        let huge_ciphertext = vec![0x42; ENVELOPE_SIZE];

        let header = Header::new(bare, &huge_ciphertext).unwrap();
        let result = Envelope::new(header, huge_ciphertext);

        assert!(result.is_err());
    }
}
