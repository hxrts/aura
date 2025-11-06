//! Tree Operations
//!
//! Implements tree mutation operations: add_leaf, remove_leaf, rotate_path.
//! These operations maintain LBBT invariants and update commitments.

use crate::tree::indexing::{LeafIndex, NodeIndex};
use crate::tree::node::LeafNode;
use crate::tree::state::{RatchetTree, TreeError};
use crate::tree::Commitment;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Metadata about nodes affected by a tree operation
///
/// Tracks which nodes were modified and their old/new commitments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AffectedPath {
    /// Indices of nodes that were modified
    pub affected_indices: Vec<NodeIndex>,
    /// Old commitments before the operation
    pub old_commitments: BTreeMap<NodeIndex, Commitment>,
    /// New commitments after the operation
    pub new_commitments: BTreeMap<NodeIndex, Commitment>,
}

impl AffectedPath {
    /// Create a new affected path
    pub fn new() -> Self {
        Self {
            affected_indices: Vec::new(),
            old_commitments: BTreeMap::new(),
            new_commitments: BTreeMap::new(),
        }
    }
}

impl Default for AffectedPath {
    fn default() -> Self {
        Self::new()
    }
}

/// Tree operation type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TreeOperation {
    /// Add a new leaf to the tree
    AddLeaf {
        /// The leaf node being added
        leaf: LeafNode,
        /// Path affected by this operation
        affected_path: AffectedPath,
    },
    /// Remove a leaf from the tree
    RemoveLeaf {
        /// Index of the leaf being removed
        leaf_index: LeafIndex,
        /// Path affected by this operation
        affected_path: AffectedPath,
    },
    /// Rotate secrets along a path (for forward secrecy)
    RotatePath {
        /// Index of the leaf whose path is being rotated
        leaf_index: LeafIndex,
        /// Path affected by this operation
        affected_path: AffectedPath,
    },
}

impl RatchetTree {
    /// Add a new leaf to the tree
    ///
    /// Inserts the leaf at the next available LBBT slot and updates commitments.
    pub fn add_leaf(&mut self, leaf: LeafNode) -> Result<AffectedPath, TreeError> {
        use crate::tree::indexing::direct_path;
        use crate::tree::node::{BranchNode, Policy};

        let mut affected = AffectedPath::new();
        let leaf_index = leaf.leaf_index;

        // Check if leaf already exists
        if self.leaves.contains_key(&leaf_index) {
            return Err(TreeError::LeafAlreadyExists(leaf_index));
        }

        // Record old commitments along the path (if tree not empty)
        if !self.is_empty() {
            let old_root = self.root_commitment;
            affected
                .old_commitments
                .insert(self.root_index()?, old_root);
        }

        // Insert the leaf
        self.leaves.insert(leaf_index, leaf);

        // If this is the first leaf, just update root commitment
        if self.num_leaves() == 1 {
            let leaf = self
                .leaves
                .get(&leaf_index)
                .expect("leaf was just inserted");
            self.root_commitment = self.compute_leaf_commitment(leaf);
            affected.affected_indices.push(leaf_index.to_node_index());
            affected
                .new_commitments
                .insert(leaf_index.to_node_index(), self.root_commitment);
            self.increment_epoch();
            return Ok(affected);
        }

        // Create/update branch nodes along the path
        let path = direct_path(leaf_index, self.num_leaves());

        // Ensure all branches exist with default All policy
        for &node_index in &path {
            self.branches
                .entry(node_index)
                .or_insert_with(|| BranchNode {
                    node_index,
                    policy: Policy::All, // Default policy
                    commitment: Commitment::default(),
                });
        }

        // Update commitments from leaf to root
        self.update_path_commitments(leaf_index)?;

        // Record new commitments
        affected.affected_indices.push(leaf_index.to_node_index());
        for &node_index in &path {
            affected.affected_indices.push(node_index);
            if let Ok(commitment) = self.get_node_commitment(node_index) {
                affected.new_commitments.insert(node_index, commitment);
            }
        }

        // Increment epoch
        self.increment_epoch();

        Ok(affected)
    }

