//! Tree Commitment Functions
//!
//! Implements deterministic cryptographic commitments for tree nodes.
//! Commitments bind the tree structure, policies, and key material into
//! a single hash that changes whenever the tree state changes.
//!
//! # Commitment Format
//!
//! **Branch Commitment:**
//! ```text
//! H("BRANCH", version, node_index, epoch, policy_hash, left_commitment, right_commitment)
//! ```
//!
//! **Leaf Commitment:**
//! ```text
//! H("LEAF", version, leaf_index, epoch, pubkey_hash)
//! ```
//!
//! Children are always ordered by NodeIndex for determinism.
//! Epoch and version are always included to prevent replay.
//!
//! # Reference
//!
//! See [`docs/123_ratchet_tree.md`](../../../docs/123_ratchet_tree.md) - Commitments section.

use super::{Epoch, NodeIndex, Policy, TreeCommitment, TreeHash32};
use crate::hash;

/// Domain separation tag for branch commitments
const BRANCH_TAG: &[u8] = b"BRANCH";

/// Domain separation tag for leaf commitments
const LEAF_TAG: &[u8] = b"LEAF";

/// Current protocol version for commitments
const COMMITMENT_VERSION: u16 = 1;

/// Compute a cryptographic commitment to a branch node.
///
/// The commitment binds:
/// - Node structure (branch tag, version, index)
/// - Temporal context (epoch)
/// - Authorization policy (policy hash)
/// - Child state (left and right commitments)
///
/// Children must be ordered by NodeIndex for deterministic results.
///
/// # Format
///
/// ```text
/// BLAKE3(
///   "BRANCH" ||
///   version (u16, little-endian) ||
///   node_index (u32, little-endian) ||
///   epoch (u64, little-endian) ||
///   policy_hash (32 bytes) ||
///   left_commitment (32 bytes) ||
///   right_commitment (32 bytes)
/// )
/// ```
///
/// # Examples
///
/// ```
/// use aura_core::tree::{commit_branch, Policy, NodeIndex};
///
/// let policy = Policy::Threshold { m: 2, n: 3 };
/// let node_idx = NodeIndex(1);
/// let epoch = 42;
/// let left = [0u8; 32];
/// let right = [1u8; 32];
///
/// let commitment = commit_branch(node_idx, epoch, &policy, &left, &right);
/// assert_eq!(commitment.len(), 32);
/// ```
pub fn commit_branch(
    node_index: NodeIndex,
    epoch: Epoch,
    policy: &Policy,
    left_commitment: &TreeHash32,
    right_commitment: &TreeHash32,
) -> TreeHash32 {
    let policy_hash = policy_hash(policy);

    let mut h = hash::hasher();
    h.update(BRANCH_TAG);
    h.update(&COMMITMENT_VERSION.to_le_bytes());
    h.update(&node_index.0.to_le_bytes());
    h.update(&epoch.to_le_bytes());
    h.update(&policy_hash);
    h.update(left_commitment);
    h.update(right_commitment);

    h.finalize()
}

/// Compute a cryptographic commitment to a leaf node.
///
/// The commitment binds:
/// - Node structure (leaf tag, version, index)
/// - Temporal context (epoch)
/// - Public key material (pubkey hash)
///
/// # Format
///
/// ```text
/// BLAKE3(
///   "LEAF" ||
///   version (u16, little-endian) ||
///   leaf_index (u32, little-endian) ||
///   epoch (u64, little-endian) ||
///   pubkey_hash (32 bytes)
/// )
/// ```
///
/// # Examples
///
/// ```
/// use aura_core::tree::{commit_leaf, LeafId};
///
/// let leaf_id = LeafId(5);
/// let epoch = 42;
/// let pubkey = vec![1, 2, 3, 4]; // Serialized public key
///
/// let commitment = commit_leaf(leaf_id, epoch, &pubkey);
/// assert_eq!(commitment.len(), 32);
/// ```
pub fn commit_leaf(leaf_id: super::LeafId, epoch: Epoch, public_key: &[u8]) -> TreeHash32 {
    let pubkey_hash = hash::hash(public_key);

    let mut h = hash::hasher();
    h.update(LEAF_TAG);
    h.update(&COMMITMENT_VERSION.to_le_bytes());
    h.update(&leaf_id.0.to_le_bytes());
    h.update(&epoch.to_le_bytes());
    h.update(&pubkey_hash);

    h.finalize()
}

/// Compute a hash of a policy for use in commitments.
///
/// Policy hashes are used in branch commitments to bind the authorization
/// policy into the tree structure.
///
/// # Format
///
/// The policy is first serialized using a canonical binary format,
/// then hashed with BLAKE3.
///
/// # Examples
///
/// ```
/// use aura_core::tree::{policy_hash, Policy};
///
/// let policy = Policy::Threshold { m: 2, n: 3 };
/// let hash = policy_hash(&policy);
/// assert_eq!(hash.len(), 32);
///
/// // Same policy produces same hash (determinism)
/// let hash2 = policy_hash(&policy);
/// assert_eq!(hash, hash2);
/// ```
pub fn policy_hash(policy: &Policy) -> TreeHash32 {
    // Serialize policy to canonical format for hashing
    // We use a simple encoding scheme:
    // - Any: [0x00]
    // - Threshold{m,n}: [0x01, m (u16 LE), n (u16 LE)]
    // - All: [0x02]

    let mut bytes = Vec::with_capacity(5);
    match policy {
        Policy::Any => {
            bytes.push(0x00);
        }
        Policy::Threshold { m, n } => {
            bytes.push(0x01);
            bytes.extend_from_slice(&m.to_le_bytes());
            bytes.extend_from_slice(&n.to_le_bytes());
        }
        Policy::All => {
            bytes.push(0x02);
        }
    }

    hash::hash(&bytes)
}

