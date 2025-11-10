use crate::semilattice::OpLog;
use aura_core::tree::{AttestedOp, Snapshot};
use std::collections::BTreeSet;

/// Compact an OpLog by replacing history before a snapshot with the snapshot fact
///
/// This implements a retraction homomorphism h: OpLog → OpLog where:
/// - h(x ⊔ y) = h(x) ⊔ h(y)  (join-preserving)
/// - h(x) ≤ x                  (retraction property)
///
/// ## Algorithm
///
/// 1. Partition OpLog into two sets: before_cut and after_cut
/// 2. Replace before_cut with snapshot fact
/// 3. Keep after_cut unchanged
/// 4. Return compacted OpLog = {snapshot_fact} ∪ after_cut
///
/// ## Invariants
///
/// - Snapshot must be valid and include all operations up to cut point
/// - Operations after cut point are preserved exactly
/// - Join semantics are preserved (other replicas can still merge)
/// - Compaction is idempotent: compact(compact(x)) = compact(x)
///
/// ## Parameters
///
/// - `oplog`: The original OpLog to compact
/// - `snapshot`: The snapshot representing the cut point
///
/// ## Returns
///
/// Compacted OpLog with history replaced by snapshot fact
pub fn compact(oplog: &OpLog, snapshot: &Snapshot) -> Result<OpLog, CompactionError> {
    // Validate snapshot
    snapshot
        .validate()
        .map_err(|e| CompactionError::InvalidSnapshot(e.to_string()))?;

    // Get all operations from OpLog
    let all_ops: Vec<AttestedOp> = oplog.list_ops().into_iter().cloned().collect();

    // Partition operations: before and after snapshot cut
    let (before_cut, after_cut) = partition_by_epoch(&all_ops, snapshot);

    // Verify snapshot covers all before_cut operations
    verify_snapshot_coverage(&before_cut, snapshot)?;

    // Create new OpLog with snapshot fact
    let mut compacted = OpLog::new();

    // Add snapshot as a special "fact" operation
    // TODO fix - In a real implementation, this would be a special OpKind::SnapshotFact
    // TODO fix - For now, we just skip adding before_cut and keep after_cut

    // Add all operations after cut
    for op in after_cut {
        compacted.append(op.clone());
    }

    Ok(compacted)
}

/// Partition operations by epoch relative to snapshot
fn partition_by_epoch(
    ops: &[AttestedOp],
    snapshot: &Snapshot,
) -> (Vec<AttestedOp>, Vec<AttestedOp>) {
    let mut before_cut = Vec::new();
    let mut after_cut = Vec::new();

    for op in ops {
        if op.op.parent_epoch < snapshot.epoch {
            before_cut.push(op.clone());
        } else {
            after_cut.push(op.clone());
        }
    }

    (before_cut, after_cut)
}

/// Verify snapshot covers all operations before the cut point
fn verify_snapshot_coverage(
    before_cut: &[AttestedOp],
    snapshot: &Snapshot,
) -> Result<(), CompactionError> {
    // Verify all leaves in before_cut are present in snapshot roster
    let mut leaves_in_ops = BTreeSet::new();

    for op in before_cut {
        match &op.op.op {
            aura_core::TreeOpKind::AddLeaf { leaf, .. } => {
                leaves_in_ops.insert(leaf.leaf_id);
            }
            _ => {}
        }
    }

    for leaf_id in leaves_in_ops {
        if !snapshot.contains_leaf(&leaf_id) {
            return Err(CompactionError::MissingLeafInSnapshot(leaf_id));
        }
    }

    Ok(())
}