    /// Remove a leaf from the tree
    ///
    /// Removes the leaf and rebalances by swapping with the last leaf.
    pub fn remove_leaf(&mut self, leaf_index: LeafIndex) -> Result<AffectedPath, TreeError> {
        use crate::tree::indexing::direct_path;

        let mut affected = AffectedPath::new();

        // Check tree is not empty
        if self.is_empty() {
            return Err(TreeError::EmptyTree);
        }

        // Check leaf exists
        if !self.leaves.contains_key(&leaf_index) {
            return Err(TreeError::LeafNotFound(leaf_index));
        }

        // Record old root commitment
        let old_root = self.root_commitment;
        affected
            .old_commitments
            .insert(self.root_index()?, old_root);

        let num_leaves = self.num_leaves();
        let last_index = LeafIndex(num_leaves - 1);

        // If removing the last leaf, we can just remove it directly
        if leaf_index == last_index {
            self.leaves.remove(&leaf_index);

            // If tree is now empty, reset
            if self.is_empty() {
                self.root_commitment = Commitment::default();
                self.branches.clear();
                self.increment_epoch();
                return Ok(affected);
            }

            // Update the new last leaf's path
            let new_last = LeafIndex(num_leaves - 2);
            self.update_path_commitments(new_last)?;

            // Record affected nodes
            affected.affected_indices.push(leaf_index.to_node_index());
            let path = direct_path(new_last, self.num_leaves());
            for &node_index in &path {
                affected.affected_indices.push(node_index);
                if let Ok(commitment) = self.get_node_commitment(node_index) {
                    affected.new_commitments.insert(node_index, commitment);
                }
            }
        } else {
            // Swap with last leaf to maintain LBBT invariant
            let mut last_leaf = self
                .leaves
                .remove(&last_index)
                .expect("last leaf exists because we checked num_leaves > 0");
            last_leaf.leaf_index = leaf_index; // Update index

            self.leaves.remove(&leaf_index);
            self.leaves.insert(leaf_index, last_leaf);

            // Update commitments from the swapped leaf
            self.update_path_commitments(leaf_index)?;

            // Record affected nodes
            affected.affected_indices.push(leaf_index.to_node_index());
            let path = direct_path(leaf_index, self.num_leaves());
            for &node_index in &path {
                affected.affected_indices.push(node_index);
                if let Ok(commitment) = self.get_node_commitment(node_index) {
                    affected.new_commitments.insert(node_index, commitment);
                }
            }
        }

        // Clean up orphaned branches
        self.prune_unused_branches();

        // Increment epoch
        self.increment_epoch();

        Ok(affected)
    }

    /// Rotate the path from a leaf to the root
    ///
    /// Generates fresh secrets and updates all commitments along the path.
    /// Note: This simulates path rotation by recomputing commitments with the new epoch.
    /// Actual secret rotation happens at the cryptographic layer.
    pub fn rotate_path(&mut self, leaf_index: LeafIndex) -> Result<AffectedPath, TreeError> {
        use crate::tree::indexing::direct_path;

        let mut affected = AffectedPath::new();

        // Check tree is not empty
        if self.is_empty() {
            return Err(TreeError::EmptyTree);
        }

        // Check leaf exists
        if !self.leaves.contains_key(&leaf_index) {
            return Err(TreeError::LeafNotFound(leaf_index));
        }

        // Record old root commitment
        let old_root = self.root_commitment;
        affected
            .old_commitments
            .insert(self.root_index()?, old_root);

        // Increment epoch FIRST (this is what triggers fresh commitment computation)
        self.increment_epoch();

        // Update commitments from leaf to root with new epoch
        self.update_path_commitments(leaf_index)?;

        // Record affected nodes
        affected.affected_indices.push(leaf_index.to_node_index());
        let path = direct_path(leaf_index, self.num_leaves());
        for &node_index in &path {
            affected.affected_indices.push(node_index);
            if let Ok(commitment) = self.get_node_commitment(node_index) {
                affected.new_commitments.insert(node_index, commitment);
            }
        }

        Ok(affected)
    }