/// Compute the root commitment for an entire tree.
///
/// This is a convenience wrapper around the branch commitment for the root node.
pub fn compute_root_commitment(
    root_index: NodeIndex,
    epoch: Epoch,
    policy: &Policy,
    left: &TreeHash32,
    right: &TreeHash32,
) -> TreeCommitment {
    TreeCommitment(commit_branch(root_index, epoch, policy, left, right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{LeafId, Policy};

    #[test]
    fn test_commit_branch_deterministic() {
        let policy = Policy::Threshold { m: 2, n: 3 };
        let node_idx = NodeIndex(1);
        let epoch = 42;
        let left = [0u8; 32];
        let right = [1u8; 32];

        let c1 = commit_branch(node_idx, epoch, &policy, &left, &right);
        let c2 = commit_branch(node_idx, epoch, &policy, &left, &right);

        assert_eq!(c1, c2, "Branch commitment should be deterministic");
    }

    #[test]
    fn test_commit_branch_changes_with_inputs() {
        let policy = Policy::Threshold { m: 2, n: 3 };
        let node_idx = NodeIndex(1);
        let epoch = 42;
        let left = [0u8; 32];
        let right = [1u8; 32];

        let base = commit_branch(node_idx, epoch, &policy, &left, &right);

        // Different epoch
        let diff_epoch = commit_branch(node_idx, 43, &policy, &left, &right);
        assert_ne!(base, diff_epoch, "Different epoch should change commitment");

        // Different policy
        let diff_policy = commit_branch(node_idx, epoch, &Policy::All, &left, &right);
        assert_ne!(
            base, diff_policy,
            "Different policy should change commitment"
        );

        // Different left child
        let mut left2 = left;
        left2[0] = 1;
        let diff_left = commit_branch(node_idx, epoch, &policy, &left2, &right);
        assert_ne!(
            base, diff_left,
            "Different left child should change commitment"
        );

        // Different right child
        let mut right2 = right;
        right2[0] = 2;
        let diff_right = commit_branch(node_idx, epoch, &policy, &left, &right2);
        assert_ne!(
            base, diff_right,
            "Different right child should change commitment"
        );
    }

    #[test]
    fn test_commit_leaf_deterministic() {
        let leaf_id = LeafId(5);
        let epoch = 42;
        let pubkey = vec![1, 2, 3, 4];

        let c1 = commit_leaf(leaf_id, epoch, &pubkey);
        let c2 = commit_leaf(leaf_id, epoch, &pubkey);

        assert_eq!(c1, c2, "Leaf commitment should be deterministic");
    }

    #[test]
    fn test_commit_leaf_changes_with_inputs() {
        let leaf_id = LeafId(5);
        let epoch = 42;
        let pubkey = vec![1, 2, 3, 4];

        let base = commit_leaf(leaf_id, epoch, &pubkey);

        // Different epoch
        let diff_epoch = commit_leaf(leaf_id, 43, &pubkey);
        assert_ne!(base, diff_epoch, "Different epoch should change commitment");

        // Different leaf ID
        let diff_id = commit_leaf(LeafId(6), epoch, &pubkey);
        assert_ne!(base, diff_id, "Different leaf ID should change commitment");

        // Different public key
        let diff_pubkey = commit_leaf(leaf_id, epoch, &[5, 6, 7, 8]);
        assert_ne!(
            base, diff_pubkey,
            "Different pubkey should change commitment"
        );
    }

    #[test]
    fn test_policy_hash_deterministic() {
        let policy = Policy::Threshold { m: 2, n: 3 };

        let h1 = policy_hash(&policy);
        let h2 = policy_hash(&policy);

        assert_eq!(h1, h2, "Policy hash should be deterministic");
    }

    #[test]
    fn test_policy_hash_different_policies() {
        let any_hash = policy_hash(&Policy::Any);
        let threshold_hash = policy_hash(&Policy::Threshold { m: 2, n: 3 });
        let all_hash = policy_hash(&Policy::All);

        assert_ne!(any_hash, threshold_hash);
        assert_ne!(any_hash, all_hash);
        assert_ne!(threshold_hash, all_hash);
    }

    #[test]
    fn test_policy_hash_different_thresholds() {
        let h1 = policy_hash(&Policy::Threshold { m: 2, n: 3 });
        let h2 = policy_hash(&Policy::Threshold { m: 3, n: 3 });
        let h3 = policy_hash(&Policy::Threshold { m: 2, n: 5 });

        assert_ne!(h1, h2, "Different m should produce different hash");
        assert_ne!(h1, h3, "Different n should produce different hash");
        assert_ne!(h2, h3);
    }

    #[test]
    fn test_compute_root_commitment() {
        let root_idx = NodeIndex(0);
        let epoch = 100;
        let policy = Policy::Threshold { m: 2, n: 3 };
        let left = [0xAA; 32];
        let right = [0xBB; 32];

        let root_commit = compute_root_commitment(root_idx, epoch, &policy, &left, &right);
        let manual_commit = commit_branch(root_idx, epoch, &policy, &left, &right);

        assert_eq!(root_commit.0, manual_commit);
    }
}