/// Verify compaction preserves join-semilattice properties
///
/// Property test: h(x ⊔ y) = h(x) ⊔ h(y)
///
/// This is a property-based test helper for testing the homomorphism law.
#[cfg(test)]
pub fn verify_join_preserving(
    oplog1: &OpLog,
    oplog2: &OpLog,
    snapshot: &Snapshot,
) -> Result<bool, CompactionError> {
    use crate::semilattice::JoinSemilattice;

    // Compute h(x ⊔ y)
    let joined = oplog1.join(oplog2);
    let h_of_join = compact(&joined, snapshot)?;

    // Compute h(x) ⊔ h(y)
    let h_x = compact(oplog1, snapshot)?;
    let h_y = compact(oplog2, snapshot)?;
    let join_of_h = h_x.join(&h_y);

    // They should be equal
    Ok(h_of_join.list_ops() == join_of_h.list_ops())
}

/// Verify retraction property: h(x) ≤ x
///
/// After compaction, the result should be a subset of the original.
#[cfg(test)]
pub fn verify_retraction(oplog: &OpLog, snapshot: &Snapshot) -> Result<bool, CompactionError> {
    let compacted = compact(oplog, snapshot)?;

    let original_cids: BTreeSet<Hash32> = oplog
        .list_ops()
        .iter()
        .map(|op| op.op.parent_commitment)
        .collect();

    let compacted_cids: BTreeSet<Hash32> = compacted
        .list_ops()
        .iter()
        .map(|op| op.op.parent_commitment)
        .collect();

    // Compacted should be subset of original
    Ok(compacted_cids.is_subset(&original_cids))
}