    /// Prune branches that are no longer part of the tree
    ///
    /// Removes branch nodes that aren't on any path from a leaf to the root.
    fn prune_unused_branches(&mut self) {
        use crate::tree::indexing::direct_path;

        if self.is_empty() {
            self.branches.clear();
            return;
        }

        // Collect all valid branch indices
        let mut valid_branches = std::collections::HashSet::new();

        // Add all branches on paths from each leaf
        for &leaf_index in self.leaves.keys() {
            let path = direct_path(leaf_index, self.num_leaves());
            for node_index in path {
                valid_branches.insert(node_index);
            }
        }

        // Remove branches not in the valid set
        self.branches
            .retain(|node_index, _| valid_branches.contains(node_index));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::node::{KeyPackage, LeafId, LeafMetadata, LeafRole};
    use crate::tree::state::RatchetTree;

    fn create_test_leaf(index: usize) -> LeafNode {
        LeafNode {
            leaf_id: LeafId::new(),
            leaf_index: LeafIndex(index),
            role: LeafRole::Device,
            public_key: KeyPackage {
                signing_key: vec![index as u8; 32],
                encryption_key: None,
            },
            metadata: LeafMetadata::default(),
        }
    }

    #[test]
    fn test_affected_path_new() {
        let path = AffectedPath::new();
        assert!(path.affected_indices.is_empty());
        assert!(path.old_commitments.is_empty());
        assert!(path.new_commitments.is_empty());
    }

    #[test]
    fn test_add_leaf_to_empty_tree() {
        let mut tree = RatchetTree::new();
        let leaf = create_test_leaf(0);

        let result = tree.add_leaf(leaf);
        assert!(result.is_ok());

        let affected = result.unwrap();
        assert_eq!(tree.num_leaves(), 1);
        assert_eq!(tree.epoch, 1);
        assert!(!affected.affected_indices.is_empty());
        assert!(!affected.new_commitments.is_empty());
    }

    #[test]
    fn test_add_multiple_leaves() {
        let mut tree = RatchetTree::new();

        // Add three leaves
        for i in 0..3 {
            let leaf = create_test_leaf(i);
            let result = tree.add_leaf(leaf);
            assert!(result.is_ok());
            assert_eq!(tree.num_leaves(), i + 1);
        }

        assert_eq!(tree.epoch, 3);
        assert!(tree.validate().is_ok());
    }

    #[test]
    fn test_add_duplicate_leaf() {
        let mut tree = RatchetTree::new();
        let leaf = create_test_leaf(0);

        tree.add_leaf(leaf.clone()).unwrap();
        let result = tree.add_leaf(leaf);

        assert!(matches!(result, Err(TreeError::LeafAlreadyExists(_))));
    }

    #[test]
    fn test_remove_leaf_from_single_leaf_tree() {
        let mut tree = RatchetTree::new();
        let leaf = create_test_leaf(0);
        tree.add_leaf(leaf).unwrap();

        let result = tree.remove_leaf(LeafIndex(0));
        assert!(result.is_ok());
        assert_eq!(tree.num_leaves(), 0);
        assert!(tree.is_empty());
    }

    #[test]
    fn test_remove_leaf_from_multi_leaf_tree() {
        let mut tree = RatchetTree::new();

        // Add three leaves
        for i in 0..3 {
            tree.add_leaf(create_test_leaf(i)).unwrap();
        }

        // Remove middle leaf
        let result = tree.remove_leaf(LeafIndex(1));
        assert!(result.is_ok());
        assert_eq!(tree.num_leaves(), 2);
        assert!(tree.validate().is_ok());
    }

    #[test]
    fn test_remove_nonexistent_leaf() {
        let mut tree = RatchetTree::new();
        tree.add_leaf(create_test_leaf(0)).unwrap();

        let result = tree.remove_leaf(LeafIndex(5));
        assert!(matches!(result, Err(TreeError::LeafNotFound(_))));
    }

    #[test]
    fn test_remove_from_empty_tree() {
        let mut tree = RatchetTree::new();
        let result = tree.remove_leaf(LeafIndex(0));
        assert!(matches!(result, Err(TreeError::EmptyTree)));
    }

    #[test]
    fn test_rotate_path() {
        let mut tree = RatchetTree::new();

        // Add two leaves
        tree.add_leaf(create_test_leaf(0)).unwrap();
        tree.add_leaf(create_test_leaf(1)).unwrap();

        let old_root = tree.root_commitment;
        let old_epoch = tree.epoch;

        // Rotate path from leaf 0
        let result = tree.rotate_path(LeafIndex(0));
        assert!(result.is_ok());

        let affected = result.unwrap();
        assert_ne!(tree.root_commitment, old_root);
        assert_eq!(tree.epoch, old_epoch + 1);
        assert!(!affected.affected_indices.is_empty());
        assert!(!affected.old_commitments.is_empty());
        assert!(!affected.new_commitments.is_empty());
    }

    #[test]
    fn test_rotate_path_empty_tree() {
        let mut tree = RatchetTree::new();
        let result = tree.rotate_path(LeafIndex(0));
        assert!(matches!(result, Err(TreeError::EmptyTree)));
    }

    #[test]
    fn test_rotate_path_nonexistent_leaf() {
        let mut tree = RatchetTree::new();
        tree.add_leaf(create_test_leaf(0)).unwrap();

        let result = tree.rotate_path(LeafIndex(5));
        assert!(matches!(result, Err(TreeError::LeafNotFound(_))));
    }

    #[test]
    fn test_operation_sequence() {
        let mut tree = RatchetTree::new();

        // Add leaves
        tree.add_leaf(create_test_leaf(0)).unwrap();
        tree.add_leaf(create_test_leaf(1)).unwrap();
        tree.add_leaf(create_test_leaf(2)).unwrap();

        // Rotate a path
        tree.rotate_path(LeafIndex(1)).unwrap();

        // Remove a leaf
        tree.remove_leaf(LeafIndex(2)).unwrap();

        // Add another leaf
        tree.add_leaf(create_test_leaf(2)).unwrap();

        assert_eq!(tree.num_leaves(), 3);
        assert!(tree.validate().is_ok());
    }

    #[test]
    fn test_affected_path_tracking() {
        let mut tree = RatchetTree::new();
        tree.add_leaf(create_test_leaf(0)).unwrap();

        let affected = tree.add_leaf(create_test_leaf(1)).unwrap();

        // Should have old commitments (root existed before)
        assert!(!affected.old_commitments.is_empty());

        // Should have new commitments for affected nodes
        assert!(!affected.new_commitments.is_empty());

        // Affected indices should include the new leaf and branches
        assert!(!affected.affected_indices.is_empty());
    }
}
