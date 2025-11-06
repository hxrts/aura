//! Journal Replay and Validation Tests
//!
//! Tests verify that tree state can be deterministically reconstructed
//! from epoch-ordered TreeOp replay.

use aura_types::identifiers::DeviceId;
use aura_journal::ledger::{JournalMap, ThresholdSignature, TreeOp, TreeOpRecord};
use aura_journal::tree::{
    AffectedPath, Commitment, LeafId, LeafIndex, LeafNode, LeafRole, NodeIndex,
    Policy,
};
use aura_journal::tree::node::{KeyPackage, LeafMetadata};
use std::collections::BTreeMap;

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

fn create_add_leaf_op(epoch: u64, leaf: LeafNode) -> TreeOpRecord {
    let mut affected = AffectedPath::new();
    affected
        .affected_indices
        .push(leaf.leaf_index.to_node_index());

    TreeOpRecord {
        epoch,
        op: TreeOp::AddLeaf {
            leaf_node: leaf,
            affected_path: affected,
        },
        affected_indices: vec![],
        new_commitments: BTreeMap::new(),
        capability_refs: vec![],
        attestation: ThresholdSignature::new(vec![0u8; 64], vec![DeviceId::new(); 2], (2, 3)),
        authored_at: 1000,
        author: DeviceId::new(),
    }
}

fn create_rotate_op(epoch: u64, leaf_index: usize) -> TreeOpRecord {
    TreeOpRecord {
        epoch,
        op: TreeOp::RotatePath {
            leaf_index: LeafIndex(leaf_index),
            affected_path: AffectedPath::new(),
        },
        affected_indices: vec![],
        new_commitments: BTreeMap::new(),
        capability_refs: vec![],
        attestation: ThresholdSignature::new(vec![0u8; 64], vec![DeviceId::new(); 2], (2, 3)),
        authored_at: 1000,
        author: DeviceId::new(),
    }
}

fn create_remove_leaf_op(epoch: u64, leaf_index: usize) -> TreeOpRecord {
    TreeOpRecord {
        epoch,
        op: TreeOp::RemoveLeaf {
            leaf_index: LeafIndex(leaf_index),
            affected_path: AffectedPath::new(),
        },
        affected_indices: vec![],
        new_commitments: BTreeMap::new(),
        capability_refs: vec![],
        attestation: ThresholdSignature::new(vec![0u8; 64], vec![DeviceId::new(); 2], (2, 3)),
        authored_at: 1000,
        author: DeviceId::new(),
    }
}

#[test]
fn test_replay_empty_journal() {
    let journal = JournalMap::new();
    let tree = journal.replay_to_tree();
    assert!(tree.is_ok());
    let tree = tree.unwrap();
    assert!(tree.is_empty());
    assert_eq!(tree.epoch, 0);
}

#[test]
fn test_replay_single_add_leaf() {
    let mut journal = JournalMap::new();

    let leaf = create_test_leaf(0);
    journal.append_tree_op(create_add_leaf_op(1, leaf)).unwrap();

    let tree = journal.replay_to_tree().unwrap();
    assert_eq!(tree.num_leaves(), 1);
    assert_eq!(tree.epoch, 1);
}

#[test]
fn test_replay_multiple_add_leaves() {
    let mut journal = JournalMap::new();

    for i in 0..5 {
        let leaf = create_test_leaf(i);
        journal
            .append_tree_op(create_add_leaf_op((i + 1) as u64, leaf))
            .unwrap();
    }

    let tree = journal.replay_to_tree().unwrap();
    assert_eq!(tree.num_leaves(), 5);
    assert_eq!(tree.epoch, 5);
}

#[test]
fn test_replay_add_and_remove() {
    let mut journal = JournalMap::new();

    // Add 3 leaves
    for i in 0..3 {
        let leaf = create_test_leaf(i);
        journal
            .append_tree_op(create_add_leaf_op((i + 1) as u64, leaf))
            .unwrap();
    }

    // Remove one
    journal.append_tree_op(create_remove_leaf_op(4, 2)).unwrap();

    let tree = journal.replay_to_tree().unwrap();
    assert_eq!(tree.num_leaves(), 2);
    assert_eq!(tree.epoch, 4);
}

