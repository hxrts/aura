//! Pure anti-entropy decision helpers.
//!
//! These functions contain no I/O or side effects and can be property tested.

use super::effects::{BloomDigest, SyncError};
use aura_core::{tree::AttestedOp, Hash32};
use std::collections::BTreeSet;

/// Compute which ops should be pushed to a peer based on local/remote digests.
pub fn compute_ops_to_push(
    oplog: &[AttestedOp],
    local: &BloomDigest,
    remote: &BloomDigest,
) -> Result<Vec<AttestedOp>, SyncError> {
    let mut result = Vec::new();

    for op in oplog.iter() {
        let cid = Hash32::from(op.op.parent_commitment);
        if local.cids.contains(&cid) && !remote.cids.contains(&cid) {
            result.push(op.clone());
        }
    }

    Ok(result)
}

/// Compute which CIDs should be pulled from a peer based on digests.
pub fn compute_cids_to_pull(local: &BloomDigest, remote: &BloomDigest) -> BTreeSet<Hash32> {
    remote
        .cids
        .iter()
        .filter(|&cid| !local.cids.contains(cid))
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{tree::TreeOpKind, Epoch, TreeOp};
    use aura_journal::{LeafId, LeafNode, NodeIndex};

    fn create_test_op(commitment: Hash32) -> AttestedOp {
        AttestedOp {
            op: TreeOp {
                parent_commitment: commitment.0,
                parent_epoch: Epoch::new(1),
                op: TreeOpKind::AddLeaf {
                    leaf: LeafNode::new_device(
                        LeafId(1),
                        aura_core::identifiers::DeviceId::new_from_entropy([3u8; 32]),
                        vec![1, 2, 3],
                    )
                    .expect("valid leaf"),
                    under: NodeIndex(0),
                },
                version: 1,
            },
            agg_sig: vec![],
            signer_count: 1,
        }
    }

    #[test]
    fn compute_ops_to_push_only_missing_remote() {
        let op1 = create_test_op(Hash32([1u8; 32]));
        let op2 = create_test_op(Hash32([2u8; 32]));
        let oplog = vec![op1, op2];

        let local = BloomDigest {
            cids: [Hash32([1u8; 32]), Hash32([2u8; 32])].into_iter().collect(),
        };
        let remote = BloomDigest {
            cids: [Hash32([2u8; 32])].into_iter().collect(),
        };

        let result = compute_ops_to_push(&oplog, &local, &remote).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].op.parent_commitment, Hash32([1u8; 32]).0);
    }

    #[test]
    fn compute_cids_to_pull_only_missing_local() {
        let local = BloomDigest {
            cids: [Hash32([1u8; 32])].into_iter().collect(),
        };
        let remote = BloomDigest {
            cids: [Hash32([1u8; 32]), Hash32([2u8; 32])].into_iter().collect(),
        };

        let result = compute_cids_to_pull(&local, &remote);
        assert_eq!(result.len(), 1);
        assert!(result.contains(&Hash32([2u8; 32])));
    }
}
