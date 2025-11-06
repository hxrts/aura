//! Property-based tests for ratchet tree invariants
//!
//! These tests verify that LBBT invariants hold across random operation sequences.

use aura_journal::tree::{
    AffectedPath, BranchNode, Commitment, LeafId, LeafIndex, LeafNode, LeafRole,
    NodeIndex, Policy, RatchetTree,
};
use aura_journal::tree::node::{KeyPackage, LeafMetadata};

/// Create a test leaf node with deterministic data
fn create_test_leaf(index: usize, role: LeafRole) -> LeafNode {
    LeafNode {
        leaf_id: LeafId::new(),
        leaf_index: LeafIndex(index),
        role,
        public_key: KeyPackage {
            signing_key: vec![index as u8; 32],
            encryption_key: None,
        },
        metadata: LeafMetadata {
            display_name: format!("Test {} {}", role, index),
            platform: Some("test".to_string()),
            extra: Default::default(),
        },
    }
}

#[test]
fn test_lbbt_invariant_maintained_across_adds() {
    let mut tree = RatchetTree::new();

    // Add 10 leaves and verify LBBT after each addition
    for i in 0..10 {
        let leaf = create_test_leaf(i, LeafRole::Device);
        let result = tree.add_leaf(leaf);
        assert!(result.is_ok(), "Failed to add leaf {}: {:?}", i, result);

        // Verify tree structure
        assert!(
            tree.validate().is_ok(),
            "Tree invalid after adding leaf {}",
            i
        );

        // Verify leaf count
        assert_eq!(tree.num_leaves(), i + 1);

        // Verify epoch incremented
        assert_eq!(tree.epoch, (i + 1) as u64);
    }
}

#[test]
fn test_lbbt_invariant_maintained_across_removes() {
    let mut tree = RatchetTree::new();

    // Add 10 leaves
    for i in 0..10 {
        tree.add_leaf(create_test_leaf(i, LeafRole::Device))
            .unwrap();
    }

    // Remove leaves one by one from the end
    for i in (0..10).rev() {
        let result = tree.remove_leaf(LeafIndex(i));
        assert!(result.is_ok(), "Failed to remove leaf {}: {:?}", i, result);

        // Verify tree structure (if not empty)
        if !tree.is_empty() {
            assert!(
                tree.validate().is_ok(),
                "Tree invalid after removing leaf {}",
                i
            );
        }

        // Verify leaf count
        assert_eq!(tree.num_leaves(), i);
    }

    assert!(tree.is_empty());
}

#[test]
fn test_lbbt_invariant_with_mixed_operations() {
    let mut tree = RatchetTree::new();

    // Add some leaves
    for i in 0..5 {
        tree.add_leaf(create_test_leaf(i, LeafRole::Device))
            .unwrap();
    }

    // Remove some
    tree.remove_leaf(LeafIndex(2)).unwrap();
    tree.remove_leaf(LeafIndex(3)).unwrap();

    assert!(tree.validate().is_ok());
    assert_eq!(tree.num_leaves(), 3);

    // Add more
    tree.add_leaf(create_test_leaf(3, LeafRole::Device))
        .unwrap();
    tree.add_leaf(create_test_leaf(4, LeafRole::Device))
        .unwrap();

    assert!(tree.validate().is_ok());
    assert_eq!(tree.num_leaves(), 5);

    // Rotate paths
    tree.rotate_path(LeafIndex(0)).unwrap();
    tree.rotate_path(LeafIndex(1)).unwrap();

    assert!(tree.validate().is_ok());
}

#[test]
fn test_commitment_determinism() {
    // Same operations should produce same commitments
    let mut tree1 = RatchetTree::new();
    let mut tree2 = RatchetTree::new();

    // Add same leaves to both trees
    for i in 0..5 {
        let leaf1 = LeafNode {
            leaf_id: LeafId::from_uuid(uuid::Uuid::from_u128(i as u128)),
            leaf_index: LeafIndex(i),
            role: LeafRole::Device,
            public_key: KeyPackage {
                signing_key: vec![i as u8; 32],
                encryption_key: None,
            },
            metadata: LeafMetadata::default(),
        };

        let leaf2 = LeafNode {
            leaf_id: LeafId::from_uuid(uuid::Uuid::from_u128(i as u128)),
            leaf_index: LeafIndex(i),
            role: LeafRole::Device,
            public_key: KeyPackage {
                signing_key: vec![i as u8; 32],
                encryption_key: None,
            },
            metadata: LeafMetadata::default(),
        };

        tree1.add_leaf(leaf1).unwrap();
        tree2.add_leaf(leaf2).unwrap();
    }

    // Commitments should be identical
    assert_eq!(tree1.root_commitment(), tree2.root_commitment());
    assert_eq!(tree1.epoch, tree2.epoch);
}