#[test]
fn test_replay_with_rotations() {
    let mut journal = JournalMap::new();

    // Add 2 leaves
    journal
        .append_tree_op(create_add_leaf_op(1, create_test_leaf(0)))
        .unwrap();
    journal
        .append_tree_op(create_add_leaf_op(2, create_test_leaf(1)))
        .unwrap();

    // Rotate paths
    journal.append_tree_op(create_rotate_op(3, 0)).unwrap();
    journal.append_tree_op(create_rotate_op(4, 1)).unwrap();

    let tree = journal.replay_to_tree().unwrap();
    assert_eq!(tree.num_leaves(), 2);
    assert_eq!(tree.epoch, 4);
}

#[test]
fn test_replay_deterministic_across_replicas() {
    let mut replica1 = JournalMap::new();
    let mut replica2 = JournalMap::new();

    // Add ops in same order
    for i in 0..3 {
        let leaf = create_test_leaf(i);
        let op = create_add_leaf_op((i + 1) as u64, leaf);
        replica1.append_tree_op(op.clone()).unwrap();
        replica2.append_tree_op(op).unwrap();
    }

    let tree1 = replica1.replay_to_tree().unwrap();
    let tree2 = replica2.replay_to_tree().unwrap();

    assert_eq!(tree1.num_leaves(), tree2.num_leaves());
    assert_eq!(tree1.epoch, tree2.epoch);
    assert_eq!(tree1.root_commitment(), tree2.root_commitment());
}

#[test]
fn test_replay_out_of_order_ops_produces_same_tree() {
    let mut journal1 = JournalMap::new();
    let mut journal2 = JournalMap::new();

    let op1 = create_add_leaf_op(1, create_test_leaf(0));
    let op2 = create_add_leaf_op(2, create_test_leaf(1));
    let op3 = create_add_leaf_op(3, create_test_leaf(2));

    // Journal 1: order 1, 2, 3
    journal1.append_tree_op(op1.clone()).unwrap();
    journal1.append_tree_op(op2.clone()).unwrap();
    journal1.append_tree_op(op3.clone()).unwrap();

    // Journal 2: order 3, 1, 2 (out of order delivery)
    journal2.append_tree_op(op3).unwrap();
    journal2.append_tree_op(op1).unwrap();
    journal2.append_tree_op(op2).unwrap();

    let tree1 = journal1.replay_to_tree().unwrap();
    let tree2 = journal2.replay_to_tree().unwrap();

    // Replay is epoch-ordered, so results should be identical
    assert_eq!(tree1.num_leaves(), tree2.num_leaves());
    assert_eq!(tree1.epoch, tree2.epoch);
}

#[test]
fn test_replay_with_epoch_bumps() {
    let mut journal = JournalMap::new();

    journal
        .append_tree_op(create_add_leaf_op(1, create_test_leaf(0)))
        .unwrap();

    let bump_op = TreeOpRecord {
        epoch: 2,
        op: TreeOp::EpochBump {
            reason: aura_journal::ledger::tree_op::EpochBumpReason::PeriodicRotation,
        },
        affected_indices: vec![],
        new_commitments: BTreeMap::new(),
        capability_refs: vec![],
        attestation: ThresholdSignature::new(vec![0u8; 64], vec![DeviceId::new(); 2], (2, 3)),
        authored_at: 1000,
        author: DeviceId::new(),
    };

    journal.append_tree_op(bump_op).unwrap();

    let tree = journal.replay_to_tree().unwrap();
    assert_eq!(tree.epoch, 2);
}

#[test]
fn test_replay_with_policy_refresh() {
    let mut journal = JournalMap::new();

    // Add leaves first
    journal
        .append_tree_op(create_add_leaf_op(1, create_test_leaf(0)))
        .unwrap();
    journal
        .append_tree_op(create_add_leaf_op(2, create_test_leaf(1)))
        .unwrap();

    // Refresh policy on a branch
    let policy_op = TreeOpRecord {
        epoch: 3,
        op: TreeOp::RefreshPolicy {
            node_index: NodeIndex::new(3), // Root for 2 leaves
            new_policy: Policy::threshold(2, 2),
            affected_path: AffectedPath::new(),
        },
        affected_indices: vec![],
        new_commitments: BTreeMap::new(),
        capability_refs: vec![],
        attestation: ThresholdSignature::new(vec![0u8; 64], vec![DeviceId::new(); 2], (2, 3)),
        authored_at: 1000,
        author: DeviceId::new(),
    };

    journal.append_tree_op(policy_op).unwrap();

    let tree = journal.replay_to_tree().unwrap();
    assert_eq!(tree.epoch, 3);

    // Verify policy was updated
    let branch = tree.get_branch(NodeIndex::new(3));
    assert!(branch.is_some());
    assert_eq!(branch.unwrap().policy, Policy::threshold(2, 2));
}

