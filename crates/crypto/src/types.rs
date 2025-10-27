// Shared types for Aura cryptographic system

use serde::{Deserialize, Serialize};

// Re-export ID types from aura-types (single source of truth)
pub use aura_types::{AccountId, DeviceId, GuardianId};

/// Merkle proof for commitment verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    /// Hash of the commitment being proven
    pub commitment_hash: [u8; 32],
    /// Sibling hashes along the path to root
    pub siblings: Vec<[u8; 32]>,
    /// Path direction indices (true = right, false = left)
    pub path_indices: Vec<bool>, // true = right, false = left
}

impl MerkleProof {
    /// Verify this proof against a Merkle root
    pub fn verify(&self, root: &[u8; 32]) -> bool {
        let mut current_hash = self.commitment_hash;

        for (sibling, is_right) in self.siblings.iter().zip(self.path_indices.iter()) {
            current_hash = if *is_right {
                // Current is left child
                compute_parent_hash(&current_hash, sibling)
            } else {
                // Current is right child
                compute_parent_hash(sibling, &current_hash)
            };
        }

        current_hash == *root
    }
}

/// Compute parent hash from left and right children
fn compute_parent_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(left);
    hasher.update(right);
    *hasher.finalize().as_bytes()
}

// ========== Type-Safe Crypto Primitives ==========

/// Type-safe 256-bit hash value
///
/// Wraps a raw [u8; 32] to provide type safety and prevent accidental
/// mixing of different hash types (commitment hashes, content hashes, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hash256(pub [u8; 32]);

impl Hash256 {
    /// Create a new Hash256 from raw bytes
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the raw bytes
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse from hex string
    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        let mut bytes = [0u8; 32];
        hex::decode_to_slice(s, &mut bytes)?;
        Ok(Self(bytes))
    }
}

impl From<[u8; 32]> for Hash256 {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl From<Hash256> for [u8; 32] {
    fn from(hash: Hash256) -> Self {
        hash.0
    }
}

impl AsRef<[u8]> for Hash256 {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Type-safe cryptographic nonce
///
/// Wraps a raw [u8; 12] to provide type safety for AES-GCM nonces
/// and prevent accidental reuse or mixing with other byte arrays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nonce(pub [u8; 12]);

impl Nonce {
    /// Create a new Nonce from raw bytes
    pub const fn new(bytes: [u8; 12]) -> Self {
        Self(bytes)
    }

    /// Get the raw bytes
    pub const fn as_bytes(&self) -> &[u8; 12] {
        &self.0
    }

    /// Generate a random nonce using the provided effects
    pub fn random(effects: &crate::Effects) -> Self {
        Self(effects.random_bytes())
    }
}

impl From<[u8; 12]> for Nonce {
    fn from(bytes: [u8; 12]) -> Self {
        Self(bytes)
    }
}

impl From<Nonce> for [u8; 12] {
    fn from(nonce: Nonce) -> Self {
        nonce.0
    }
}

impl AsRef<[u8]> for Nonce {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Type-safe cryptographic signature wrapper
///
/// Provides a type-safe wrapper around ed25519 signatures to prevent
/// accidental mixing with raw byte arrays or other signature types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignatureBytes(pub [u8; 64]);

impl Serialize for SignatureBytes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for SignatureBytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        if bytes.len() != 64 {
            return Err(serde::de::Error::invalid_length(bytes.len(), &"64 bytes"));
        }
        let mut array = [0u8; 64];
        array.copy_from_slice(&bytes);
        Ok(Self(array))
    }
}

impl SignatureBytes {
    /// Create a new SignatureBytes from raw bytes
    pub const fn new(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    /// Get the raw bytes
    pub const fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse from hex string
    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        let mut bytes = [0u8; 64];
        hex::decode_to_slice(s, &mut bytes)?;
        Ok(Self(bytes))
    }
}

impl From<[u8; 64]> for SignatureBytes {
    fn from(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }
}

impl From<SignatureBytes> for [u8; 64] {
    fn from(sig: SignatureBytes) -> Self {
        sig.0
    }
}

impl From<ed25519_dalek::Signature> for SignatureBytes {
    fn from(sig: ed25519_dalek::Signature) -> Self {
        Self(sig.to_bytes())
    }
}

impl TryFrom<SignatureBytes> for ed25519_dalek::Signature {
    type Error = ed25519_dalek::SignatureError;

    fn try_from(bytes: SignatureBytes) -> Result<Self, Self::Error> {
        Ok(ed25519_dalek::Signature::from_bytes(&bytes.0))
    }
}

impl AsRef<[u8]> for SignatureBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash256() {
        let bytes = [42u8; 32];
        let hash = Hash256::new(bytes);
        assert_eq!(hash.as_bytes(), &bytes);
        assert_eq!(Hash256::from(bytes), hash);
        assert_eq!(<[u8; 32]>::from(hash), bytes);
    }

    #[test]
    fn test_hash256_hex() {
        let hash = Hash256::new([0xAB; 32]);
        let hex = hash.to_hex();
        assert_eq!(hex.len(), 64); // 32 bytes * 2 hex chars
        let parsed = Hash256::from_hex(&hex).unwrap();
        assert_eq!(parsed, hash);
    }

    #[test]
    fn test_nonce() {
        let bytes = [42u8; 12];
        let nonce = Nonce::new(bytes);
        assert_eq!(nonce.as_bytes(), &bytes);
        assert_eq!(Nonce::from(bytes), nonce);
        assert_eq!(<[u8; 12]>::from(nonce), bytes);
    }

    #[test]
    fn test_signature_bytes() {
        let bytes = [42u8; 64];
        let sig = SignatureBytes::new(bytes);
        assert_eq!(sig.as_bytes(), &bytes);
        assert_eq!(SignatureBytes::from(bytes), sig);
        assert_eq!(<[u8; 64]>::from(sig), bytes);
    }

    #[test]
    fn test_signature_bytes_hex() {
        let sig = SignatureBytes::new([0xCD; 64]);
        let hex = sig.to_hex();
        assert_eq!(hex.len(), 128); // 64 bytes * 2 hex chars
        let parsed = SignatureBytes::from_hex(&hex).unwrap();
        assert_eq!(parsed, sig);
    }
}
