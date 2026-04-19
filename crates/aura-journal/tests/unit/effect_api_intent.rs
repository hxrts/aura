use crate::common::{physical_time_ms, test_device_id, test_uuid};
use aura_core::{Hash32, LeafId, LeafNode, NodeIndex, Policy, TreeOpKind as TreeOperation};
use aura_journal::effect_api::intent::{Intent, IntentBatch, IntentId, IntentStatus, Priority};

#[test]
fn intent_id_creation() {
    let id1 = IntentId::new(test_uuid(1));
    let id2 = IntentId::new(test_uuid(2));
    assert_ne!(id1, id2);
}

#[test]
fn priority_values() {
    assert!(Priority::high() > Priority::default_priority());
    assert!(Priority::default_priority() > Priority::low());
}

#[test]
fn intent_creation() {
    let leaf = match LeafNode::new_device(LeafId(0), test_device_id(9), vec![0u8; 32]) {
        Ok(leaf) => leaf,
        Err(err) => panic!("leaf: {err}"),
    };

    let op = TreeOperation::AddLeaf {
        leaf,
        under: NodeIndex(0),
    };

    let intent = Intent::new(
        IntentId::new(test_uuid(6)),
        op,
        vec![NodeIndex(0)],
        Hash32([0u8; 32]),
        Priority::default_priority(),
        test_device_id(1),
        physical_time_ms(1000),
    );

    assert_eq!(intent.priority, Priority::default_priority());
    assert!(!intent.is_stale(&Hash32([0u8; 32])));
}

#[test]
#[allow(clippy::disallowed_methods)]
fn intent_conflicts() {
    let op = TreeOperation::RotateEpoch {
        affected: vec![NodeIndex(0)],
    };

    let intent1 = Intent::new(
        IntentId::new(test_uuid(4)),
        op.clone(),
        vec![NodeIndex(0), NodeIndex(1)],
        Hash32([1u8; 32]),
        Priority::default_priority(),
        test_device_id(1),
        physical_time_ms(1000),
    );
    let intent2 = Intent::new(
        IntentId::new(test_uuid(5)),
        op,
        vec![NodeIndex(1), NodeIndex(2)],
        Hash32([2u8; 32]),
        Priority::default_priority(),
        test_device_id(1),
        physical_time_ms(1000),
    );

    assert!(intent1.conflicts_with(&intent2));
}

#[test]
#[allow(clippy::disallowed_methods)]
fn intent_no_conflict_same_snapshot() {
    let op = TreeOperation::RotateEpoch {
        affected: vec![NodeIndex(0)],
    };
    let snapshot = [1u8; 32];

    let intent1 = Intent::new(
        IntentId::new(test_uuid(7)),
        op.clone(),
        vec![NodeIndex(0), NodeIndex(1)],
        Hash32(snapshot),
        Priority::default_priority(),
        test_device_id(1),
        physical_time_ms(1000),
    );
    let intent2 = Intent::new(
        IntentId::new(test_uuid(7)),
        op,
        vec![NodeIndex(1), NodeIndex(2)],
        Hash32(snapshot),
        Priority::default_priority(),
        test_device_id(1),
        physical_time_ms(1000),
    );

    assert!(!intent1.conflicts_with(&intent2));
}

#[test]
#[allow(clippy::disallowed_methods)]
fn intent_is_stale() {
    let intent = Intent::new(
        IntentId::new(test_uuid(7)),
        TreeOperation::RotateEpoch {
            affected: vec![NodeIndex(0)],
        },
        vec![],
        Hash32([1u8; 32]),
        Priority::default_priority(),
        test_device_id(1),
        physical_time_ms(1000),
    );

    assert!(!intent.is_stale(&Hash32([1u8; 32])));
    assert!(intent.is_stale(&Hash32([2u8; 32])));
}

#[test]
#[allow(clippy::disallowed_methods)]
fn intent_age() {
    let intent = Intent::new(
        IntentId::new(test_uuid(7)),
        TreeOperation::RemoveLeaf {
            leaf: LeafId(0),
            reason: 0,
        },
        vec![],
        Hash32([0u8; 32]),
        Priority::default_priority(),
        test_device_id(1),
        physical_time_ms(1000),
    );

    assert!(intent.age(&physical_time_ms(1500)).is_some());
    assert_eq!(intent.age(&physical_time_ms(1000)), Some(0));
}

#[test]
#[allow(clippy::disallowed_methods)]
fn intent_batch_add() {
    let snapshot = [1u8; 32];
    let mut batch = IntentBatch::new(Hash32(snapshot));

    let intent = Intent::new(
        IntentId::new(test_uuid(7)),
        TreeOperation::ChangePolicy {
            node: NodeIndex(0),
            new_policy: Policy::All,
        },
        vec![NodeIndex(0)],
        Hash32(snapshot),
        Priority::default_priority(),
        test_device_id(1),
        physical_time_ms(1000),
    );

    let result = batch.try_add(intent);
    assert!(result.is_ok());
    assert_eq!(batch.len(), 1);
}

#[test]
#[allow(clippy::disallowed_methods)]
fn intent_batch_rejects_snapshot_mismatch() {
    let snapshot1 = [1u8; 32];
    let snapshot2 = [2u8; 32];
    let mut batch = IntentBatch::new(Hash32(snapshot1));

    let intent = Intent::new(
        IntentId::new(test_uuid(7)),
        TreeOperation::RotateEpoch {
            affected: vec![NodeIndex(0)],
        },
        vec![NodeIndex(0)],
        Hash32(snapshot2),
        Priority::default_priority(),
        test_device_id(1),
        physical_time_ms(1000),
    );

    let result = batch.try_add(intent);
    assert!(result.is_err());
}

#[test]
#[allow(clippy::disallowed_methods)]
fn intent_batch_intent_ids() {
    let snapshot = [1u8; 32];
    let mut batch = IntentBatch::new(Hash32(snapshot));
    let intent = Intent::new(
        IntentId::new(test_uuid(7)),
        TreeOperation::RotateEpoch {
            affected: vec![NodeIndex(0)],
        },
        vec![NodeIndex(0)],
        Hash32(snapshot),
        Priority::default_priority(),
        test_device_id(1),
        physical_time_ms(1000),
    );

    let id = intent.intent_id;
    batch.try_add(intent).unwrap();

    let ids = batch.intent_ids();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0], id);
}

#[test]
fn intent_status_display() {
    assert_eq!(IntentStatus::Pending.to_string(), "pending");
    assert_eq!(IntentStatus::Executing.to_string(), "executing");
    assert_eq!(IntentStatus::Completed.to_string(), "completed");
}
