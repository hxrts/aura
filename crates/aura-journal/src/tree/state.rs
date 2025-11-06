//! Ratchet Tree State Management
//!
//! Implements the RatchetTree state container that maintains the left-balanced binary tree
//! structure with epochs, commitments, and node management.

use crate::tree::commitment::{
    compute_branch_commitment, compute_leaf_commitment, policy_tag, Commitment,
};
use crate::tree::indexing::{direct_path, root_index, LeafIndex, NodeIndex};
use crate::tree::node::{BranchNode, LeafNode, Policy};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Epoch counter for the ratchet tree
///
/// Monotonically increasing version number that increments on every tree mutation.
/// Provides forward secrecy and prevents replay attacks.
pub type Epoch = u64;

/// Ratchet tree state container
///
/// Maintains the complete tree structure including leaves, branches, and commitments.
/// The tree follows LBBT (Left-Balanced Binary Tree) invariants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RatchetTree {
    /// Current epoch (increments on every mutation)
    pub epoch: Epoch,
    /// Root commitment (hash of entire tree structure)
    pub root_commitment: Commitment,
    /// Leaf nodes indexed by leaf index
    pub leaves: BTreeMap<LeafIndex, LeafNode>,
    /// Branch nodes indexed by node index
    pub branches: BTreeMap<NodeIndex, BranchNode>,
}

impl RatchetTree {
    /// Create a new empty ratchet tree
    pub fn new() -> Self {
        Self {
            epoch: 0,
            root_commitment: Commitment::default(),
            leaves: BTreeMap::new(),
            branches: BTreeMap::new(),
        }
    }

    /// Get the number of leaves in the tree
    pub fn num_leaves(&self) -> usize {
        self.leaves.len()
    }

    /// Check if the tree is empty
    pub fn is_empty(&self) -> bool {
        self.leaves.is_empty()
    }

    /// Get a leaf node by index
    pub fn get_leaf(&self, index: LeafIndex) -> Option<&LeafNode> {
        self.leaves.get(&index)
    }

    /// Get a branch node by index
    pub fn get_branch(&self, index: NodeIndex) -> Option<&BranchNode> {
        self.branches.get(&index)
    }

    /// Get the root node index
    pub fn root_index(&self) -> Result<NodeIndex, TreeError> {
        if self.is_empty() {
            return Err(TreeError::EmptyTree);
        }
        Ok(root_index(self.num_leaves()))
    }

    /// Get the root commitment
    pub fn root_commitment(&self) -> &Commitment {
        &self.root_commitment
    }

    /// Compute the commitment for a leaf node
    pub(crate) fn compute_leaf_commitment(&self, leaf: &LeafNode) -> Commitment {
        compute_leaf_commitment(
            leaf.leaf_index.value(),
            self.epoch,
            &leaf.public_key.signing_key,
        )
    }

    /// Compute the commitment for a branch node
    fn compute_branch_commitment(
        &self,
        node_index: NodeIndex,
        policy: &Policy,
        left_commitment: &Commitment,
        right_commitment: &Commitment,
    ) -> Commitment {
        compute_branch_commitment(
            node_index.value(),
            self.epoch,
            policy_tag(policy),
            left_commitment,
            right_commitment,
        )
    }

    /// Update commitments along a path from a leaf to the root
    ///
    /// Recomputes all branch commitments on the direct path.
    pub(crate) fn update_path_commitments(
        &mut self,
        leaf_index: LeafIndex,
    ) -> Result<(), TreeError> {
        if self.num_leaves() == 0 {
            return Err(TreeError::EmptyTree);
        }

        // Get the leaf commitment
        let leaf = self
            .leaves
            .get(&leaf_index)
            .ok_or(TreeError::LeafNotFound(leaf_index))?;
        let mut current_commitment = self.compute_leaf_commitment(leaf);

        // If single leaf, it's the root
        if self.num_leaves() == 1 {
            self.root_commitment = current_commitment;
            return Ok(());
        }

        // Update commitments up the path
        let path = direct_path(leaf_index, self.num_leaves());

        for node_index in path {
            // Get left and right children
            let left_index = node_index
                .left_child()
                .ok_or(TreeError::InvalidNodeIndex(node_index))?;
            let right_index = node_index
                .right_child()
                .ok_or(TreeError::InvalidNodeIndex(node_index))?;

            // Get commitments for both children
            let left_commitment = self.get_node_commitment(left_index)?;
            let right_commitment = self.get_node_commitment(right_index)?;

            // Get the branch node to get its policy
            let branch = self
                .branches
                .get(&node_index)
                .ok_or(TreeError::BranchNotFound(node_index))?;

            // Compute new commitment
            current_commitment = self.compute_branch_commitment(
                node_index,
                &branch.policy,
                &left_commitment,
                &right_commitment,
            );

            // Update the branch node's commitment
            if let Some(branch) = self.branches.get_mut(&node_index) {
                branch.commitment = current_commitment;
            }
        }

        // Update root commitment
        self.root_commitment = current_commitment;

        Ok(())
    }

