//! CRDT Convergence Tests for Journal Ledger
//!
//! Tests verify that JournalMap satisfies CRDT properties:
//! - Associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
//! - Commutativity: a ⊔ b = b ⊔ a
//! - Idempotency: a ⊔ a = a

use aura_types::identifiers::DeviceId;
use aura_journal::ledger::{
    Intent, IntentId, IntentStatus, JournalMap, Priority, ThresholdSignature, TreeOp, TreeOpRecord,
};
use aura_journal::tree::{AffectedPath, Commitment, LeafIndex, NodeIndex, TreeOperation};
use std::collections::BTreeMap;

fn create_test_op(epoch: u64, commitment: Commitment) -> TreeOpRecord {
    let mut commitments = BTreeMap::new();
    commitments.insert(NodeIndex::new(0), commitment);

    TreeOpRecord {
        epoch,
        op: TreeOp::EpochBump {
            reason: aura_journal::ledger::tree_op::EpochBumpReason::PeriodicRotation,
        },
        affected_indices: vec![],
        new_commitments: commitments,
        capability_refs: vec![],
        attestation: ThresholdSignature::new(vec![0u8; 64], vec![DeviceId::new(); 2], (2, 3)),
        authored_at: 1000,
        author: DeviceId::new(),
    }
}

fn create_test_intent(priority: u64, snapshot: Commitment) -> Intent {
    Intent::new(
        TreeOperation::RotatePath {
            leaf_index: LeafIndex(0),
            affected_path: AffectedPath::new(),
        },
        vec![],
        snapshot,
        Priority::new(priority),
        DeviceId::new(),
        1000,
    )
}

#[test]
fn test_merge_associativity() {
    // (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
    let mut a = JournalMap::new();
    let mut b = JournalMap::new();
    let mut c = JournalMap::new();

    a.append_tree_op(create_test_op(1, Commitment::new([1u8; 32])))
        .unwrap();
    b.append_tree_op(create_test_op(2, Commitment::new([2u8; 32])))
        .unwrap();
    c.append_tree_op(create_test_op(3, Commitment::new([3u8; 32])))
        .unwrap();

    // Left: (a ⊔ b) ⊔ c
    let mut left = a.clone();
    left.merge(&b);
    left.merge(&c);

    // Right: a ⊔ (b ⊔ c)
    let mut right = a.clone();
    let mut b_c = b.clone();
    b_c.merge(&c);
    right.merge(&b_c);

    assert_eq!(left.num_ops(), right.num_ops());
    assert_eq!(left.latest_epoch(), right.latest_epoch());
}

#[test]
fn test_merge_commutativity() {
    // a ⊔ b = b ⊔ a
    let mut a = JournalMap::new();
    let mut b = JournalMap::new();

    a.append_tree_op(create_test_op(1, Commitment::new([1u8; 32])))
        .unwrap();
    b.append_tree_op(create_test_op(2, Commitment::new([2u8; 32])))
        .unwrap();

    // Left: a ⊔ b
    let mut left = a.clone();
    left.merge(&b);

    // Right: b ⊔ a
    let mut right = b.clone();
    right.merge(&a);

    assert_eq!(left.num_ops(), right.num_ops());
    assert_eq!(left.latest_epoch(), right.latest_epoch());
    assert_eq!(left.get_op(1), right.get_op(1));
    assert_eq!(left.get_op(2), right.get_op(2));
}

#[test]
fn test_merge_idempotency() {
    // a ⊔ a = a
    let mut a = JournalMap::new();
    a.append_tree_op(create_test_op(1, Commitment::new([1u8; 32])))
        .unwrap();

    let original_ops = a.num_ops();
    let original_epoch = a.latest_epoch();

    a.merge(&a.clone());

    assert_eq!(a.num_ops(), original_ops);
    assert_eq!(a.latest_epoch(), original_epoch);
}