#[test]
fn test_replay_validates_tree_structure() {
    let mut journal = JournalMap::new();

    // Add leaves
    for i in 0..3 {
        journal
            .append_tree_op(create_add_leaf_op((i + 1) as u64, create_test_leaf(i)))
            .unwrap();
    }

    let tree = journal.replay_to_tree().unwrap();

    // Tree should be valid
    assert!(tree.validate().is_ok());
}

#[test]
fn test_cached_tree_invalidation() {
    let mut journal = JournalMap::new();

    // Get initial tree (builds cache)
    journal
        .append_tree_op(create_add_leaf_op(1, create_test_leaf(0)))
        .unwrap();
    let tree1 = journal.get_tree().unwrap();
    assert_eq!(tree1.num_leaves(), 1);

    // Add another op (should invalidate cache)
    journal
        .append_tree_op(create_add_leaf_op(2, create_test_leaf(1)))
        .unwrap();

    // Get tree again (should rebuild)
    let tree2 = journal.get_tree().unwrap();
    assert_eq!(tree2.num_leaves(), 2);
}

#[test]
fn test_replay_complex_sequence() {
    let mut journal = JournalMap::new();

    // Add 5 leaves
    for i in 0..5 {
        journal
            .append_tree_op(create_add_leaf_op((i + 1) as u64, create_test_leaf(i)))
            .unwrap();
    }

    // Rotate some paths
    journal.append_tree_op(create_rotate_op(6, 0)).unwrap();
    journal.append_tree_op(create_rotate_op(7, 2)).unwrap();

    // Remove some leaves
    journal.append_tree_op(create_remove_leaf_op(8, 4)).unwrap();
    journal.append_tree_op(create_remove_leaf_op(9, 3)).unwrap();

    // Add back
    journal
        .append_tree_op(create_add_leaf_op(10, create_test_leaf(3)))
        .unwrap();

    let tree = journal.replay_to_tree().unwrap();
    assert_eq!(tree.num_leaves(), 4);
    assert_eq!(tree.epoch, 10);
    assert!(tree.validate().is_ok());
}

#[test]
fn test_current_root_commitment() {
    let mut journal = JournalMap::new();

    assert!(journal.current_root_commitment().is_none());

    let mut commitments = BTreeMap::new();
    let test_commitment = Commitment::new([42u8; 32]);
    commitments.insert(NodeIndex::new(0), test_commitment);

    let op = TreeOpRecord {
        epoch: 1,
        op: TreeOp::EpochBump {
            reason: aura_journal::ledger::tree_op::EpochBumpReason::PeriodicRotation,
        },
        affected_indices: vec![],
        new_commitments: commitments,
        capability_refs: vec![],
        attestation: ThresholdSignature::new(vec![0u8; 64], vec![DeviceId::new(); 2], (2, 3)),
        authored_at: 1000,
        author: DeviceId::new(),
    };

    journal.append_tree_op(op).unwrap();

    let commitment = journal.current_root_commitment();
    assert!(commitment.is_some());
    assert_eq!(commitment.unwrap(), test_commitment);
}

#[test]
fn test_ops_ordered() {
    let mut journal = JournalMap::new();

    // Add ops out of order
    journal
        .append_tree_op(create_add_leaf_op(3, create_test_leaf(2)))
        .unwrap();
    journal
        .append_tree_op(create_add_leaf_op(1, create_test_leaf(0)))
        .unwrap();
    journal
        .append_tree_op(create_add_leaf_op(2, create_test_leaf(1)))
        .unwrap();

    let ops = journal.ops_ordered();

    // Should be in epoch order
    assert_eq!(ops.len(), 3);
    assert_eq!(ops[0].epoch, 1);
    assert_eq!(ops[1].epoch, 2);
    assert_eq!(ops[2].epoch, 3);
}