#[test]
fn test_epoch_monotonicity() {
    let mut tree = RatchetTree::new();
    let mut last_epoch = 0;

    // Add leaves
    for i in 0..5 {
        tree.add_leaf(create_test_leaf(i, LeafRole::Device))
            .unwrap();
        assert!(tree.epoch > last_epoch, "Epoch did not increase after add");
        last_epoch = tree.epoch;
    }

    // Rotate paths
    for i in 0..5 {
        tree.rotate_path(LeafIndex(i)).unwrap();
        assert!(
            tree.epoch > last_epoch,
            "Epoch did not increase after rotate"
        );
        last_epoch = tree.epoch;
    }

    // Remove leaves
    for i in (0..5).rev() {
        tree.remove_leaf(LeafIndex(i)).unwrap();
        if !tree.is_empty() {
            assert!(
                tree.epoch > last_epoch,
                "Epoch did not increase after remove"
            );
        }
        last_epoch = tree.epoch;
    }
}

#[test]
fn test_commitment_changes_on_mutation() {
    let mut tree = RatchetTree::new();

    tree.add_leaf(create_test_leaf(0, LeafRole::Device))
        .unwrap();
    tree.add_leaf(create_test_leaf(1, LeafRole::Device))
        .unwrap();

    let commitment_after_add = *tree.root_commitment();

    // Rotate should change commitment
    tree.rotate_path(LeafIndex(0)).unwrap();
    assert_ne!(
        *tree.root_commitment(),
        commitment_after_add,
        "Commitment should change after rotation"
    );

    let commitment_after_rotate = *tree.root_commitment();

    // Remove should change commitment
    tree.remove_leaf(LeafIndex(1)).unwrap();
    assert_ne!(
        *tree.root_commitment(),
        commitment_after_rotate,
        "Commitment should change after removal"
    );
}

#[test]
fn test_path_rotation_provides_forward_secrecy() {
    let mut tree = RatchetTree::new();

    tree.add_leaf(create_test_leaf(0, LeafRole::Device))
        .unwrap();
    tree.add_leaf(create_test_leaf(1, LeafRole::Device))
        .unwrap();

    let old_root = *tree.root_commitment();
    let old_epoch = tree.epoch;

    // Rotate path
    let affected = tree.rotate_path(LeafIndex(0)).unwrap();

    // New epoch and commitment
    assert!(tree.epoch > old_epoch);
    assert_ne!(*tree.root_commitment(), old_root);

    // Old commitment should be recorded
    assert!(affected.old_commitments.contains_key(&NodeIndex::new(3))); // Root index for 2 leaves
}

#[test]
fn test_device_and_guardian_roles() {
    let mut tree = RatchetTree::new();

    // Add devices
    tree.add_leaf(create_test_leaf(0, LeafRole::Device))
        .unwrap();
    tree.add_leaf(create_test_leaf(1, LeafRole::Device))
        .unwrap();

    // Add guardians
    tree.add_leaf(create_test_leaf(2, LeafRole::Guardian))
        .unwrap();
    tree.add_leaf(create_test_leaf(3, LeafRole::Guardian))
        .unwrap();

    assert_eq!(tree.num_leaves(), 4);

    // Verify roles are preserved
    assert_eq!(tree.get_leaf(LeafIndex(0)).unwrap().role, LeafRole::Device);
    assert_eq!(
        tree.get_leaf(LeafIndex(2)).unwrap().role,
        LeafRole::Guardian
    );

    assert!(tree.validate().is_ok());
}