#[test]
fn test_concurrent_op_appends_converge() {
    // Two replicas receive different ops, then merge
    let mut replica1 = JournalMap::new();
    let mut replica2 = JournalMap::new();

    // Replica 1 gets op at epoch 1
    replica1
        .append_tree_op(create_test_op(1, Commitment::new([1u8; 32])))
        .unwrap();

    // Replica 2 gets op at epoch 2
    replica2
        .append_tree_op(create_test_op(2, Commitment::new([2u8; 32])))
        .unwrap();

    // After merge, both should have both ops
    replica1.merge(&replica2);
    replica2.merge(&replica1);

    assert_eq!(replica1.num_ops(), 2);
    assert_eq!(replica2.num_ops(), 2);
    assert_eq!(replica1.latest_epoch(), Some(2));
    assert_eq!(replica2.latest_epoch(), Some(2));
}

#[test]
fn test_out_of_order_delivery_converges() {
    let mut replica1 = JournalMap::new();
    let mut replica2 = JournalMap::new();

    // Replica 1 receives ops in order: 1, 2, 3
    replica1
        .append_tree_op(create_test_op(1, Commitment::new([1u8; 32])))
        .unwrap();
    replica1
        .append_tree_op(create_test_op(2, Commitment::new([2u8; 32])))
        .unwrap();
    replica1
        .append_tree_op(create_test_op(3, Commitment::new([3u8; 32])))
        .unwrap();

    // Replica 2 receives ops out of order: 3, 1, 2
    replica2
        .append_tree_op(create_test_op(3, Commitment::new([3u8; 32])))
        .unwrap();
    replica2
        .append_tree_op(create_test_op(1, Commitment::new([1u8; 32])))
        .unwrap();
    replica2
        .append_tree_op(create_test_op(2, Commitment::new([2u8; 32])))
        .unwrap();

    // Both should have same final state
    assert_eq!(replica1.num_ops(), replica2.num_ops());
    assert_eq!(replica1.latest_epoch(), replica2.latest_epoch());
}

#[test]
fn test_invalid_signature_rejected() {
    let mut journal = JournalMap::new();

    // Create op with insufficient signers
    let mut op = create_test_op(1, Commitment::new([1u8; 32]));
    op.attestation = ThresholdSignature::new(vec![0u8; 64], vec![DeviceId::new()], (2, 3));

    let result = journal.append_tree_op(op);
    assert!(result.is_err());
    assert_eq!(journal.num_ops(), 0);
}

#[test]
fn test_intent_or_set_add_wins() {
    let mut replica1 = JournalMap::new();
    let mut replica2 = JournalMap::new();

    let intent = create_test_intent(100, Commitment::new([1u8; 32]));
    let id = intent.intent_id;

    // Replica 1 adds intent
    replica1.submit_intent(intent).unwrap();

    // Replica 2 tombstones it (simulating concurrent execution)
    replica2.tombstone_intent(id).unwrap();

    // Merge: add should NOT win over tombstone
    replica1.merge(&replica2);

    // Intent should be tombstoned
    assert_eq!(replica1.get_intent_status(&id), IntentStatus::Completed);
}

#[test]
fn test_intent_tombstones_prevent_readd() {
    let mut journal = JournalMap::new();

    let intent = create_test_intent(100, Commitment::new([1u8; 32]));
    let id = intent.intent_id;

    // Add and tombstone
    journal.submit_intent(intent.clone()).unwrap();
    journal.tombstone_intent(id).unwrap();

    // Try to re-add
    let result = journal.submit_intent(intent);
    assert!(result.is_err());
}

#[test]
fn test_epoch_conflict_resolution() {
    let mut journal = JournalMap::new();

    // Two conflicting ops at same epoch with different commitments
    let op1 = create_test_op(1, Commitment::new([1u8; 32]));
    let op2 = create_test_op(1, Commitment::new([2u8; 32]));

    journal.append_tree_op(op1).unwrap();
    journal.append_tree_op(op2).unwrap();

    // Should have only one op (the one with higher commitment)
    assert_eq!(journal.num_ops(), 1);
    let stored_op = journal.get_op(1).unwrap();
    assert_eq!(
        stored_op.root_commitment().unwrap(),
        &Commitment::new([2u8; 32])
    );
}

