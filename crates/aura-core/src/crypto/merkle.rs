//! Merkle tree utilities for cryptographic operations
//!
//! Simple merkle tree utilities using pure synchronous hashing.

use crate::crypto::hash::hash;
use crate::Result;

/// Maximum depth of a merkle tree (supports up to 2^32 leaves)
pub const MAX_MERKLE_DEPTH: u32 = 32;

/// Merkle proof structure containing sibling path and directions
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimpleMerkleProof {
    /// Path of sibling hashes from leaf to root
    pub sibling_path: Vec<[u8; 32]>,
    /// Index of the leaf in the original tree (used to determine path directions)
    pub leaf_index: u32,
    /// Total number of leaves in the tree (needed for reconstruction)
    pub tree_size: u32,
}

impl SimpleMerkleProof {
    /// Create a new empty Merkle proof
    pub fn new() -> Self {
        Self {
            sibling_path: Vec::new(),
            leaf_index: 0,
            tree_size: 0,
        }
    }

    /// Create a new Merkle proof with the specified parameters
    ///
    /// # Panics
    /// Panics if `sibling_path.len()` exceeds `MAX_MERKLE_DEPTH`.
    #[must_use]
    pub fn with_params(sibling_path: Vec<[u8; 32]>, leaf_index: u32, tree_size: u32) -> Self {
        assert!(
            sibling_path.len() <= MAX_MERKLE_DEPTH as usize,
            "sibling_path length {} exceeds MAX_MERKLE_DEPTH {}",
            sibling_path.len(),
            MAX_MERKLE_DEPTH
        );
        Self {
            sibling_path,
            leaf_index,
            tree_size,
        }
    }
}

impl Default for SimpleMerkleProof {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur during Merkle proof validation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MerkleValidationError {
    #[error("Sibling path length {actual} exceeds maximum depth {max}")]
    PathTooLong { actual: u32, max: u32 },

    #[error("Leaf index {index} is out of bounds for tree size {size}")]
    LeafIndexOutOfBounds { index: u32, size: u32 },

    #[error("Invalid tree size: {0}")]
    InvalidTreeSize(u32),
}

impl SimpleMerkleProof {
    /// Validate proof invariants after deserialization.
    ///
    /// Returns `Ok(())` if the proof is well-formed, or an error describing
    /// which invariant was violated. Call this after deserializing a proof
    /// to ensure it meets structural requirements before verification.
    pub fn validate(&self) -> std::result::Result<(), MerkleValidationError> {
        // Check path length doesn't exceed maximum depth
        if self.sibling_path.len() > MAX_MERKLE_DEPTH as usize {
            return Err(MerkleValidationError::PathTooLong {
                actual: self.sibling_path.len() as u32,
                max: MAX_MERKLE_DEPTH,
            });
        }

        // For non-empty proofs, validate index and size consistency
        if self.tree_size > 0 {
            if self.leaf_index >= self.tree_size {
                return Err(MerkleValidationError::LeafIndexOutOfBounds {
                    index: self.leaf_index,
                    size: self.tree_size,
                });
            }
        } else if !self.sibling_path.is_empty() || self.leaf_index != 0 {
            // Empty tree should have no path and zero index
            return Err(MerkleValidationError::InvalidTreeSize(0));
        }

        Ok(())
    }
}

/// Generate a Merkle proof for a specific leaf in the tree
///
/// # Arguments
/// * `leaves` - All leaf values in the tree
/// * `leaf_index` - Index of the leaf to generate a proof for
///
/// # Returns
/// A Merkle proof for the specified leaf
pub fn generate_merkle_proof(leaves: &[Vec<u8>], leaf_index: usize) -> Result<SimpleMerkleProof> {
    if leaves.is_empty() || leaf_index >= leaves.len() {
        return Ok(SimpleMerkleProof::new());
    }

    // Pre-size for number of leaves
    let mut current_level: Vec<[u8; 32]> = Vec::with_capacity(leaves.len());

    // Hash all leaves to create the bottom level
    for leaf in leaves {
        current_level.push(hash(leaf));
    }

    // Sibling path depth is ceil(log2(leaves.len()))
    let estimated_depth = (leaves.len() as f64).log2().ceil() as usize;
    let mut sibling_path = Vec::with_capacity(estimated_depth);
    let mut index = leaf_index;

    // Build path up to root, collecting sibling hashes
    while current_level.len() > 1 {
        let mut next_level = Vec::new();

        for i in (0..current_level.len()).step_by(2) {
            if i + 1 < current_level.len() {
                // We have a pair - combine left and right
                let left = &current_level[i];
                let right = &current_level[i + 1];

                // If this pair contains our target node, save the sibling
                if i == index || i + 1 == index {
                    if i == index {
                        sibling_path.push(*right); // Right sibling
                    } else {
                        sibling_path.push(*left); // Left sibling
                    }
                }

                // Combine left and right hashes
                let mut combined = Vec::new();
                combined.extend_from_slice(left);
                combined.extend_from_slice(right);
                next_level.push(hash(&combined));
            } else {
                // Odd node - promote unchanged to next level.
                // WHY: When tree level has odd count, the unpaired node moves up
                // without hashing. This is the standard "promotion" strategy used
                // in Bitcoin's merkle trees. Alternative would be self-hashing
                // (H(x,x)), but promotion is simpler and equally secure since
                // the tree structure is already encoded in the proof.
                next_level.push(current_level[i]);
            }
        }

        current_level = next_level;
        index /= 2; // Move up one level in the tree
    }

    Ok(SimpleMerkleProof::with_params(
        sibling_path,
        leaf_index as u32,
        leaves.len() as u32,
    ))
}

/// Build a Merkle root from a list of leaves using proper tree construction
///
/// # Arguments
/// * `leaves` - List of leaf values
///
/// # Returns
/// The computed Merkle root hash
pub fn build_merkle_root(leaves: &[Vec<u8>]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }

