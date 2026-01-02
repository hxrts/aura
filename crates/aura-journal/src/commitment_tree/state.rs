//! TreeState - Materialized Tree View
//!
//! TreeState represents the materialized tree at a specific epoch, derived
//! from the OpLog via the reduction function. It is **NEVER** stored directly.
//!
//! ## Key Design (from docs/123_commitment_tree.md):
//!
//! - **Derived State**: Computed on-demand from OpLog
//! - **Snapshot in Time**: Represents tree at a specific epoch
//! - **Navigation Helpers**: Query nodes, leaves, paths, policies
//! - **Commitment Verification**: Validate tree integrity

use aura_core::{
    tree::{
        BranchNode, BranchSigningKey, Epoch, LeafNode, SigningWitness, TreeHash32, TreeStateView,
    },
    LeafId, NodeIndex, Policy, PolicyError,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Tree topology structure for tracking parent-child relationships
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TreeTopology {
    /// Parent pointers: node -> parent node
    parent_pointers: BTreeMap<NodeIndex, NodeIndex>,

    /// Children pointers: parent -> set of child nodes
    children_pointers: BTreeMap<NodeIndex, BTreeSet<NodeIndex>>,

    /// Leaf placements: leaf -> parent branch node
    leaf_parents: BTreeMap<LeafId, NodeIndex>,

    /// Root node index (if exists)
    root_node: Option<NodeIndex>,
}

impl TreeTopology {
    /// Creates a new empty tree topology
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a leaf under a branch node
    pub fn add_leaf(&mut self, leaf_id: LeafId, parent: NodeIndex) {
        self.leaf_parents.insert(leaf_id, parent);
    }

    /// Remove a leaf
    pub fn remove_leaf(&mut self, leaf_id: &LeafId) -> Option<NodeIndex> {
        self.leaf_parents.remove(leaf_id)
    }

    /// Add a branch node as child of another branch
    pub fn add_branch(&mut self, child: NodeIndex, parent: Option<NodeIndex>) {
        if let Some(parent_node) = parent {
            self.parent_pointers.insert(child, parent_node);
            self.children_pointers
                .entry(parent_node)
                .or_default()
                .insert(child);
        } else {
            // This is the root node
            self.root_node = Some(child);
        }
    }

    /// Get parent of a node
    pub fn get_parent(&self, node: NodeIndex) -> Option<NodeIndex> {
        self.parent_pointers.get(&node).copied()
    }

    /// Get children of a node
    pub fn get_children(&self, node: NodeIndex) -> BTreeSet<NodeIndex> {
        self.children_pointers
            .get(&node)
            .cloned()
            .unwrap_or_default()
    }

    /// Get parent of a leaf
    pub fn get_leaf_parent(&self, leaf_id: LeafId) -> Option<NodeIndex> {
        self.leaf_parents.get(&leaf_id).copied()
    }

    /// Get path from node to root
    pub fn get_path_to_root(&self, start: NodeIndex) -> Vec<NodeIndex> {
        let mut path = vec![start];
        let mut current = start;

        while let Some(parent) = self.get_parent(current) {
            path.push(parent);
            current = parent;
        }

        path
    }

    /// Get path from leaf to root
    pub fn get_leaf_path_to_root(&self, leaf_id: LeafId) -> Vec<NodeIndex> {
        if let Some(parent) = self.get_leaf_parent(leaf_id) {
            self.get_path_to_root(parent)
        } else {
            Vec::new()
        }
    }
}

/// Materialized tree state at a specific epoch
///
/// **CRITICAL**: This is **derived** state, computed on-demand from OpLog.
/// TreeState is NEVER stored directly in the journal.
///
/// The TreeState represents the result of reducing all operations in the OpLog
/// up to a specific epoch. It provides a convenient view for querying the tree
/// structure, membership, and policies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeState {
    /// Current epoch (monotonically increasing)
    pub epoch: Epoch,

    /// Root commitment (hash of entire tree structure)
    pub root_commitment: TreeHash32,

    /// Branch nodes indexed by node index
    pub branches: BTreeMap<NodeIndex, BranchNode>,

    /// Leaf nodes indexed by leaf ID
    pub leaves: BTreeMap<LeafId, LeafNode>,

    /// Leaf commitments indexed by leaf ID
    /// Stored separately for efficient commitment recomputation
    leaf_commitments: BTreeMap<LeafId, TreeHash32>,

    /// Tree topology: tracks parent-child relationships for efficient navigation
    /// This enables parent pointer navigation and affected node computation
    tree_topology: TreeTopology,

    /// Branch signing keys for threshold signature verification
    ///
    /// Each branch node has a group public key established via DKG.
    /// The threshold is derived from the branch's Policy, not stored here.
    /// Keys are updated when membership changes or DKG is re-run.
    branch_signing_keys: BTreeMap<NodeIndex, BranchSigningKey>,
}

