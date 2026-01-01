use crate::algebra::OpLog;
use aura_core::{
    tree::{Epoch, Snapshot, TreeOp, TreeOpKind},
    AttestedOp, LeafId, NodeIndex,
};
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

    // Only add snapshot fact if there were operations before the cut
    if !before_cut.is_empty() {
        let snapshot_fact = create_snapshot_fact_operation(snapshot, snapshot.epoch)?;
        compacted.append(snapshot_fact);
    }

    // Add all operations after cut
    for op in after_cut {
        compacted.append(op.clone());
    }

    Ok(compacted)
}

/// Create a snapshot fact operation from a snapshot
fn create_snapshot_fact_operation(
    snapshot: &Snapshot,
    cut_epoch: Epoch,
) -> Result<AttestedOp, CompactionError> {
    // Represent the snapshot as a RotateEpoch fence; downstream reducers treat this
    // as an epoch boundary for compaction until a dedicated SnapshotFact type lands.
    let snapshot_op = TreeOpKind::RotateEpoch {
        affected: snapshot
            .roster
            .iter()
            .map(|leaf_id| NodeIndex(leaf_id.0))
            .collect(),
    };

    let tree_op = TreeOp {
        parent_epoch: Epoch::from(u64::from(cut_epoch).saturating_sub(1)),
        parent_commitment: snapshot.commitment,
        op: snapshot_op,
        version: 1,
    };

    // Create a minimal signature for the snapshot fact
    let aggregate_signature = create_snapshot_signature(&tree_op)?;

    let attested_op = AttestedOp {
        op: tree_op,
        agg_sig: aggregate_signature,
        signer_count: 1,
    };

    Ok(attested_op)
}

/// Serialize snapshot metadata for inclusion in snapshot fact
#[allow(dead_code)]
fn serialize_snapshot_metadata(snapshot: &Snapshot) -> Result<Vec<u8>, CompactionError> {
    use std::io::Write;

    let mut buffer = Vec::new();

    // Write epoch
    buffer
        .write_all(&u64::from(snapshot.epoch).to_be_bytes())
        .map_err(|e| CompactionError::SerializationError(e.to_string()))?;

    // Write tree hash
    buffer
        .write_all(&snapshot.commitment)
        .map_err(|e| CompactionError::SerializationError(e.to_string()))?;

    // Write roster size
    buffer
        .write_all(&(snapshot.roster.len() as u32).to_be_bytes())
        .map_err(|e| CompactionError::SerializationError(e.to_string()))?;

    // Write each leaf in roster
    for leaf_id in &snapshot.roster {
        let leaf_bytes = serialize_leaf_id(leaf_id)?;
        buffer
            .write_all(&(leaf_bytes.len() as u32).to_be_bytes())
            .map_err(|e| CompactionError::SerializationError(e.to_string()))?;
        buffer
            .write_all(&leaf_bytes)
            .map_err(|e| CompactionError::SerializationError(e.to_string()))?;
    }

    Ok(buffer)
}

/// Serialize a leaf ID for snapshot metadata
#[allow(dead_code)]
fn serialize_leaf_id(leaf_id: &LeafId) -> Result<Vec<u8>, CompactionError> {
    // This is simplified - real implementation would use proper serialization
    let mut buffer = Vec::new();

    // Write leaf ID
    buffer.extend_from_slice(&leaf_id.0.to_be_bytes());

    Ok(buffer)
}

/// Create a signature for the snapshot fact operation
fn create_snapshot_signature(
    tree_op: &aura_core::tree::TreeOp,
) -> Result<Vec<u8>, CompactionError> {
    use aura_core::hash;

    let mut h = hash::hasher();
    h.update(b"SNAPSHOT_FACT");
    h.update(&u64::from(tree_op.parent_epoch).to_be_bytes());
    h.update(&tree_op.parent_commitment);

    // Hash the operation
    let op_bytes = serialize_tree_op(tree_op)?;
    h.update(&op_bytes);

    Ok(h.finalize().to_vec())
}

