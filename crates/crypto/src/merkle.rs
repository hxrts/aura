// Merkle tree operations for DKD commitment persistence
//
// Reference: 080_architecture_protocol_integration.md - Part 3: Ledger Compaction
//
// This module implements Merkle tree construction and proof generation/verification
// for DKD commitments. This allows recovery to verify guardian shares after ledger
// compaction has pruned the original commitment events.

use crate::{CryptoError, Result};
use crate::MerkleProof;

/// Build a Merkle tree from commitment hashes
///
/// Returns the Merkle root and a proof for each leaf (commitment)
///
/// Reference: 080 spec Part 3: Ledger Compaction
pub fn build_commitment_tree(commitments: &[[u8; 32]]) -> Result<([u8; 32], Vec<MerkleProof>)> {
    if commitments.is_empty() {
        return Err(CryptoError::CryptoError(
            "Cannot build Merkle tree from empty commitment list".to_string(),
        ));
    }

    // Handle single commitment case
    if commitments.len() == 1 {
        let proof = MerkleProof {
            commitment_hash: commitments[0],
            siblings: Vec::new(),
            path_indices: Vec::new(),
        };
        return Ok((commitments[0], vec![proof]));
    }

    // Build tree bottom-up
    let mut current_level: Vec<[u8; 32]> = commitments.to_vec();
    let mut all_levels = vec![current_level.clone()];

    // Build tree levels
    while current_level.len() > 1 {
        let mut next_level = Vec::new();

        for i in (0..current_level.len()).step_by(2) {
            if i + 1 < current_level.len() {
                // Pair exists
                let parent = compute_parent_hash(&current_level[i], &current_level[i + 1]);
                next_level.push(parent);
            } else {
                // Odd node, promote to next level
                next_level.push(current_level[i]);
            }
        }

        current_level = next_level;
        all_levels.push(current_level.clone());
    }

    // Root is the single node in the top level
    let root = current_level[0];

    // Generate proofs for each commitment
    let proofs = commitments
        .iter()
        .enumerate()
        .map(|(idx, &commitment)| generate_proof(&all_levels, idx, commitment))
        .collect();

    Ok((root, proofs))
}

/// Generate a Merkle proof for a specific leaf index
fn generate_proof(
    levels: &[Vec<[u8; 32]>],
    leaf_index: usize,
    commitment_hash: [u8; 32],
) -> MerkleProof {
    let mut siblings = Vec::new();
    let mut path_indices = Vec::new();
    let mut current_index = leaf_index;

    // Traverse from leaf to root, collecting siblings
    for level in levels.iter().take(levels.len() - 1) {
        // Determine sibling index and position
        let is_right_child = current_index % 2 == 1;
        let sibling_index = if is_right_child {
            current_index - 1
        } else {
            current_index + 1
        };

        // Add sibling if it exists
        if sibling_index < level.len() {
            siblings.push(level[sibling_index]);
            // The journal verification does: if is_right then hash(current, sibling) else hash(sibling, current)
            // But path_indices documents "true = right", so we need to match the actual behavior
            // Based on the verification logic, we need to invert the logic:
            // - if current is right child (index % 2 == 1), we want is_right = false
            // - if current is left child (index % 2 == 0), we want is_right = true
            path_indices.push(!is_right_child);
        }

        // Move to parent level
        current_index /= 2;
    }

    MerkleProof {
        commitment_hash,
        siblings,
        path_indices,
    }
}

/// Compute parent hash in Merkle tree
///
/// This is exported in types.rs but duplicated here for tree construction
fn compute_parent_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(left);
    hasher.update(right);
    *hasher.finalize().as_bytes()
}

