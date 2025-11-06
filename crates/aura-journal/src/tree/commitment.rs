//! Tree Commitment Computation
//!
//! Implements Blake3-based commitment hashing for ratchet tree nodes.
//! Commitments bind both structure and content, enabling verification of tree integrity.
//!
//! ## Commitment Scheme
//!
//! - Branch: `H("BRANCH", node_index, epoch, policy_tag, left_commitment, right_commitment)`
//! - Leaf: `H("LEAF", leaf_index, epoch, public_key)`
//!
//! The inclusion of node indices ensures that structural tampering is detected.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Commitment hash for a tree node
///
/// A Blake3 hash (32 bytes) binding node content and structure.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct Commitment(pub [u8; 32]);

impl Commitment {
    /// Create a commitment from a 32-byte array
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Create a commitment from a slice (must be exactly 32 bytes)
    pub fn from_slice(bytes: &[u8]) -> Result<Self, CommitmentError> {
        if bytes.len() != 32 {
            return Err(CommitmentError::InvalidLength {
                expected: 32,
                actual: bytes.len(),
            });
        }
        let mut array = [0u8; 32];
        array.copy_from_slice(bytes);
        Ok(Self(array))
    }

    /// Get the bytes of this commitment
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to a hex string for display
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Create from a hex string
    pub fn from_hex(s: &str) -> Result<Self, CommitmentError> {
        let bytes = hex::decode(s).map_err(|_| CommitmentError::InvalidHex)?;
        Self::from_slice(&bytes)
    }
}

impl fmt::Debug for Commitment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Commitment({}...)", &self.to_hex()[..8])
    }
}

impl fmt::Display for Commitment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &self.to_hex()[..16])
    }
}

/// Tag identifying the type of node being committed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitmentTag {
    /// Branch node commitment
    Branch,
    /// Leaf node commitment
    Leaf,
}

impl CommitmentTag {
    /// Get the tag as a byte string
    pub fn as_bytes(&self) -> &'static [u8] {
        match self {
            CommitmentTag::Branch => b"BRANCH",
            CommitmentTag::Leaf => b"LEAF",
        }
    }
}

/// Errors that can occur during commitment operations
#[derive(Debug, thiserror::Error)]
pub enum CommitmentError {
    /// Invalid commitment length
    #[error("Invalid commitment length: expected {expected}, got {actual}")]
    InvalidLength {
        /// Expected length
        expected: usize,
        /// Actual length
        actual: usize,
    },

    /// Invalid hex string
    #[error("Invalid hex string")]
    InvalidHex,
}

/// Compute a branch commitment
///
/// Hashes: `H("BRANCH", node_index, epoch, policy_tag, left_commitment, right_commitment)`
pub fn compute_branch_commitment(
    node_index: usize,
    epoch: u64,
    policy_tag: u8,
    left_commitment: &Commitment,
    right_commitment: &Commitment,
) -> Commitment {
    let mut hasher = blake3::Hasher::new();

    // Tag
    hasher.update(CommitmentTag::Branch.as_bytes());

    // Node index (8 bytes, little-endian)
    hasher.update(&node_index.to_le_bytes());

    // Epoch (8 bytes, little-endian)
    hasher.update(&epoch.to_le_bytes());

    // Policy tag (1 byte: 0=All, 1=Any, 2=Threshold)
    hasher.update(&[policy_tag]);

    // Left commitment
    hasher.update(left_commitment.as_bytes());

    // Right commitment
    hasher.update(right_commitment.as_bytes());

    Commitment(*hasher.finalize().as_bytes())
}

/// Compute a leaf commitment
///
/// Hashes: `H("LEAF", leaf_index, epoch, public_key)`
pub fn compute_leaf_commitment(leaf_index: usize, epoch: u64, public_key: &[u8]) -> Commitment {
    let mut hasher = blake3::Hasher::new();

    // Tag
    hasher.update(CommitmentTag::Leaf.as_bytes());

    // Leaf index (8 bytes, little-endian)
    hasher.update(&leaf_index.to_le_bytes());

    // Epoch (8 bytes, little-endian)
    hasher.update(&epoch.to_le_bytes());

    // Public key
    hasher.update(public_key);

    Commitment(*hasher.finalize().as_bytes())
}

/// Get the policy tag byte for a policy
pub fn policy_tag(policy: &crate::tree::Policy) -> u8 {
    use crate::tree::Policy;
    match policy {
        Policy::All => 0,
        Policy::Any => 1,
        Policy::Threshold { .. } => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commitment_creation() {
        let bytes = [42u8; 32];
        let commitment = Commitment::new(bytes);
        assert_eq!(commitment.as_bytes(), &bytes);
    }

    #[test]
    fn test_commitment_from_slice() {
        let bytes = vec![1u8; 32];
        let commitment = Commitment::from_slice(&bytes).unwrap();
        assert_eq!(commitment.as_bytes()[0], 1);
    }

    #[test]
    fn test_commitment_from_slice_wrong_length() {
        let bytes = vec![1u8; 16];
        let result = Commitment::from_slice(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_commitment_hex() {
        let bytes = [0u8; 32];
        let commitment = Commitment::new(bytes);
        let hex = commitment.to_hex();
        assert_eq!(hex.len(), 64); // 32 bytes = 64 hex chars

        let parsed = Commitment::from_hex(&hex).unwrap();
        assert_eq!(parsed, commitment);
    }

    #[test]
    fn test_commitment_deterministic() {
        // Same inputs should produce same commitment
        let left = Commitment::new([1u8; 32]);
        let right = Commitment::new([2u8; 32]);

        let c1 = compute_branch_commitment(1, 0, 0, &left, &right);
        let c2 = compute_branch_commitment(1, 0, 0, &left, &right);

        assert_eq!(c1, c2);
    }

    #[test]
    fn test_commitment_different_inputs() {
        // Different inputs should produce different commitments
        let left = Commitment::new([1u8; 32]);
        let right = Commitment::new([2u8; 32]);

        let c1 = compute_branch_commitment(1, 0, 0, &left, &right);
        let c2 = compute_branch_commitment(2, 0, 0, &left, &right); // Different index

        assert_ne!(c1, c2);
    }

    #[test]
    fn test_leaf_commitment() {
        let public_key = vec![0u8; 32];
        let c1 = compute_leaf_commitment(0, 0, &public_key);
        let c2 = compute_leaf_commitment(0, 0, &public_key);
        assert_eq!(c1, c2);

        let c3 = compute_leaf_commitment(1, 0, &public_key); // Different index
        assert_ne!(c1, c3);
    }

    #[test]
    fn test_commitment_tag() {
        assert_eq!(CommitmentTag::Branch.as_bytes(), b"BRANCH");
        assert_eq!(CommitmentTag::Leaf.as_bytes(), b"LEAF");
    }

    #[test]
    fn test_policy_tags() {
        use crate::tree::Policy;
        assert_eq!(policy_tag(&Policy::All), 0);
        assert_eq!(policy_tag(&Policy::Any), 1);
        assert_eq!(policy_tag(&Policy::Threshold { m: 2, n: 3 }), 2);
    }
}