/// Serialize a tree operation for hashing/signing
fn serialize_tree_op(tree_op: &aura_core::tree::TreeOp) -> Result<Vec<u8>, CompactionError> {
    // This is simplified - real implementation would use proper serialization
    let mut buffer = Vec::new();

    // Write parent epoch
    buffer.extend_from_slice(&u64::from(tree_op.parent_epoch).to_be_bytes());

    // Write parent commitment
    buffer.extend_from_slice(&tree_op.parent_commitment);

    // Write operation type and version
    buffer.extend_from_slice(&tree_op.version.to_be_bytes());

    match &tree_op.op {
        aura_core::TreeOpKind::RotateEpoch { affected } => {
            buffer.push(4); // Opcode for rotate epoch
            buffer.extend_from_slice(&(affected.len() as u32).to_be_bytes());
            for node in affected {
                buffer.extend_from_slice(&node.0.to_be_bytes());
            }
        }
        aura_core::TreeOpKind::AddLeaf { leaf, under } => {
            buffer.push(1); // Opcode for add leaf
            buffer.extend_from_slice(&leaf.leaf_id.0.to_be_bytes());
            buffer.extend_from_slice(&under.0.to_be_bytes());
        }
        aura_core::TreeOpKind::RemoveLeaf { leaf, reason } => {
            buffer.push(2); // Opcode for remove leaf
            buffer.extend_from_slice(&leaf.0.to_be_bytes());
            buffer.push(*reason);
        }
        aura_core::TreeOpKind::ChangePolicy { node, new_policy } => {
            buffer.push(3); // Opcode for change policy
            buffer.extend_from_slice(&node.0.to_be_bytes());
            // Serialize policy (simplified)
            let policy_bytes = serde_json::to_vec(new_policy)
                .map_err(|e| CompactionError::SerializationError(e.to_string()))?;
            buffer.extend_from_slice(&policy_bytes);
        }
    }

    Ok(buffer)
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
        if let aura_core::TreeOpKind::AddLeaf { leaf, .. } = &op.op.op {
            leaves_in_ops.insert(leaf.leaf_id);
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
    use crate::algebra::JoinSemilattice;

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

    let original_cids: BTreeSet<[u8; 32]> = oplog
        .list_ops()
        .iter()
        .map(|op| op.op.parent_commitment)
        .collect();

    let compacted_cids: BTreeSet<[u8; 32]> = compacted
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

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::tree::{LeafNode, TreeOp, TreeOpKind};
    use aura_core::{Epoch, LeafId, NodeIndex, Policy};
    use std::collections::BTreeMap;

    fn create_test_op(epoch: u64, commitment: [u8; 32], leaf_id: u8) -> AttestedOp {
        let device_id = aura_core::DeviceId(uuid::Uuid::from_bytes([8u8; 16]));
        AttestedOp {
            op: TreeOp {
                parent_commitment: commitment,
                parent_epoch: Epoch::new(epoch),
                op: TreeOpKind::AddLeaf {
                    leaf: LeafNode::new_device(LeafId(leaf_id as u32), device_id, vec![1, 2, 3])
                        .expect("valid leaf"),
                    under: NodeIndex(0),
                },
                version: 1,
            },
            agg_sig: vec![1u8; 64], // Dummy signature for tests
            signer_count: 1,
        }
    }

    fn create_test_snapshot(epoch: u64, leaf_ids: Vec<u64>) -> Snapshot {
        Snapshot::new(
            Epoch::new(epoch),
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
        oplog.append(create_test_op(1, [1u8; 32], 1));
        oplog.append(create_test_op(2, [2u8; 32], 2));
        oplog.append(create_test_op(3, [3u8; 32], 3));

        let snapshot = create_test_snapshot(5, vec![1, 2, 3]);

        let compacted = compact(&oplog, &snapshot).unwrap();
        // All ops before cut should be replaced with one snapshot fact
        assert_eq!(compacted.list_ops().len(), 1);
    }

    #[test]
    fn test_compact_all_after_cut() {
        let mut oplog = OpLog::new();
        oplog.append(create_test_op(6, [1u8; 32], 1));
        oplog.append(create_test_op(7, [2u8; 32], 2));

        let snapshot = create_test_snapshot(5, vec![1]);

        let compacted = compact(&oplog, &snapshot).unwrap();
        // All ops after cut should be preserved
        assert_eq!(compacted.list_ops().len(), 2);
    }

    #[test]
    fn test_compact_mixed() {
        let mut oplog = OpLog::new();
        oplog.append(create_test_op(1, [1u8; 32], 1));
        oplog.append(create_test_op(2, [2u8; 32], 2));
        oplog.append(create_test_op(6, [3u8; 32], 3));
        oplog.append(create_test_op(7, [4u8; 32], 4));

        let snapshot = create_test_snapshot(5, vec![1, 2]);

        let compacted = compact(&oplog, &snapshot).unwrap();
        // Snapshot fact + 2 ops after epoch 5 should remain
        assert_eq!(compacted.list_ops().len(), 3);

        let cids: Vec<[u8; 32]> = compacted
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
        let mut oplog1 = OpLog::new();
        oplog1.append(create_test_op(1, [1u8; 32], 1));

        let mut oplog2 = OpLog::new();
        oplog2.append(create_test_op(2, [2u8; 32], 2));

        let snapshot = create_test_snapshot(5, vec![1, 2]);

        let result = verify_join_preserving(&oplog1, &oplog2, &snapshot);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_verify_retraction() {
        let mut oplog = OpLog::new();
        oplog.append(create_test_op(1, [1u8; 32], 1));
        oplog.append(create_test_op(2, [2u8; 32], 2));
        oplog.append(create_test_op(6, [3u8; 32], 3));

        let snapshot = create_test_snapshot(5, vec![1, 2]);

        let result = verify_retraction(&oplog, &snapshot);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_idempotent_compaction() {
        let mut oplog = OpLog::new();
        oplog.append(create_test_op(1, [1u8; 32], 1));
        oplog.append(create_test_op(6, [2u8; 32], 2));

        let snapshot = create_test_snapshot(5, vec![1]);

        let compacted1 = compact(&oplog, &snapshot).unwrap();
        let compacted2 = compact(&compacted1, &snapshot).unwrap();

        // compact(compact(x)) = compact(x)
        assert_eq!(compacted1.list_ops().len(), compacted2.list_ops().len());
    }
}
