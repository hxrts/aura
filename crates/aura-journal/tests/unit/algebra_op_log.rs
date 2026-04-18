use aura_core::{Epoch, LeafId, LeafNode, NodeIndex, TreeOp, TreeOpKind};
use aura_journal::algebra::OpLog;
use aura_journal::AttestedOp;
use aura_journal::Hash32;

fn create_test_operation(leaf_id: u32, parent_epoch: Epoch) -> AttestedOp {
    AttestedOp {
        op: TreeOp {
            parent_epoch,
            parent_commitment: [0u8; 32],
            op: TreeOpKind::AddLeaf {
                leaf: LeafNode::new_device(
                    LeafId(leaf_id),
                    aura_core::DeviceId(uuid::Uuid::from_bytes([5u8; 16])),
                    vec![leaf_id as u8; 32],
                )
                .expect("valid leaf"),
                under: NodeIndex(0),
            },
            version: 1,
        },
        agg_sig: vec![42u8; 64],
        signer_count: 3,
    }
}

fn compute_operation_cid(op: &AttestedOp) -> Hash32 {
    let mut h = aura_core::hash::hasher();

    h.update(&u64::from(op.op.parent_epoch).to_le_bytes());
    h.update(&op.op.parent_commitment);
    h.update(&op.op.version.to_le_bytes());

    match &op.op.op {
        aura_core::TreeOpKind::AddLeaf { leaf, under } => {
            h.update(b"AddLeaf");
            h.update(&leaf.leaf_id.0.to_le_bytes());
            h.update(&under.0.to_le_bytes());
            h.update(&leaf.public_key);
        }
        aura_core::TreeOpKind::RemoveLeaf { leaf, reason } => {
            h.update(b"RemoveLeaf");
            h.update(&leaf.0.to_le_bytes());
            h.update(&[*reason]);
        }
        aura_core::TreeOpKind::ChangePolicy { node, new_policy } => {
            h.update(b"ChangePolicy");
            h.update(&node.0.to_le_bytes());
            h.update(&aura_core::policy_hash(new_policy));
        }
        aura_core::TreeOpKind::RotateEpoch { affected } => {
            h.update(b"RotateEpoch");
            for node in affected {
                h.update(&node.0.to_le_bytes());
            }
        }
    }

    h.update(&op.signer_count.to_le_bytes());
    h.update(&op.agg_sig);

    Hash32(h.finalize())
}

#[test]
fn oplog_creation() {
    let log = OpLog::new();
    assert!(log.is_empty());
    assert_eq!(log.len(), 0);
    assert_eq!(log.version(), 0);
}

#[test]
fn add_operation() {
    let mut log = OpLog::new();
    let op = create_test_operation(1, Epoch::initial());

    let cid = log.add_operation(op.clone());
    assert_eq!(log.len(), 1);
    assert_eq!(log.version(), 1);
    assert!(log.contains_operation(&cid));
    assert_eq!(log.get_operation(&cid), Some(&op));
}

#[test]
fn duplicate_operations() {
    let mut log = OpLog::new();
    let op = create_test_operation(1, Epoch::initial());

    let cid1 = log.add_operation(op.clone());
    let cid2 = log.add_operation(op);

    assert_eq!(cid1, cid2);
    assert_eq!(log.len(), 1);
    assert_eq!(log.version(), 1);
}

#[test]
fn operation_ordering() {
    let mut log = OpLog::new();
    let op1 = create_test_operation(1, Epoch::initial());
    let op2 = create_test_operation(2, Epoch::initial());
    let op3 = create_test_operation(3, Epoch::initial());

    log.add_operation(op3);
    log.add_operation(op1);
    log.add_operation(op2);

    let operations = log.get_all_operations();
    assert_eq!(operations.len(), 3);

    let cids: Vec<_> = operations
        .iter()
        .map(|op| compute_operation_cid(op))
        .collect();
    let mut sorted_cids = cids.clone();
    sorted_cids.sort();
    assert_eq!(cids, sorted_cids);
}