impl TreeState {
    /// Create a new empty tree state at epoch 0
    pub fn new() -> Self {
        Self {
            epoch: Epoch::initial(),
            root_commitment: [0u8; 32],
            branches: BTreeMap::new(),
            leaves: BTreeMap::new(),
            leaf_commitments: BTreeMap::new(),
            tree_topology: TreeTopology::new(),
            branch_signing_keys: BTreeMap::new(),
        }
    }

    /// Create a tree state with specific parameters
    pub fn with_params(
        epoch: Epoch,
        root_commitment: TreeHash32,
        branches: BTreeMap<NodeIndex, BranchNode>,
        leaves: BTreeMap<LeafId, LeafNode>,
    ) -> Self {
        Self {
            epoch,
            root_commitment,
            branches,
            leaves,
            leaf_commitments: BTreeMap::new(),
            tree_topology: TreeTopology::new(),
            branch_signing_keys: BTreeMap::new(),
        }
    }

    // ===== Navigation Helpers =====

    /// Get a branch node by index
    pub fn get_branch(&self, index: &NodeIndex) -> Option<&BranchNode> {
        self.branches.get(index)
    }

    /// Get a leaf node by ID
    pub fn get_leaf(&self, id: &LeafId) -> Option<&LeafNode> {
        self.leaves.get(id)
    }

    /// Get the policy at a node
    pub fn get_policy(&self, index: &NodeIndex) -> Option<&Policy> {
        self.branches.get(index).map(|b| &b.policy)
    }

    /// List all leaf IDs
    pub fn list_leaf_ids(&self) -> Vec<LeafId> {
        self.leaves.keys().copied().collect()
    }

    /// List all branch indices
    pub fn list_branch_indices(&self) -> Vec<NodeIndex> {
        self.branches.keys().copied().collect()
    }

    /// Get the number of leaves in the tree
    pub fn num_leaves(&self) -> usize {
        self.leaves.len()
    }

    /// Get the number of branches in the tree
    pub fn num_branches(&self) -> usize {
        self.branches.len()
    }

    /// Check if tree is empty
    pub fn is_empty(&self) -> bool {
        self.leaves.is_empty()
    }

    // ===== Tree Navigation =====

    /// Get parent of a branch node
    pub fn get_parent(&self, node: NodeIndex) -> Option<NodeIndex> {
        self.tree_topology.get_parent(node)
    }

    /// Get children of a branch node
    pub fn get_children(&self, node: NodeIndex) -> BTreeSet<NodeIndex> {
        self.tree_topology.get_children(node)
    }

    /// Get parent branch of a leaf
    pub fn get_leaf_parent(&self, leaf_id: LeafId) -> Option<NodeIndex> {
        self.tree_topology.get_leaf_parent(leaf_id)
    }

    /// Get path from node to root
    pub fn get_path_to_root(&self, node: NodeIndex) -> Vec<NodeIndex> {
        self.tree_topology.get_path_to_root(node)
    }

    /// Get path from leaf to root
    pub fn get_leaf_path_to_root(&self, leaf_id: LeafId) -> Vec<NodeIndex> {
        self.tree_topology.get_leaf_path_to_root(leaf_id)
    }

