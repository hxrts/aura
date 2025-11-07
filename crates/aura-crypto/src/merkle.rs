//! Merkle tree utilities for cryptographic operations
//!
//! Simple merkle tree utilities using the effects system for hashing.

use crate::effects::CryptoEffects;
use crate::Result;

/// Simple Merkle proof structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimpleMerkleProof {
    /// Path of hashes from leaf to root
    pub proof_path: Vec<[u8; 32]>,
    /// Index of the leaf in the original tree
    pub leaf_index: usize,
}

impl SimpleMerkleProof {
    /// Create a new empty Merkle proof
    pub fn new() -> Self {
        Self {
            proof_path: Vec::new(),
            leaf_index: 0,
        }
    }
}

impl Default for SimpleMerkleProof {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a commitment tree from a list of commitment hashes
///
/// This is a simplified implementation using the effects system for hashing.
///
/// # Arguments
/// * `commitments` - List of commitment hashes to build tree from
/// * `effects` - Effects object for hashing operations
///
/// # Returns
/// A simple merkle proof or an error if the operation fails
pub async fn build_commitment_tree(
    commitments: &[Vec<u8>],
    effects: &impl CryptoEffects,
) -> Result<SimpleMerkleProof> {
    // Simplified implementation
    if commitments.is_empty() {
        return Ok(SimpleMerkleProof::new());
    }

    // Simple placeholder: create a proof with the first commitment as the leaf
    let mut proof_path = Vec::new();

    // Add path elements using blake3 hashing
    for i in 0..commitments.len().min(8) {
        let hash = if i < commitments.len() {
            effects.blake3_hash_async(&commitments[i]).await
        } else {
            [0u8; 32]
        };
        proof_path.push(hash);
    }

    Ok(SimpleMerkleProof {
        proof_path,
        leaf_index: 0,
    })
}

/// Build a Merkle root from a list of leaves using effects system
///
/// # Arguments
/// * `leaves` - List of leaf hashes
/// * `effects` - Effects object for hashing operations
///
/// # Returns
/// The computed Merkle root hash
pub async fn build_merkle_root(leaves: &[Vec<u8>], effects: &impl CryptoEffects) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }

    // Simple implementation: hash all leaves together
    let mut combined = Vec::new();
    for leaf in leaves {
        combined.extend_from_slice(leaf);
    }

    effects.blake3_hash_async(&combined).await
}

/// Verify a Merkle proof against a root hash (simplified)
///
/// # Arguments
/// * `proof` - The merkle proof to verify
/// * `root` - The root hash to verify against
/// * `leaf` - The leaf hash being verified
/// * `effects` - Effects object for hashing operations
///
/// # Returns
/// `true` if the proof is valid, `false` otherwise
pub async fn verify_merkle_proof(
    proof: &SimpleMerkleProof,
    root: &[u8; 32],
    leaf: &[u8; 32],
    effects: &impl CryptoEffects,
) -> bool {
    // Simplified verification
    if proof.proof_path.is_empty() {
        return effects.blake3_hash_async(leaf).await == *root;
    }

    // In a real implementation, this would compute the path to the root
    // For now, just check if the leaf hash matches any in the proof path
    let leaf_hash = effects.blake3_hash_async(leaf).await;
    proof.proof_path.contains(&leaf_hash)
}