    // Pre-size for number of leaves
    let mut current_level: Vec<[u8; 32]> = Vec::with_capacity(leaves.len());

    // Hash all leaves to create the bottom level
    for leaf in leaves {
        current_level.push(hash(leaf));
    }

    // Build tree bottom-up until we reach the root
    while current_level.len() > 1 {
        let mut next_level = Vec::new();

        for i in (0..current_level.len()).step_by(2) {
            if i + 1 < current_level.len() {
                // We have a pair - combine left and right
                let left = &current_level[i];
                let right = &current_level[i + 1];

                // Combine left and right hashes: H(left || right)
                let mut combined = Vec::new();
                combined.extend_from_slice(left);
                combined.extend_from_slice(right);
                next_level.push(hash(&combined));
            } else {
                // Odd node - promote to next level unchanged
                next_level.push(current_level[i]);
            }
        }

        current_level = next_level;
    }

    current_level[0]
}

/// Verify a Merkle proof against a root hash using proper cryptographic verification
///
/// # Arguments
/// * `proof` - The merkle proof to verify
/// * `root` - The expected root hash to verify against
/// * `leaf_value` - The original leaf value being verified
///
/// # Returns
/// `true` if the proof is valid, `false` otherwise
pub fn verify_merkle_proof(proof: &SimpleMerkleProof, root: &[u8; 32], leaf_value: &[u8]) -> bool {
    // Validate proof structure before verification
    if proof.validate().is_err() {
        return false;
    }

    // Start with the leaf hash
    let mut current_hash = hash(leaf_value);

    // If no siblings, tree has only one leaf
    if proof.sibling_path.is_empty() {
        return &current_hash == root;
    }

    let mut index = proof.leaf_index as usize;

    // Walk up the tree using sibling path
    for sibling_hash in &proof.sibling_path {
        // Pre-size for two 32-byte hashes concatenated
        let mut combined = Vec::with_capacity(64);

        // Determine if we're the left or right child based on index
        if index % 2 == 0 {
            // We're the left child, sibling is right
            combined.extend_from_slice(&current_hash);
            combined.extend_from_slice(sibling_hash);
        } else {
            // We're the right child, sibling is left
            combined.extend_from_slice(sibling_hash);
            combined.extend_from_slice(&current_hash);
        }

        // Hash the combined value to get parent hash
        current_hash = hash(&combined);

        // Move to parent level
        index /= 2;
    }

    // Check if computed root matches expected root
    &current_hash == root
}

/// Build a commitment tree and return both root and proof for the first commitment
///
/// # Arguments
/// * `commitments` - List of commitment values to build tree from
///
/// # Returns
/// A tuple containing (root_hash, proof_for_first_commitment)
pub fn build_commitment_tree(
    commitments: &[Vec<u8>],
) -> Result<(Option<[u8; 32]>, SimpleMerkleProof)> {
    if commitments.is_empty() {
        return Ok((None, SimpleMerkleProof::new()));
    }

    // Build the merkle root
    let root = build_merkle_root(commitments);

    // Generate proof for the first commitment (index 0)
    let proof = generate_merkle_proof(commitments, 0)?;

    Ok((Some(root), proof))
}