#[test]
fn join_semilattice_properties() {
    use aura_core::semilattice::JoinSemilattice;

    let mut log1 = OpLog::new();
    let mut log2 = OpLog::new();

    let op1 = create_test_operation(1, Epoch::initial());
    let op2 = create_test_operation(2, Epoch::initial());
    let op3 = create_test_operation(3, Epoch::initial());

    log1.add_operation(op1);
    log1.add_operation(op2.clone());
    log2.add_operation(op2);
    log2.add_operation(op3);

    let joined = log1.join(&log2);
    assert_eq!(joined.len(), 3);

    let joined_reverse = log2.join(&log1);
    assert_eq!(joined.get_all_cids(), joined_reverse.get_all_cids());

    let self_joined = log1.join(&log1);
    assert_eq!(self_joined.get_all_cids(), log1.get_all_cids());
}

#[test]
fn merge_with() {
    let mut log1 = OpLog::new();
    let mut log2 = OpLog::new();

    let op1 = create_test_operation(1, Epoch::initial());
    let op2 = create_test_operation(2, Epoch::initial());

    log1.add_operation(op1.clone());
    log2.add_operation(op2.clone());

    let initial_version = log1.version();
    log1.merge_with(&log2);

    assert_eq!(log1.len(), 2);
    assert!(log1.version() > initial_version);
    assert!(log1.contains_operation(&compute_operation_cid(&op1)));
    assert!(log1.contains_operation(&compute_operation_cid(&op2)));
}

#[test]
fn filtering_and_queries() {
    let mut log = OpLog::new();
    let op1 = create_test_operation(1, Epoch::initial());
    let op2 = create_test_operation(2, Epoch::new(5));
    let op3 = create_test_operation(3, Epoch::new(10));

    log.add_operation(op1);
    log.add_operation(op2);
    log.add_operation(op3);

    let early_ops = log.operations_in_epoch_range(Epoch::initial(), Epoch::new(5));
    assert_eq!(early_ops.len(), 2);

    let latest = log.latest_operation().expect("latest op");
    assert_eq!(latest.op.parent_epoch, Epoch::new(10));

    let epoch_zero_ops = log.filter(|op| op.op.parent_epoch == Epoch::initial());
    assert_eq!(epoch_zero_ops.len(), 1);
}

#[test]
fn partial_ordering() {
    let mut log1 = OpLog::new();
    let mut log2 = OpLog::new();

    let op1 = create_test_operation(1, Epoch::initial());
    let op2 = create_test_operation(2, Epoch::initial());

    log1.add_operation(op1.clone());
    log2.add_operation(op1);
    log2.add_operation(op2);

    assert!(log1 < log2);
    assert!(log2 > log1);
    assert_eq!(log1.partial_cmp(&log1), Some(std::cmp::Ordering::Equal));
}

#[test]
fn summary_and_sync() {
    let mut log1 = OpLog::new();
    let mut log2 = OpLog::new();

    let op1 = create_test_operation(1, Epoch::initial());
    let op2 = create_test_operation(2, Epoch::initial());

    log1.add_operation(op1);
    log2.add_operation(op2);

    let summary1 = log1.create_summary();
    let summary2 = log2.create_summary();

    assert!(!log1.contains_all_from_summary(&summary2));
    assert!(!log2.contains_all_from_summary(&summary1));

    let missing_in_log1 = summary1.missing_cids(&summary2);
    assert_eq!(missing_in_log1.len(), 1);
}

#[test]
fn compaction() {
    let mut log = OpLog::new();

    let op1 = create_test_operation(1, Epoch::initial());
    let op2 = create_test_operation(2, Epoch::new(5));
    let op3 = create_test_operation(3, Epoch::new(10));

    log.add_operation(op1);
    log.add_operation(op2);
    log.add_operation(op3);

    assert_eq!(log.len(), 3);

    let removed = log.compact_before_epoch(Epoch::new(8));
    assert_eq!(removed, 2);
    assert_eq!(log.len(), 1);
}

#[test]
fn cid_determinism() {
    let op1 = create_test_operation(1, Epoch::initial());
    let op2 = create_test_operation(1, Epoch::initial());

    let cid1 = compute_operation_cid(&op1);
    let cid2 = compute_operation_cid(&op2);
    assert_eq!(cid1, cid2);

    let op3 = create_test_operation(2, Epoch::initial());
    let cid3 = compute_operation_cid(&op3);
    assert_ne!(cid1, cid3);
}