/// Errors that can occur during compaction
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompactionError {
    /// Invalid snapshot
    #[error("Invalid snapshot: {0}")]
    InvalidSnapshot(String),

    /// Snapshot doesn't cover all operations before cut
    #[error("Snapshot missing leaf {0:?} from before-cut operations")]
    MissingLeafInSnapshot(aura_core::tree::LeafId),

    /// Compaction would violate invariants
    #[error("Compaction invariant violation: {0}")]
    InvariantViolation(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{
        tree::{Epoch, LeafId, LeafNode, LeafRole, NodeIndex, Policy, TreeOp, TreeOpKind},
        Hash32,
    };
    use std::collections::BTreeMap;

    fn create_test_op(epoch: u64, commitment: Hash32, leaf_id: u8) -> AttestedOp {
        let device_id = aura_core::DeviceId::new();
        AttestedOp {
            op: TreeOp {
                parent_commitment: commitment,
                parent_epoch: epoch,
                op: TreeOpKind::AddLeaf {
                    leaf: LeafNode::new_device(LeafId(leaf_id as u32), device_id, vec![1, 2, 3]),
                    under: NodeIndex(0),
                },
                version: 1,
            },
            agg_sig: vec![],
            signer_count: 1,
        }
    }

    fn create_test_snapshot(epoch: u64, leaf_ids: Vec<u64>) -> Snapshot {
        Snapshot::new(
            epoch,
            [1u8; 32],
            leaf_ids.into_iter().map(|id| LeafId(id as u32)).collect(),
            BTreeMap::from([(NodeIndex(0), Policy::Any)]),
            1000,
        )
    }

    #[test]
    fn test_compact_empty_oplog() {
        let oplog = OpLog::new();
        let snapshot = create_test_snapshot(5, vec![1]);

        let result = compact(&oplog, &snapshot);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().list_ops().len(), 0);
    }

    #[test]
    fn test_compact_all_before_cut() {
        let mut oplog = OpLog::new();
        oplog.add_operation(create_test_op(1, [1u8; 32], 1));
        oplog.add_operation(create_test_op(2, [2u8; 32], 2));
        oplog.add_operation(create_test_op(3, [3u8; 32], 3));

        let snapshot = create_test_snapshot(5, vec![1, 2, 3]);

        let compacted = compact(&oplog, &snapshot).unwrap();
        // All ops before cut should be removed
        assert_eq!(compacted.list_ops().len(), 0);
    }

    #[test]
    fn test_compact_all_after_cut() {
        let mut oplog = OpLog::new();
        oplog.add_operation(create_test_op(6, [1u8; 32], 1));
        oplog.add_operation(create_test_op(7, [2u8; 32], 2));

        let snapshot = create_test_snapshot(5, vec![1]);

        let compacted = compact(&oplog, &snapshot).unwrap();
        // All ops after cut should be preserved
        assert_eq!(compacted.list_ops().len(), 2);
    }

    #[test]
    fn test_compact_mixed() {
        let mut oplog = OpLog::new();
        oplog.add_operation(create_test_op(1, [1u8; 32], 1));
        oplog.add_operation(create_test_op(2, [2u8; 32], 2));
        oplog.add_operation(create_test_op(6, [3u8; 32], 3));
        oplog.add_operation(create_test_op(7, [4u8; 32], 4));

        let snapshot = create_test_snapshot(5, vec![1, 2]);

        let compacted = compact(&oplog, &snapshot).unwrap();
        // Only ops after epoch 5 should remain
        assert_eq!(compacted.list_ops().len(), 2);

        let cids: Vec<Hash32> = compacted
            .list_ops()
            .iter()
            .map(|op| op.op.parent_commitment)
            .collect();
        assert!(cids.contains(&[3u8; 32]));
        assert!(cids.contains(&[4u8; 32]));
    }

    #[test]
    fn test_partition_by_epoch() {
        let ops = vec![
            create_test_op(1, [1u8; 32], 1),
            create_test_op(3, [2u8; 32], 2),
            create_test_op(5, [3u8; 32], 3),
            create_test_op(7, [4u8; 32], 4),
        ];

        let snapshot = create_test_snapshot(5, vec![1, 2, 3]);
        let (before, after) = partition_by_epoch(&ops, &snapshot);

        assert_eq!(before.len(), 2); // Epochs 1, 3
        assert_eq!(after.len(), 2); // Epochs 5, 7
    }

    #[test]
    fn test_verify_snapshot_coverage_success() {
        let before_cut = vec![
            create_test_op(1, [1u8; 32], 1),
            create_test_op(2, [2u8; 32], 2),
        ];

        let snapshot = create_test_snapshot(5, vec![1, 2]);

        assert!(verify_snapshot_coverage(&before_cut, &snapshot).is_ok());
    }

    #[test]
    fn test_verify_snapshot_coverage_missing_leaf() {
        let before_cut = vec![
            create_test_op(1, [1u8; 32], 1),
            create_test_op(2, [2u8; 32], 2),
            create_test_op(3, [3u8; 32], 3),
        ];

        let snapshot = create_test_snapshot(5, vec![1, 2]); // Missing leaf 3

        let result = verify_snapshot_coverage(&before_cut, &snapshot);
        assert!(matches!(
            result,
            Err(CompactionError::MissingLeafInSnapshot(LeafId(3)))
        ));
    }

    #[test]
    fn test_verify_join_preserving() {
        use crate::semilattice::JoinSemilattice;

        let mut oplog1 = OpLog::new();
        oplog1.add_operation(create_test_op(1, [1u8; 32], 1));

        let mut oplog2 = OpLog::new();
        oplog2.add_operation(create_test_op(2, [2u8; 32], 2));

        let snapshot = create_test_snapshot(5, vec![1, 2]);

        let result = verify_join_preserving(&oplog1, &oplog2, &snapshot);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_verify_retraction() {
        let mut oplog = OpLog::new();
        oplog.add_operation(create_test_op(1, [1u8; 32], 1));
        oplog.add_operation(create_test_op(2, [2u8; 32], 2));
        oplog.add_operation(create_test_op(6, [3u8; 32], 3));

        let snapshot = create_test_snapshot(5, vec![1, 2]);

        let result = verify_retraction(&oplog, &snapshot);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_idempotent_compaction() {
        let mut oplog = OpLog::new();
        oplog.add_operation(create_test_op(1, [1u8; 32], 1));
        oplog.add_operation(create_test_op(6, [2u8; 32], 2));

        let snapshot = create_test_snapshot(5, vec![1]);

        let compacted1 = compact(&oplog, &snapshot).unwrap();
        let compacted2 = compact(&compacted1, &snapshot).unwrap();

        // compact(compact(x)) = compact(x)
        assert_eq!(compacted1.list_ops().len(), compacted2.list_ops().len());
    }
}