    /// Get all nodes on paths from given nodes to root
    /// This is used for computing affected nodes efficiently
    pub fn get_affected_paths(
        &self,
        nodes: &[NodeIndex],
        leaves: &[LeafId],
    ) -> BTreeSet<NodeIndex> {
        let mut affected = BTreeSet::new();

        // Add paths for branch nodes
        for &node in nodes {
            affected.extend(self.get_path_to_root(node));
        }

        // Add paths for leaves
        for &leaf in leaves {
            affected.extend(self.get_leaf_path_to_root(leaf));
        }

        affected
    }

    // ===== Commitment Verification =====

    /// Get the current root commitment
    ///
    /// This is the commitment that binds the entire tree structure.
    /// All operations reference their parent by (epoch, commitment).
    pub fn current_commitment(&self) -> TreeHash32 {
        self.root_commitment
    }

    /// Verify that a commitment matches the current root
    pub fn verify_commitment(&self, commitment: &TreeHash32) -> bool {
        &self.root_commitment == commitment
    }

    // ===== Tree Properties =====

    /// Get the current epoch
    pub fn current_epoch(&self) -> Epoch {
        self.epoch
    }

    /// Increment the epoch (used during reduction)
    pub fn increment_epoch(&mut self) -> Result<(), aura_core::AuraError> {
        self.epoch = self.epoch.next()?;
        Ok(())
    }

    /// Set the root commitment (used during reduction)
    pub fn set_root_commitment(&mut self, commitment: TreeHash32) {
        self.root_commitment = commitment;
    }

    // ===== Mutation Operations (used by reduction) =====

    /// Add a leaf node to the tree
    ///
    /// This is used internally by the reduction function.
    pub fn add_leaf(&mut self, leaf: LeafNode) {
        self.leaves.insert(leaf.leaf_id, leaf);
    }

    /// Add a leaf node to the tree under a specific branch
    ///
    /// This is used internally by the reduction function for AddLeaf operations.
    pub fn add_leaf_under(&mut self, leaf: LeafNode, parent: NodeIndex) {
        let leaf_id = leaf.leaf_id;
        self.leaves.insert(leaf_id, leaf);
        self.tree_topology.add_leaf(leaf_id, parent);
    }

    /// Remove a leaf node from the tree
    ///
    /// This is used internally by the reduction function.
    pub fn remove_leaf(&mut self, id: &LeafId) -> Option<LeafNode> {
        // Remove from topology first to get the parent
        let _parent = self.tree_topology.remove_leaf(id);
        // Remove the actual leaf node
        let leaf = self.leaves.remove(id);
        // Remove leaf commitment
        self.leaf_commitments.remove(id);
        leaf
    }

    /// Get the affected parent when removing a leaf
    ///
    /// This returns the parent node that will need commitment recomputation.
    pub fn get_remove_leaf_affected_parent(&self, id: &LeafId) -> Option<NodeIndex> {
        self.tree_topology.get_leaf_parent(*id)
    }

    /// Add a branch node to the tree
    ///
    /// This is used internally by the reduction function.
    pub fn add_branch(&mut self, branch: BranchNode) {
        self.branches.insert(branch.node, branch);
    }

    /// Add a branch node with parent tracking
    ///
    /// This is used internally by the reduction function.
    pub fn add_branch_with_parent(&mut self, branch: BranchNode, parent: Option<NodeIndex>) {
        let node_index = branch.node;
        self.branches.insert(node_index, branch);
        self.tree_topology.add_branch(node_index, parent);
    }

    /// Update a branch node's policy
    ///
    /// This is used internally by the reduction function.
    pub fn update_branch_policy(
        &mut self,
        index: &NodeIndex,
        policy: Policy,
    ) -> Result<(), TreeStateError> {
        self.branches
            .get_mut(index)
            .map(|b| b.policy = policy)
            .ok_or(TreeStateError::BranchNotFound(*index))
    }

    /// Set the commitment for a leaf node
    ///
    /// This is used internally by the reduction function when applying AddLeaf operations.
    pub fn set_leaf_commitment(&mut self, leaf_id: LeafId, commitment: TreeHash32) {
        self.leaf_commitments.insert(leaf_id, commitment);
    }