#[test]
fn test_affected_path_completeness() {
    let mut tree = RatchetTree::new();

    tree.add_leaf(create_test_leaf(0, LeafRole::Device))
        .unwrap();
    tree.add_leaf(create_test_leaf(1, LeafRole::Device))
        .unwrap();

    let affected = tree
        .add_leaf(create_test_leaf(2, LeafRole::Device))
        .unwrap();

    // Should have old commitments (root existed before)
    assert!(!affected.old_commitments.is_empty());

    // Should have new commitments for affected nodes
    assert!(!affected.new_commitments.is_empty());

    // Affected indices should include the new leaf and ancestors
    assert!(!affected.affected_indices.is_empty());
    assert!(affected
        .affected_indices
        .contains(&LeafIndex(2).to_node_index()));
}

#[test]
fn test_single_leaf_tree_operations() {
    let mut tree = RatchetTree::new();

    // Add single leaf
    tree.add_leaf(create_test_leaf(0, LeafRole::Device))
        .unwrap();
    assert_eq!(tree.num_leaves(), 1);
    assert!(tree.validate().is_ok());

    // Single leaf is its own root
    let root_idx = tree.root_index().unwrap();
    assert_eq!(root_idx, NodeIndex::new(0));

    // Rotate single leaf path
    let old_commitment = *tree.root_commitment();
    tree.rotate_path(LeafIndex(0)).unwrap();
    assert_ne!(*tree.root_commitment(), old_commitment);

    // Remove single leaf
    tree.remove_leaf(LeafIndex(0)).unwrap();
    assert!(tree.is_empty());
}

#[test]
fn test_large_tree_operations() {
    let mut tree = RatchetTree::new();

    // Add 100 leaves
    for i in 0..100 {
        let leaf = create_test_leaf(
            i,
            if i % 5 == 0 {
                LeafRole::Guardian
            } else {
                LeafRole::Device
            },
        );
        tree.add_leaf(leaf).unwrap();
    }

    assert_eq!(tree.num_leaves(), 100);
    assert!(tree.validate().is_ok());

    // Rotate some paths
    for i in (0..10).step_by(2) {
        tree.rotate_path(LeafIndex(i)).unwrap();
    }

    assert!(tree.validate().is_ok());

    // Remove some leaves
    for i in (90..100).rev() {
        tree.remove_leaf(LeafIndex(i)).unwrap();
    }

    assert_eq!(tree.num_leaves(), 90);
    assert!(tree.validate().is_ok());
}

#[test]
fn test_swap_behavior_on_remove() {
    let mut tree = RatchetTree::new();

    // Add 5 leaves with distinct keys
    for i in 0..5 {
        tree.add_leaf(create_test_leaf(i, LeafRole::Device))
            .unwrap();
    }

    // Note which leaf is at index 4
    let last_leaf_key = tree
        .get_leaf(LeafIndex(4))
        .unwrap()
        .public_key
        .signing_key
        .clone();

    // Remove leaf at index 1 (should swap with last)
    tree.remove_leaf(LeafIndex(1)).unwrap();

    // The leaf that was at index 4 should now be at index 1
    assert_eq!(tree.num_leaves(), 4);

    // After swap, the last leaf (originally at 4) moves to position 1
    let moved_leaf = tree.get_leaf(LeafIndex(1)).unwrap();
    assert_eq!(moved_leaf.public_key.signing_key, last_leaf_key);
    assert_eq!(moved_leaf.leaf_index, LeafIndex(1));

    assert!(tree.validate().is_ok());
}

#[test]
fn test_consecutive_rotations() {
    let mut tree = RatchetTree::new();

    tree.add_leaf(create_test_leaf(0, LeafRole::Device))
        .unwrap();
    tree.add_leaf(create_test_leaf(1, LeafRole::Device))
        .unwrap();

    let commitments: Vec<Commitment> = (0..10)
        .map(|_| {
            tree.rotate_path(LeafIndex(0)).unwrap();
            *tree.root_commitment()
        })
        .collect();

    // All commitments should be unique (each rotation changes state)
    for i in 0..commitments.len() {
        for j in (i + 1)..commitments.len() {
            assert_ne!(
                commitments[i], commitments[j],
                "Rotation {} and {} produced same commitment",
                i, j
            );
        }
    }
}
