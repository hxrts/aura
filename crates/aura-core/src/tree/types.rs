//! Core Tree Data Types
//!
//! Fundamental data structures for the commitment tree, following the specification
//! in `docs/123_commitment_tree.md`.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Monotonically increasing epoch counter for key rotation and replay prevention.
///
/// Each tree operation is bound to a parent epoch. After operations are applied,
/// the epoch may advance, invalidating old signing shares and preventing replay.
pub type Epoch = u64;

/// 32-byte cryptographic hash for tree commitments.
///
/// Used for:
/// - Branch commitments (hash of policy + child commitments)
/// - Leaf commitments (hash of public key)
/// - Parent binding in operations
pub type TreeHash32 = [u8; 32];

/// Unique identifier for a leaf node in the tree.
///
/// Leaf IDs are stable across tree modifications and epoch rotations.
/// They identify devices or guardians permanently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct LeafId(pub u32);

impl fmt::Display for LeafId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Leaf#{}", self.0)
    }
}

/// Index of a node in the tree (both branch and leaf nodes).
///
/// Nodes are indexed in a binary tree structure. Children are ordered
/// by NodeIndex for deterministic commitment calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct NodeIndex(pub u32);

impl fmt::Display for NodeIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node#{}", self.0)
    }
}

/// Role of a leaf node in the authentication tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LeafRole {
    /// A device owned and controlled by the identity
    Device,
    /// A guardian trusted to help with recovery
    Guardian,
}

/// Leaf node representing a device or guardian.
///
/// Leaves contain the public key material and metadata needed for
/// threshold operations. The actual signing shares are derived off-chain
/// and never stored in the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeafNode {
    /// Unique identifier for this leaf
    pub leaf_id: LeafId,

    /// Device identifier for this leaf
    pub device_id: crate::types::identifiers::DeviceId,

    /// Role (device or guardian)
    pub role: LeafRole,

    /// Serialized FROST key package or public key
    /// Format is opaque to the tree layer
    pub public_key: Vec<u8>,

    /// Optional opaque metadata (device name, guardian info, etc.)
    pub meta: Vec<u8>,
}

impl LeafNode {
    /// Create a new device leaf node
    pub fn new_device(
        leaf_id: LeafId,
        device_id: crate::types::identifiers::DeviceId,
        public_key: Vec<u8>,
    ) -> Self {
        Self {
            leaf_id,
            device_id,
            role: LeafRole::Device,
            public_key,
            meta: Vec::new(),
        }
    }

    /// Create a new guardian leaf node
    pub fn new_guardian(
        leaf_id: LeafId,
        device_id: crate::types::identifiers::DeviceId,
        public_key: Vec<u8>,
    ) -> Self {
        Self {
            leaf_id,
            device_id,
            role: LeafRole::Guardian,
            public_key,
            meta: Vec::new(),
        }
    }

    /// Create a leaf node with metadata
    pub fn with_meta(mut self, meta: Vec<u8>) -> Self {
        self.meta = meta;
        self
    }
}

/// Type of node in the tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    /// Leaf node (device or guardian)
    Leaf(LeafNode),
    /// Branch node (internal)
    Branch,
}

/// Branch node with policy and cryptographic commitment.
///
/// Branch nodes represent internal tree structure and define the
/// threshold policy required for operations under that subtree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchNode {
    /// Index of this branch node
    pub node: NodeIndex,

    /// Threshold policy for this subtree
    pub policy: super::Policy,

    /// Cryptographic commitment to this branch's structure
    /// Computed as: H("BRANCH", version, node_index, epoch, policy_hash, left, right)
    pub commitment: TreeHash32,
}

/// Root commitment identifying the entire tree state.
///
/// This commitment changes whenever any part of the tree structure,
/// policies, or leaf keys change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct TreeCommitment(pub TreeHash32);

impl TreeCommitment {
    /// Create a tree commitment from a hash
    pub fn from_hash(hash: TreeHash32) -> Self {
        Self(hash)
    }

    /// Get the underlying hash
    pub fn as_hash(&self) -> &TreeHash32 {
        &self.0
    }
}

impl fmt::Display for TreeCommitment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TreeCommit({})", hex::encode(&self.0[..8]))
    }
}

/// Kind of tree modification operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TreeOpKind {
    /// Add a new leaf (device or guardian) under a branch
    AddLeaf {
        /// The leaf to add
        leaf: LeafNode,
        /// Parent branch node index
        under: NodeIndex,
    },

    /// Remove a leaf from the tree
    RemoveLeaf {
        /// Leaf to remove
        leaf: LeafId,
        /// Reason code (0 = revoked, 1 = lost, 2 = compromised, etc.)
        reason: u8,
    },

    /// Change the threshold policy of a branch node
    ChangePolicy {
        /// Node to update
        node: NodeIndex,
        /// New policy (must be stricter or equal via meet-semilattice)
        new_policy: super::Policy,
    },

    /// Rotate epoch and refresh key material
    RotateEpoch {
        /// Hint of affected node indices (for efficiency, not validated)
        affected: Vec<NodeIndex>,
    },
}

/// Tree modification operation with parent binding.
///
/// Operations reference their parent state by (epoch, commitment) to prevent
/// replay attacks and ensure lineage. The version field enables protocol upgrades.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeOp {
    /// Epoch of the parent state this operation modifies
    pub parent_epoch: Epoch,

    /// Commitment of the parent state (replay prevention)
    pub parent_commitment: TreeHash32,

    /// The actual tree modification
    pub op: TreeOpKind,

    /// Protocol version for upgrade safety
    pub version: u16,
}

/// Tree operation with threshold signature attestation.
///
/// This is the only form of tree operation stored in the journal.
/// The aggregate signature proves that at least m-of-n signers approved
/// this operation under the policy active at the parent state.
///
/// **Privacy**: The journal stores only the signer count, not individual
/// signer identities. The aggregate signature is verifiable against the
/// group public key committed in the parent tree state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestedOp {
    /// The tree operation
    pub op: TreeOp,

    /// FROST aggregate signature over the operation
    pub agg_sig: Vec<u8>,

    /// Number of signers who contributed (for threshold verification)
    /// This reveals cardinality only, not individual identities
    pub signer_count: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leaf_id_display() {
        let id = LeafId(42);
        assert_eq!(format!("{}", id), "Leaf#42");
    }

    #[test]
    fn test_node_index_display() {
        let idx = NodeIndex(7);
        assert_eq!(format!("{}", idx), "Node#7");
    }

    #[test]
    fn test_tree_commitment_display() {
        let hash = [0u8; 32];
        let commit = TreeCommitment(hash);
        assert_eq!(format!("{}", commit), "TreeCommit(0000000000000000)");
    }

    #[test]
    fn test_leaf_id_ordering() {
        let id1 = LeafId(1);
        let id2 = LeafId(2);
        assert!(id1 < id2);
    }

    #[test]
    fn test_node_index_ordering() {
        let idx1 = NodeIndex(5);
        let idx2 = NodeIndex(10);
        assert!(idx1 < idx2);
    }
}