/// Verify a Merkle proof (convenience function)
///
/// Delegates to MerkleProof::verify() but provides a cleaner API
pub fn verify_merkle_proof(
    commitment_hash: &[u8; 32],
    proof: &MerkleProof,
    root: &[u8; 32],
) -> bool {
    // Verify commitment matches proof
    if &proof.commitment_hash != commitment_hash {
        return false;
    }

    // Verify proof against root
    proof.verify(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_str(s: &str) -> [u8; 32] {
        *blake3::hash(s.as_bytes()).as_bytes()
    }

    #[test]
    fn test_single_commitment() {
        let commitment = hash_str("commitment1");
        let (root, proofs) = build_commitment_tree(&[commitment]).unwrap();

        // Root should be the commitment itself
        assert_eq!(root, commitment);

        // Proof should be empty (no siblings)
        assert_eq!(proofs.len(), 1);
        assert!(proofs[0].siblings.is_empty());
        assert!(proofs[0].path_indices.is_empty());

        // Verify proof
        assert!(verify_merkle_proof(&commitment, &proofs[0], &root));
    }

    #[test]
    fn test_two_commitments() {
        let c1 = hash_str("commitment1");
        let c2 = hash_str("commitment2");

        let (root, proofs) = build_commitment_tree(&[c1, c2]).unwrap();

        assert_eq!(proofs.len(), 2);

        // Root should be hash(c1 || c2)
        let expected_root = compute_parent_hash(&c1, &c2);
        assert_eq!(root, expected_root);

        // Verify both proofs
        assert!(verify_merkle_proof(&c1, &proofs[0], &root));
        assert!(verify_merkle_proof(&c2, &proofs[1], &root));
    }

    #[test]
    fn test_three_commitments() {
        let c1 = hash_str("commitment1");
        let c2 = hash_str("commitment2");
        let c3 = hash_str("commitment3");

        let (root, proofs) = build_commitment_tree(&[c1, c2, c3]).unwrap();

        assert_eq!(proofs.len(), 3);

        // Verify all proofs
        assert!(verify_merkle_proof(&c1, &proofs[0], &root));
        assert!(verify_merkle_proof(&c2, &proofs[1], &root));
        assert!(verify_merkle_proof(&c3, &proofs[2], &root));
    }

    #[test]
    fn test_four_commitments() {
        let c1 = hash_str("commitment1");
        let c2 = hash_str("commitment2");
        let c3 = hash_str("commitment3");
        let c4 = hash_str("commitment4");

        let (root, proofs) = build_commitment_tree(&[c1, c2, c3, c4]).unwrap();

        assert_eq!(proofs.len(), 4);

        // Verify all proofs
        assert!(verify_merkle_proof(&c1, &proofs[0], &root));
        assert!(verify_merkle_proof(&c2, &proofs[1], &root));
        assert!(verify_merkle_proof(&c3, &proofs[2], &root));
        assert!(verify_merkle_proof(&c4, &proofs[3], &root));

        // Verify tree structure
        // Level 0: c1, c2, c3, c4
        // Level 1: hash(c1,c2), hash(c3,c4)
        // Level 2: hash(hash(c1,c2), hash(c3,c4))
        let h12 = compute_parent_hash(&c1, &c2);
        let h34 = compute_parent_hash(&c3, &c4);
        let expected_root = compute_parent_hash(&h12, &h34);
        assert_eq!(root, expected_root);
    }

    #[test]
    fn test_invalid_proof_rejected() {
        let c1 = hash_str("commitment1");
        let c2 = hash_str("commitment2");
        let c3 = hash_str("tampered");

        let (root, proofs) = build_commitment_tree(&[c1, c2]).unwrap();

        // Try to verify c3 with c1's proof - should fail
        assert!(!verify_merkle_proof(&c3, &proofs[0], &root));
    }

    #[test]
    fn test_wrong_root_rejected() {
        let c1 = hash_str("commitment1");
        let c2 = hash_str("commitment2");

        let (_root, proofs) = build_commitment_tree(&[c1, c2]).unwrap();

        let wrong_root = hash_str("wrong_root");

        // Proof should fail against wrong root
        assert!(!verify_merkle_proof(&c1, &proofs[0], &wrong_root));
    }

    #[test]
    fn test_empty_commitments() {
        let result = build_commitment_tree(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_large_tree() {
        // Test with many commitments
        let commitments: Vec<[u8; 32]> = (0..100)
            .map(|i| hash_str(&format!("commitment{}", i)))
            .collect();

        let (root, proofs) = build_commitment_tree(&commitments).unwrap();

        assert_eq!(proofs.len(), 100);

        // Verify all proofs
        for (commitment, proof) in commitments.iter().zip(proofs.iter()) {
            assert!(verify_merkle_proof(commitment, proof, &root));
        }
    }

    #[test]
    fn test_proof_structure() {
        let c1 = hash_str("c1");
        let c2 = hash_str("c2");
        let c3 = hash_str("c3");
        let c4 = hash_str("c4");

        let (root, proofs) = build_commitment_tree(&[c1, c2, c3, c4]).unwrap();

        // For a balanced tree of 4 leaves, each proof should have 2 siblings
        // (one at level 0, one at level 1)
        for proof in &proofs {
            assert_eq!(proof.siblings.len(), 2, "Each proof should have 2 siblings");
            assert_eq!(
                proof.path_indices.len(),
                2,
                "Each proof should have 2 path indices"
            );
        }

        // Manually reconstruct root from proof[0] to verify structure
        // Use the same logic as the journal's verify function
        let mut current = c1;
        for (sibling, is_right) in proofs[0].siblings.iter().zip(proofs[0].path_indices.iter()) {
            current = if *is_right {
                // Current is left child (journal logic)
                compute_parent_hash(&current, sibling)
            } else {
                // Current is right child (journal logic)
                compute_parent_hash(sibling, &current)
            };
        }
        assert_eq!(current, root);
    }
}