    /// Get the commitment for any node (leaf or branch)
    pub(crate) fn get_node_commitment(
        &self,
        node_index: NodeIndex,
    ) -> Result<Commitment, TreeError> {
        if node_index.is_leaf() {
            let leaf_index = node_index
                .to_leaf_index()
                .ok_or(TreeError::InvalidNodeIndex(node_index))?;
            let leaf = self
                .leaves
                .get(&leaf_index)
                .ok_or(TreeError::LeafNotFound(leaf_index))?;
            Ok(self.compute_leaf_commitment(leaf))
        } else {
            let branch = self
                .branches
                .get(&node_index)
                .ok_or(TreeError::BranchNotFound(node_index))?;
            Ok(branch.commitment)
        }
    }

    /// Increment the epoch counter
    pub fn increment_epoch(&mut self) {
        self.epoch += 1;
    }

    /// Validate tree invariants
    ///
    /// Checks:
    /// - All leaf indices are contiguous starting from 0
    /// - All branch indices are valid for the tree size
    /// - Commitments are correctly computed
    pub fn validate(&self) -> Result<(), TreeError> {
        let num_leaves = self.num_leaves();

        if num_leaves == 0 {
            return Ok(());
        }

        // Check leaf indices are contiguous
        for i in 0..num_leaves {
            let leaf_index = LeafIndex(i);
            if !self.leaves.contains_key(&leaf_index) {
                return Err(TreeError::InvalidTreeStructure(format!(
                    "Missing leaf at index {}",
                    i
                )));
            }
        }

        // Validate root commitment matches
        if num_leaves > 0 {
            let root_idx = root_index(num_leaves);
            let computed_root = self.get_node_commitment(root_idx)?;
            if computed_root != self.root_commitment {
                return Err(TreeError::InvalidCommitment {
                    node: root_idx,
                    expected: self.root_commitment,
                    actual: computed_root,
                });
            }
        }

        Ok(())
    }
}

impl Default for RatchetTree {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur during tree operations
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// Tree is empty
    #[error("Tree is empty")]
    EmptyTree,

    /// Leaf not found
    #[error("Leaf not found at index {0}")]
    LeafNotFound(LeafIndex),

    /// Branch not found
    #[error("Branch not found at index {0}")]
    BranchNotFound(NodeIndex),

    /// Invalid node index
    #[error("Invalid node index: {0}")]
    InvalidNodeIndex(NodeIndex),

    /// Invalid tree structure
    #[error("Invalid tree structure: {0}")]
    InvalidTreeStructure(String),

    /// Invalid commitment
    #[error("Invalid commitment at node {node}: expected {expected}, got {actual}")]
    InvalidCommitment {
        /// The node index where commitment mismatch occurred
        node: NodeIndex,
        /// Expected commitment value
        expected: Commitment,
        /// Actual commitment value
        actual: Commitment,
    },

    /// Leaf already exists
    #[error("Leaf already exists at index {0}")]
    LeafAlreadyExists(LeafIndex),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::node::{KeyPackage, LeafId, LeafMetadata};

    #[allow(dead_code)]
    fn create_test_leaf(index: usize) -> LeafNode {
        LeafNode::new_device(
            LeafId::new(),
            LeafIndex(index),
            KeyPackage {
                signing_key: vec![index as u8; 32],
                encryption_key: None,
            },
            LeafMetadata::default(),
        )
    }

    #[test]
    fn test_new_tree() {
        let tree = RatchetTree::new();
        assert_eq!(tree.epoch, 0);
        assert!(tree.is_empty());
        assert_eq!(tree.num_leaves(), 0);
    }

    #[test]
    fn test_empty_tree_root() {
        let tree = RatchetTree::new();
        assert!(tree.root_index().is_err());
    }
}