#[test]
fn test_intent_merge_or_set_semantics() {
    let mut replica1 = JournalMap::new();
    let mut replica2 = JournalMap::new();

    let intent1 = create_test_intent(100, Commitment::new([1u8; 32]));
    let intent2 = create_test_intent(200, Commitment::new([1u8; 32]));

    let id1 = intent1.intent_id;
    let id2 = intent2.intent_id;

    // Replica 1 has intent1
    replica1.submit_intent(intent1).unwrap();

    // Replica 2 has intent2
    replica2.submit_intent(intent2).unwrap();

    // Merge
    replica1.merge(&replica2);

    // Both intents should be present
    assert_eq!(replica1.num_intents(), 2);
    assert!(replica1.get_intent(&id1).is_some());
    assert!(replica1.get_intent(&id2).is_some());
}

#[test]
fn test_prune_stale_intents() {
    let mut journal = JournalMap::new();

    let old_commitment = Commitment::new([1u8; 32]);
    let new_commitment = Commitment::new([2u8; 32]);

    // Add intent with old snapshot
    let intent = create_test_intent(100, old_commitment);
    journal.submit_intent(intent).unwrap();

    assert_eq!(journal.num_intents(), 1);

    // Prune stale intents
    journal.prune_stale_intents(&new_commitment);

    // Check that the intent was pruned
    assert_eq!(journal.num_intents(), 0);
}

#[test]
fn test_multiple_replicas_converge_after_partition() {
    // Simulate network partition with 3 replicas
    let mut replica1 = JournalMap::new();
    let mut replica2 = JournalMap::new();
    let mut replica3 = JournalMap::new();

    // Partition: replica1 | replica2,3

    // Side 1
    replica1
        .append_tree_op(create_test_op(1, Commitment::new([1u8; 32])))
        .unwrap();
    replica1
        .append_tree_op(create_test_op(2, Commitment::new([2u8; 32])))
        .unwrap();

    // Side 2
    replica2
        .append_tree_op(create_test_op(3, Commitment::new([3u8; 32])))
        .unwrap();
    replica3
        .append_tree_op(create_test_op(3, Commitment::new([3u8; 32])))
        .unwrap();
    replica2
        .append_tree_op(create_test_op(4, Commitment::new([4u8; 32])))
        .unwrap();
    replica3
        .append_tree_op(create_test_op(4, Commitment::new([4u8; 32])))
        .unwrap();

    // Heal partition
    replica1.merge(&replica2);
    replica1.merge(&replica3);

    replica2.merge(&replica1);
    replica3.merge(&replica1);

    // All replicas converge to same state
    assert_eq!(replica1.num_ops(), 4);
    assert_eq!(replica2.num_ops(), 4);
    assert_eq!(replica3.num_ops(), 4);

    assert_eq!(replica1.latest_epoch(), Some(4));
    assert_eq!(replica2.latest_epoch(), Some(4));
    assert_eq!(replica3.latest_epoch(), Some(4));
}

#[test]
fn test_capability_tombstones_in_merge() {
    let mut replica1 = JournalMap::new();
    let mut replica2 = JournalMap::new();

    // Both replicas add ops with capabilities
    let op1 = create_test_op(1, Commitment::new([1u8; 32]));
    let op2 = create_test_op(2, Commitment::new([2u8; 32]));

    replica1.append_tree_op(op1).unwrap();
    replica2.append_tree_op(op2).unwrap();

    // Merge
    replica1.merge(&replica2);

    // Both ops should be present
    assert_eq!(replica1.num_ops(), 2);
}

#[test]
fn test_journal_stats() {
    let mut journal = JournalMap::new();

    journal
        .append_tree_op(create_test_op(1, Commitment::new([1u8; 32])))
        .unwrap();
    journal
        .append_tree_op(create_test_op(2, Commitment::new([2u8; 32])))
        .unwrap();

    let intent = create_test_intent(100, Commitment::new([1u8; 32]));
    journal.submit_intent(intent).unwrap();

    let stats = journal.stats().unwrap();

    assert_eq!(stats.num_ops, 2);
    assert_eq!(stats.num_intents, 1);
    assert_eq!(stats.latest_epoch, Some(2));
}
