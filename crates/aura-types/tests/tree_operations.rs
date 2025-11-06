//! Unit tests for tree operations
//!
//! Tests specific tree operation behaviors and edge cases.

use aura_journal::tree::{
    LeafId, LeafIndex, LeafNode, LeafRole, NodeIndex, Policy, RatchetTree,
};
use aura_journal::tree::node::{KeyPackage, LeafMetadata};

fn create_test_leaf(index: usize) -> LeafNode {
    LeafNode {
        leaf_id: LeafId::new(),
        leaf_index: LeafIndex(index),
        role: LeafRole::Device,
        public_key: KeyPackage {
            signing_key: vec![index as u8; 32],
            encryption_key: None,
        },
        metadata: LeafMetadata {
            display_name: format!("Device {}", index),
            platform: Some("test".to_string()),
            extra: Default::default(),
        },
    }
}

#[test]
fn test_add_leaf_to_empty_tree() {
    let mut tree = RatchetTree::new();
    assert!(tree.is_empty());
    assert_eq!(tree.epoch, 0);

    let leaf = create_test_leaf(0);
    let result = tree.add_leaf(leaf);

    assert!(result.is_ok());
    let affected = result.unwrap();

    assert_eq!(tree.num_leaves(), 1);
    assert_eq!(tree.epoch, 1);
    assert!(!tree.is_empty());

    // Single leaf is the root
    assert_eq!(tree.root_index().unwrap(), NodeIndex::new(0));

    // Check affected path
    assert!(affected
        .affected_indices
        .contains(&LeafIndex(0).to_node_index()));
    assert!(!affected.new_commitments.is_empty());
}

#[test]
fn test_add_second_leaf() {
    let mut tree = RatchetTree::new();

    tree.add_leaf(create_test_leaf(0)).unwrap();
    let old_root = *tree.root_commitment();

    let result = tree.add_leaf(create_test_leaf(1));
    assert!(result.is_ok());

    assert_eq!(tree.num_leaves(), 2);
    assert_eq!(tree.epoch, 2);

    // Root should have changed
    assert_ne!(*tree.root_commitment(), old_root);

    // Root index for 2 leaves should be 3
    assert_eq!(tree.root_index().unwrap(), NodeIndex::new(3));
}

#[test]
fn test_add_multiple_leaves_maintains_balance() {
    let mut tree = RatchetTree::new();

    for i in 0..10 {
        let result = tree.add_leaf(create_test_leaf(i));
        assert!(result.is_ok());
        assert_eq!(tree.num_leaves(), i + 1);
        assert!(tree.validate().is_ok());
    }
}

#[test]
fn test_add_duplicate_leaf_fails() {
    let mut tree = RatchetTree::new();

    let leaf = create_test_leaf(0);
    tree.add_leaf(leaf.clone()).unwrap();

    let result = tree.add_leaf(leaf);
    assert!(result.is_err());
}

#[test]
fn test_remove_leaf_from_empty_tree_fails() {
    let mut tree = RatchetTree::new();
    let result = tree.remove_leaf(LeafIndex(0));
    assert!(result.is_err());
}

#[test]
fn test_remove_nonexistent_leaf_fails() {
    let mut tree = RatchetTree::new();
    tree.add_leaf(create_test_leaf(0)).unwrap();

    let result = tree.remove_leaf(LeafIndex(99));
    assert!(result.is_err());
}

#[test]
fn test_remove_single_leaf() {
    let mut tree = RatchetTree::new();
    tree.add_leaf(create_test_leaf(0)).unwrap();

    let result = tree.remove_leaf(LeafIndex(0));
    assert!(result.is_ok());

    assert!(tree.is_empty());
    assert_eq!(tree.num_leaves(), 0);
}

#[test]
fn test_remove_last_leaf_from_multi_leaf_tree() {
    let mut tree = RatchetTree::new();

    // Add 3 leaves
    tree.add_leaf(create_test_leaf(0)).unwrap();
    tree.add_leaf(create_test_leaf(1)).unwrap();
    tree.add_leaf(create_test_leaf(2)).unwrap();

    // Remove last leaf
    let result = tree.remove_leaf(LeafIndex(2));
    assert!(result.is_ok());

    assert_eq!(tree.num_leaves(), 2);
    assert!(tree.validate().is_ok());
}

