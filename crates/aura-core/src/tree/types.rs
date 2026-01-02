//! Core Tree Data Types
//!
//! Fundamental data structures for the commitment tree, following the specification
//! in `docs/123_commitment_tree.md`.

/// Maximum size for leaf public keys in bytes (Ed25519 FROST keys).
pub const MAX_LEAF_PUBLIC_KEY_BYTES: usize = 64;

/// Maximum size for leaf metadata in bytes.
pub const MAX_LEAF_META_BYTES: usize = 256;

/// Maximum size for aggregate signatures in bytes (FROST aggregate).
pub const MAX_AGG_SIG_BYTES: usize = 128;

use crate::AuraError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;

/// Monotonically increasing epoch counter for key rotation and replay prevention.
///
/// Each tree operation is bound to a parent epoch. After operations are applied,
/// the epoch may advance, invalidating old signing shares and preventing replay.
pub use crate::types::Epoch;

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

/// Validated public key bytes for a leaf node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeafPublicKey(#[serde(with = "serde_bytes")] Vec<u8>);

impl LeafPublicKey {
    pub fn try_new(bytes: Vec<u8>) -> Result<Self, AuraError> {
        if bytes.len() > MAX_LEAF_PUBLIC_KEY_BYTES {
            return Err(AuraError::invalid(format!(
                "Leaf public key exceeded MAX_LEAF_PUBLIC_KEY_BYTES ({MAX_LEAF_PUBLIC_KEY_BYTES})"
            )));
        }
        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl TryFrom<Vec<u8>> for LeafPublicKey {
    type Error = AuraError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl TryFrom<&[u8]> for LeafPublicKey {
    type Error = AuraError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::try_new(value.to_vec())
    }
}

impl AsRef<[u8]> for LeafPublicKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Deref for LeafPublicKey {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<LeafPublicKey> for Vec<u8> {
    fn from(value: LeafPublicKey) -> Self {
        value.0
    }
}

impl From<&LeafPublicKey> for Vec<u8> {
    fn from(value: &LeafPublicKey) -> Self {
        value.0.clone()
    }
}

/// Validated metadata bytes for a leaf node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeafMetadata(#[serde(with = "serde_bytes")] Vec<u8>);

impl LeafMetadata {
    pub fn try_new(bytes: Vec<u8>) -> Result<Self, AuraError> {
        if bytes.len() > MAX_LEAF_META_BYTES {
            return Err(AuraError::invalid(format!(
                "Leaf metadata exceeded MAX_LEAF_META_BYTES ({MAX_LEAF_META_BYTES})"
            )));
        }
        Ok(Self(bytes))
    }

    pub fn empty() -> Self {
        Self(Vec::new())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl TryFrom<Vec<u8>> for LeafMetadata {
    type Error = AuraError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl TryFrom<&[u8]> for LeafMetadata {
    type Error = AuraError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::try_new(value.to_vec())
    }
}

impl AsRef<[u8]> for LeafMetadata {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Deref for LeafMetadata {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<LeafMetadata> for Vec<u8> {
    fn from(value: LeafMetadata) -> Self {
        value.0
    }
}

impl From<&LeafMetadata> for Vec<u8> {
    fn from(value: &LeafMetadata) -> Self {
        value.0.clone()
    }
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
    pub public_key: LeafPublicKey,

    /// Optional opaque metadata (device name, guardian info, etc.)
    pub meta: LeafMetadata,
}

impl LeafNode {
    /// Create a new leaf node with explicit role.
    pub fn new(
        leaf_id: LeafId,
        device_id: crate::types::identifiers::DeviceId,
        role: LeafRole,
        public_key: impl TryInto<LeafPublicKey, Error = AuraError>,
        meta: LeafMetadata,
    ) -> Result<Self, AuraError> {
        Ok(Self {
            leaf_id,
            device_id,
            role,
            public_key: public_key.try_into()?,
            meta,
        })
    }

    /// Create a new device leaf node
    pub fn new_device(
        leaf_id: LeafId,
        device_id: crate::types::identifiers::DeviceId,
        public_key: impl TryInto<LeafPublicKey, Error = AuraError>,
    ) -> Result<Self, AuraError> {
        Self::new(
            leaf_id,
            device_id,
            LeafRole::Device,
            public_key,
            LeafMetadata::empty(),
        )
    }

    /// Create a new guardian leaf node
    pub fn new_guardian(
        leaf_id: LeafId,
        device_id: crate::types::identifiers::DeviceId,
        public_key: impl TryInto<LeafPublicKey, Error = AuraError>,
    ) -> Result<Self, AuraError> {
        Self::new(
            leaf_id,
            device_id,
            LeafRole::Guardian,
            public_key,
            LeafMetadata::empty(),
        )
    }

    /// Create a leaf node with metadata
    #[must_use]
    pub fn with_meta(mut self, meta: LeafMetadata) -> Self {
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
    #[must_use]
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

/// Unique identifier for a snapshot proposal.
///
/// Used to track approval progress for snapshot creation.
/// This is the canonical definition - other modules should import from here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProposalId(pub TreeHash32);

impl ProposalId {
    /// Create a new proposal ID from a hash
    #[must_use]
    pub fn new(hash: TreeHash32) -> Self {
        Self(hash)
    }

    /// Create a proposal ID from a Hash32
    #[must_use]
    pub fn from_hash32(hash: crate::Hash32) -> Self {
        Self(hash.0)
    }

    /// Convert to Hash32
    #[must_use]
    pub fn to_hash32(&self) -> crate::Hash32 {
        crate::Hash32(self.0)
    }

    /// Get the underlying bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Create a zero proposal ID (for testing)
    #[must_use]
    pub fn zero() -> Self {
        Self([0u8; 32])
    }
}

impl From<crate::Hash32> for ProposalId {
    fn from(hash: crate::Hash32) -> Self {
        Self(hash.0)
    }
}

impl From<ProposalId> for crate::Hash32 {
    fn from(id: ProposalId) -> Self {
        crate::Hash32(id.0)
    }
}

impl fmt::Display for ProposalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Proposal({})", hex::encode(&self.0[..8]))
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

/// Signing key configuration for a branch node.
///
/// Stores the group public key established via DKG for verifying aggregate
/// signatures on operations at this branch. The threshold is derived from
/// the branch's Policy, not stored redundantly here.
///
/// **Lifecycle**: Updated when:
/// - Branch is created (initial DKG)
/// - Membership changes under the branch (new DKG)
/// - Policy change affects signing group
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BranchSigningKey {
    /// Group public key for aggregate signature verification (32 bytes for Ed25519)
    pub group_public_key: [u8; 32],

    /// Epoch when this key was established via DKG
    pub key_epoch: Epoch,
}

impl BranchSigningKey {
    /// Create a new branch signing key
    #[must_use]
    pub fn new(group_public_key: [u8; 32], key_epoch: Epoch) -> Self {
        Self {
            group_public_key,
            key_epoch,
        }
    }

    /// Get the group public key bytes
    pub fn group_key(&self) -> &[u8; 32] {
        &self.group_public_key
    }

    /// Get the epoch when this key was established
    pub fn epoch(&self) -> Epoch {
        self.key_epoch
    }
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
///
/// **Security**: The binding message includes the group public key to prevent
/// signature reuse across different signing groups.
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
        assert_eq!(format!("{id}"), "Leaf#42");
    }

    #[test]
    fn test_node_index_display() {
        let idx = NodeIndex(7);
        assert_eq!(format!("{idx}"), "Node#7");
    }

    #[test]
    fn test_tree_commitment_display() {
        let hash = [0u8; 32];
        let commit = TreeCommitment(hash);
        assert_eq!(format!("{commit}"), "TreeCommit(0000000000000000)");
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

    #[test]
    fn test_leaf_public_key_bounds() {
        let ok = vec![0u8; MAX_LEAF_PUBLIC_KEY_BYTES];
        assert!(LeafPublicKey::try_new(ok).is_ok());

        let too_large = vec![0u8; MAX_LEAF_PUBLIC_KEY_BYTES + 1];
        assert!(LeafPublicKey::try_new(too_large).is_err());
    }

    #[test]
    fn test_leaf_metadata_bounds() {
        let ok = vec![1u8; MAX_LEAF_META_BYTES];
        assert!(LeafMetadata::try_new(ok).is_ok());

        let too_large = vec![1u8; MAX_LEAF_META_BYTES + 1];
        assert!(LeafMetadata::try_new(too_large).is_err());
    }

    #[test]
    fn test_epoch_conversions() {
        let epoch = Epoch::new(42);
        let raw: u64 = epoch.into();
        assert_eq!(raw, 42);
        assert_eq!(Epoch::from(raw), Epoch::new(42));
    }
}