    /// Get the commitment for a leaf node
    pub fn get_leaf_commitment(&self, leaf_id: &LeafId) -> Option<&TreeHash32> {
        self.leaf_commitments.get(leaf_id)
    }

    /// Iterate over all leaf commitments (sorted by LeafId)
    ///
    /// Returns an iterator of (LeafId, &TreeHash32) pairs for deterministic commitment computation.
    pub fn iter_leaf_commitments(&self) -> impl Iterator<Item = (LeafId, &TreeHash32)> {
        self.leaf_commitments.iter().map(|(id, c)| (*id, c))
    }

    /// Iterate over all branches (sorted by NodeIndex)
    ///
    /// Returns an iterator of branch nodes for deterministic commitment computation.
    pub fn iter_branches(&self) -> impl Iterator<Item = &BranchNode> {
        self.branches.values()
    }

    // ===== Additional Mutation Methods (for application.rs) =====

    /// Insert a leaf node (alias for add_leaf for consistency)
    pub fn insert_leaf(&mut self, leaf: LeafNode) {
        self.add_leaf(leaf);
    }

    /// Set epoch to a specific value
    pub fn set_epoch(&mut self, epoch: Epoch) {
        self.epoch = epoch;
    }

    /// Set policy for a branch node
    pub fn set_policy(&mut self, index: NodeIndex, policy: Policy) {
        if let Some(branch) = self.branches.get_mut(&index) {
            branch.policy = policy;
        }
    }

    // ===== Signing Key Operations =====

    /// Get the signing key for a branch node
    ///
    /// Returns the group public key used for verifying threshold signatures
    /// on operations at this branch.
    pub fn get_signing_key(&self, index: &NodeIndex) -> Option<&BranchSigningKey> {
        self.branch_signing_keys.get(index)
    }

    /// Set the signing key for a branch node
    ///
    /// Called after DKG completes to store the group public key.
    pub fn set_signing_key(&mut self, index: NodeIndex, key: BranchSigningKey) {
        self.branch_signing_keys.insert(index, key);
    }

    /// Remove the signing key for a branch node
    ///
    /// Called when a branch is removed from the tree.
    pub fn remove_signing_key(&mut self, index: &NodeIndex) -> Option<BranchSigningKey> {
        self.branch_signing_keys.remove(index)
    }

    /// Check if a branch has a signing key
    pub fn has_signing_key(&self, index: &NodeIndex) -> bool {
        self.branch_signing_keys.contains_key(index)
    }

    /// Get all signing keys
    pub fn signing_keys(&self) -> &BTreeMap<NodeIndex, BranchSigningKey> {
        &self.branch_signing_keys
    }

    /// Get the signing witness for a node (key + threshold derived from policy)
    ///
    /// This is the complete information needed to verify an operation at this node.
    pub fn signing_witness(&self, index: &NodeIndex) -> Result<SigningWitness, TreeStateError> {
        let key = self
            .branch_signing_keys
            .get(index)
            .ok_or(TreeStateError::SigningKeyNotFound(*index))?;
        let policy = self
            .get_policy(index)
            .ok_or(TreeStateError::PolicyNotFound(*index))?;
        let child_count = self.get_children(*index).len();
        let threshold = policy
            .required_signers(child_count)
            .map_err(|e| TreeStateError::InvalidPolicy {
                node: *index,
                source: e,
            })?;

        Ok(SigningWitness::from_signing_key(key, threshold))
    }
}

/// Implementation of TreeStateView for TreeState
///
/// This allows TreeState to be used with the verification module from aura-core.
impl TreeStateView for TreeState {
    fn get_signing_key(&self, node: NodeIndex) -> Option<&BranchSigningKey> {
        self.branch_signing_keys.get(&node)
    }

    fn get_policy(&self, node: NodeIndex) -> Option<&Policy> {
        self.branches.get(&node).map(|b| &b.policy)
    }

    fn child_count(&self, node: NodeIndex) -> usize {
        self.get_children(node).len()
    }

    fn current_epoch(&self) -> Epoch {
        self.epoch
    }

    fn current_commitment(&self) -> TreeHash32 {
        self.root_commitment
    }
}