#[test]
fn test_remove_middle_leaf_swaps_with_last() {
    let mut tree = RatchetTree::new();

    // Add 4 leaves with distinct signing keys
    for i in 0..4 {
        tree.add_leaf(create_test_leaf(i)).unwrap();
    }

    let last_leaf_key = tree
        .get_leaf(LeafIndex(3))
        .unwrap()
        .public_key
        .signing_key
        .clone();

    // Remove leaf at index 1
    let result = tree.remove_leaf(LeafIndex(1));
    assert!(result.is_ok());

    assert_eq!(tree.num_leaves(), 3);

    // Leaf that was at index 3 should now be at index 1
    let swapped_leaf = tree.get_leaf(LeafIndex(1)).unwrap();
    assert_eq!(swapped_leaf.public_key.signing_key, last_leaf_key);
    assert_eq!(swapped_leaf.leaf_index, LeafIndex(1));

    assert!(tree.validate().is_ok());
}

#[test]
fn test_rotate_path_from_empty_tree_fails() {
    let mut tree = RatchetTree::new();
    let result = tree.rotate_path(LeafIndex(0));
    assert!(result.is_err());
}

#[test]
fn test_rotate_path_nonexistent_leaf_fails() {
    let mut tree = RatchetTree::new();
    tree.add_leaf(create_test_leaf(0)).unwrap();

    let result = tree.rotate_path(LeafIndex(99));
    assert!(result.is_err());
}

#[test]
fn test_rotate_path_single_leaf() {
    let mut tree = RatchetTree::new();
    tree.add_leaf(create_test_leaf(0)).unwrap();

    let old_commitment = *tree.root_commitment();
    let old_epoch = tree.epoch;

    let result = tree.rotate_path(LeafIndex(0));
    assert!(result.is_ok());

    assert_ne!(*tree.root_commitment(), old_commitment);
    assert_eq!(tree.epoch, old_epoch + 1);
}

#[test]
fn test_rotate_path_updates_ancestors() {
    let mut tree = RatchetTree::new();

    tree.add_leaf(create_test_leaf(0)).unwrap();
    tree.add_leaf(create_test_leaf(1)).unwrap();
    tree.add_leaf(create_test_leaf(2)).unwrap();

    let old_root = *tree.root_commitment();

    let result = tree.rotate_path(LeafIndex(1));
    assert!(result.is_ok());

    let affected = result.unwrap();

    // Root should have changed
    assert_ne!(*tree.root_commitment(), old_root);

    // Affected path should include the leaf and its ancestors
    assert!(affected
        .affected_indices
        .contains(&LeafIndex(1).to_node_index()));
    assert!(!affected.old_commitments.is_empty());
    assert!(!affected.new_commitments.is_empty());
}

#[test]
fn test_consecutive_operations() {
    let mut tree = RatchetTree::new();

    // Add leaves
    tree.add_leaf(create_test_leaf(0)).unwrap();
    tree.add_leaf(create_test_leaf(1)).unwrap();
    tree.add_leaf(create_test_leaf(2)).unwrap();

    let epoch_after_add = tree.epoch;

    // Rotate
    tree.rotate_path(LeafIndex(0)).unwrap();
    assert_eq!(tree.epoch, epoch_after_add + 1);

    // Remove
    tree.remove_leaf(LeafIndex(2)).unwrap();
    assert_eq!(tree.epoch, epoch_after_add + 2);

    // Add again
    tree.add_leaf(create_test_leaf(2)).unwrap();
    assert_eq!(tree.epoch, epoch_after_add + 3);

    assert!(tree.validate().is_ok());
}

#[test]
fn test_affected_path_records_old_commitments() {
    let mut tree = RatchetTree::new();

    tree.add_leaf(create_test_leaf(0)).unwrap();

    let affected = tree.add_leaf(create_test_leaf(1)).unwrap();

    // Should record old root commitment
    assert!(!affected.old_commitments.is_empty());
    assert!(affected.old_commitments.contains_key(&NodeIndex::new(0))); // Old root
}

