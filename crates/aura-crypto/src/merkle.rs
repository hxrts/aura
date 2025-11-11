//! Merkle tree utilities for cryptographic operations
//!
//! Simple merkle tree utilities using the effects system for hashing.

use aura_core::effects::CryptoEffects;
use crate::Result;

/// Merkle proof structure containing sibling path and directions
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimpleMerkleProof {
    /// Path of sibling hashes from leaf to root
    pub sibling_path: Vec<[u8; 32]>,
    /// Index of the leaf in the original tree (used to determine path directions)
    pub leaf_index: usize,
    /// Total number of leaves in the tree (needed for reconstruction)
    pub tree_size: usize,
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
    pub fn with_params(sibling_path: Vec<[u8; 32]>, leaf_index: usize, tree_size: usize) -> Self {
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

/// Generate a Merkle proof for a specific leaf in the tree
///
/// # Arguments
/// * `leaves` - All leaf values in the tree
/// * `leaf_index` - Index of the leaf to generate a proof for
/// * `effects` - Effects object for hashing operations
///
/// # Returns
/// A Merkle proof for the specified leaf
pub async fn generate_merkle_proof(
    leaves: &[Vec<u8>],
    leaf_index: usize,
    effects: &impl CryptoEffects,
) -> Result<SimpleMerkleProof> {
    if leaves.is_empty() || leaf_index >= leaves.len() {
        return Ok(SimpleMerkleProof::new());
    }

    let mut current_level: Vec<[u8; 32]> = Vec::new();
    
    // Hash all leaves to create the bottom level
    for leaf in leaves {
        current_level.push(effects.hash(leaf).await);
    }

    let mut sibling_path = Vec::new();
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
                        sibling_path.push(*left);  // Left sibling
                    }
                }
                
                // Combine left and right hashes
                let mut combined = Vec::new();
                combined.extend_from_slice(left);
                combined.extend_from_slice(right);
                next_level.push(effects.hash(&combined).await);
            } else {
                // Odd node - promote to next level
                next_level.push(current_level[i]);
            }
        }
        
        current_level = next_level;
        index /= 2; // Move up one level in the tree
    }

    Ok(SimpleMerkleProof::with_params(
        sibling_path,
        leaf_index,
        leaves.len(),
    ))
}

/// Build a Merkle root from a list of leaves using proper tree construction
///
/// # Arguments
/// * `leaves` - List of leaf values
/// * `effects` - Effects object for hashing operations
///
/// # Returns
/// The computed Merkle root hash
pub async fn build_merkle_root(leaves: &[Vec<u8>], effects: &impl CryptoEffects) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }

    let mut current_level: Vec<[u8; 32]> = Vec::new();
    
    // Hash all leaves to create the bottom level
    for leaf in leaves {
        current_level.push(effects.hash(leaf).await);
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
                next_level.push(effects.hash(&combined).await);
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
/// * `effects` - Effects object for hashing operations
///
/// # Returns
/// `true` if the proof is valid, `false` otherwise
pub async fn verify_merkle_proof(
    proof: &SimpleMerkleProof,
    root: &[u8; 32],
    leaf_value: &[u8],
    effects: &impl CryptoEffects,
) -> bool {
    // Start with the leaf hash
    let mut current_hash = effects.hash(leaf_value).await;
    
    // If no siblings, tree has only one leaf
    if proof.sibling_path.is_empty() {
        return &current_hash == root;
    }
    
    let mut index = proof.leaf_index;
    
    // Walk up the tree using sibling path
    for sibling_hash in &proof.sibling_path {
        let mut combined = Vec::new();
        
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
        current_hash = effects.hash(&combined).await;
        
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
/// * `effects` - Effects object for hashing operations
///
/// # Returns
/// A tuple containing (root_hash, proof_for_first_commitment)
pub async fn build_commitment_tree(
    commitments: &[Vec<u8>],
    effects: &impl CryptoEffects,
) -> Result<(Option<[u8; 32]>, SimpleMerkleProof)> {
    if commitments.is_empty() {
        return Ok((None, SimpleMerkleProof::new()));
    }

    // Build the merkle root
    let root = build_merkle_root(commitments, effects).await;
    
    // Generate proof for the first commitment (index 0)
    let proof = generate_merkle_proof(commitments, 0, effects).await?;
    
    Ok((Some(root), proof))
}