impl Default for TreeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur during TreeState operations
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TreeStateError {
    /// Branch node not found
    #[error("Branch not found at index {0:?}")]
    BranchNotFound(NodeIndex),

    /// Leaf node not found
    #[error("Leaf not found with ID {0:?}")]
    LeafNotFound(LeafId),

    /// Invalid tree structure
    #[error("Invalid tree structure: {0}")]
    InvalidStructure(String),

    /// Commitment mismatch
    #[error("Commitment mismatch: expected {expected:?}, got {actual:?}")]
    CommitmentMismatch {
        /// The expected commitment hash
        expected: TreeHash32,
        /// The actual commitment hash
        actual: TreeHash32,
    },

    /// Signing key not found for node
    #[error("Signing key not found for node {0:?}")]
    SigningKeyNotFound(NodeIndex),

    /// Policy not found for node
    #[error("Policy not found for node {0:?}")]
    PolicyNotFound(NodeIndex),

    /// Invalid policy for node
    #[error("Invalid policy for node {node:?}: {source}")]
    InvalidPolicy {
        /// Node with invalid policy
        node: NodeIndex,
        /// Underlying policy error
        source: PolicyError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tree() {
        let state = TreeState::new();
        assert_eq!(state.epoch, Epoch::initial());
        assert!(state.is_empty());
        assert_eq!(state.num_leaves(), 0);
        assert_eq!(state.num_branches(), 0);
    }

    #[test]
    fn test_add_leaf() {
        let mut state = TreeState::new();

        let leaf = LeafNode::new_device(
            LeafId(1),
            aura_core::DeviceId(uuid::Uuid::from_bytes([11u8; 16])),
            vec![0u8; 32],
        )
        .expect("valid leaf");

        state.add_leaf(leaf.clone());

        assert_eq!(state.num_leaves(), 1);
        assert_eq!(state.get_leaf(&LeafId(1)), Some(&leaf));
    }

    #[test]
    fn test_remove_leaf() {
        let mut state = TreeState::new();

        let leaf = LeafNode::new_device(
            LeafId(1),
            aura_core::DeviceId(uuid::Uuid::from_bytes([12u8; 16])),
            vec![0u8; 32],
        )
        .expect("valid leaf");

        state.add_leaf(leaf.clone());
        assert_eq!(state.num_leaves(), 1);

        let removed = state.remove_leaf(&LeafId(1));
        assert_eq!(removed, Some(leaf));
        assert_eq!(state.num_leaves(), 0);
    }

    #[test]
    fn test_add_branch() {
        let mut state = TreeState::new();

        let branch = BranchNode {
            node: NodeIndex(1),
            policy: Policy::All,
            commitment: [0u8; 32],
        };

        state.add_branch(branch.clone());

        assert_eq!(state.num_branches(), 1);
        assert_eq!(state.get_branch(&NodeIndex(1)), Some(&branch));
        assert_eq!(state.get_policy(&NodeIndex(1)), Some(&Policy::All));
    }

    #[test]
    fn test_update_branch_policy() {
        let mut state = TreeState::new();

        let branch = BranchNode {
            node: NodeIndex(1),
            policy: Policy::All,
            commitment: [0u8; 32],
        };

        state.add_branch(branch);

        let result = state.update_branch_policy(&NodeIndex(1), Policy::Any);
        assert!(result.is_ok());
        assert_eq!(state.get_policy(&NodeIndex(1)), Some(&Policy::Any));
    }

    #[test]
    fn test_commitment_verification() {
        let commitment = [42u8; 32];
        let mut state = TreeState::new();
        state.set_root_commitment(commitment);

        assert_eq!(state.current_commitment(), commitment);
        assert!(state.verify_commitment(&commitment));
        assert!(!state.verify_commitment(&[0u8; 32]));
    }

    #[test]
    fn test_epoch_increment() {
        let mut state = TreeState::new();
        assert_eq!(state.current_epoch(), Epoch::initial());

        state.increment_epoch().unwrap();
        assert_eq!(state.current_epoch(), Epoch::new(1));

        state.increment_epoch().unwrap();
        assert_eq!(state.current_epoch(), Epoch::new(2));
    }
}