#[test]
fn test_affected_path_records_new_commitments() {
    let mut tree = RatchetTree::new();

    tree.add_leaf(create_test_leaf(0)).unwrap();
    tree.add_leaf(create_test_leaf(1)).unwrap();

    let affected = tree.rotate_path(LeafIndex(0)).unwrap();

    // Should record new commitments for affected nodes
    assert!(!affected.new_commitments.is_empty());

    // Should include the root
    let root_idx = tree.root_index().unwrap();
    assert!(affected.new_commitments.contains_key(&root_idx));
}

#[test]
fn test_tree_validation_catches_missing_leaves() {
    let mut tree = RatchetTree::new();

    tree.add_leaf(create_test_leaf(0)).unwrap();
    tree.add_leaf(create_test_leaf(1)).unwrap();
    tree.add_leaf(create_test_leaf(2)).unwrap();

    // Manually corrupt the tree by removing a leaf from the middle
    tree.leaves.remove(&LeafIndex(1));

    let result = tree.validate();
    assert!(result.is_err());
}

#[test]
fn test_leaf_and_guardian_operations() {
    let mut tree = RatchetTree::new();

    // Add device
    let mut device = create_test_leaf(0);
    device.role = LeafRole::Device;
    tree.add_leaf(device).unwrap();

    // Add guardian
    let mut guardian = create_test_leaf(1);
    guardian.role = LeafRole::Guardian;
    tree.add_leaf(guardian).unwrap();

    assert_eq!(tree.num_leaves(), 2);
    assert_eq!(tree.get_leaf(LeafIndex(0)).unwrap().role, LeafRole::Device);
    assert_eq!(
        tree.get_leaf(LeafIndex(1)).unwrap().role,
        LeafRole::Guardian
    );

    // Operations should work the same regardless of role
    tree.rotate_path(LeafIndex(0)).unwrap();
    tree.rotate_path(LeafIndex(1)).unwrap();

    assert!(tree.validate().is_ok());
}

#[test]
fn test_get_leaf() {
    let mut tree = RatchetTree::new();

    tree.add_leaf(create_test_leaf(0)).unwrap();
    tree.add_leaf(create_test_leaf(1)).unwrap();

    let leaf0 = tree.get_leaf(LeafIndex(0));
    assert!(leaf0.is_some());
    assert_eq!(leaf0.unwrap().leaf_index, LeafIndex(0));

    let leaf1 = tree.get_leaf(LeafIndex(1));
    assert!(leaf1.is_some());
    assert_eq!(leaf1.unwrap().leaf_index, LeafIndex(1));

    let leaf_missing = tree.get_leaf(LeafIndex(99));
    assert!(leaf_missing.is_none());
}

#[test]
fn test_epoch_increments_correctly() {
    let mut tree = RatchetTree::new();
    assert_eq!(tree.epoch, 0);

    tree.add_leaf(create_test_leaf(0)).unwrap();
    assert_eq!(tree.epoch, 1);

    tree.add_leaf(create_test_leaf(1)).unwrap();
    assert_eq!(tree.epoch, 2);

    tree.rotate_path(LeafIndex(0)).unwrap();
    assert_eq!(tree.epoch, 3);

    tree.remove_leaf(LeafIndex(1)).unwrap();
    assert_eq!(tree.epoch, 4);
}

#[test]
fn test_root_commitment_updates() {
    let mut tree = RatchetTree::new();

    let initial_commitment = *tree.root_commitment();

    tree.add_leaf(create_test_leaf(0)).unwrap();
    let after_first = *tree.root_commitment();
    assert_ne!(after_first, initial_commitment);

    tree.add_leaf(create_test_leaf(1)).unwrap();
    let after_second = *tree.root_commitment();
    assert_ne!(after_second, after_first);

    tree.rotate_path(LeafIndex(0)).unwrap();
    let after_rotate = *tree.root_commitment();
    assert_ne!(after_rotate, after_second);
}
