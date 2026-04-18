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
    use crate::test_support::{create_test_op, digest_from_hashes};

    #[test]
    fn compute_ops_to_push_only_missing_remote() {
        let op1 = create_test_op(Hash32([1u8; 32]));
        let op2 = create_test_op(Hash32([2u8; 32]));
        let oplog = vec![op1, op2];

        let local = digest_from_hashes([Hash32([1u8; 32]), Hash32([2u8; 32])]);
        let remote = digest_from_hashes([Hash32([2u8; 32])]);

        let result = compute_ops_to_push(&oplog, &local, &remote).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].op.parent_commitment, Hash32([1u8; 32]).0);
    }

    #[test]
    fn compute_cids_to_pull_only_missing_local() {
        let local = digest_from_hashes([Hash32([1u8; 32])]);
        let remote = digest_from_hashes([Hash32([1u8; 32]), Hash32([2u8; 32])]);

        let result = compute_cids_to_pull(&local, &remote);
        assert_eq!(result.len(), 1);
        assert!(result.contains(&Hash32([2u8; 32])));
    }

    /// Calling reconciliation twice with identical inputs must produce
    /// identical outputs. If non-deterministic, peers derive different
    /// merged views from the same state — silent divergence.
    #[test]
    fn reconciliation_is_deterministic() {
        let op1 = create_test_op(Hash32([1u8; 32]));
        let op2 = create_test_op(Hash32([2u8; 32]));
        let op3 = create_test_op(Hash32([3u8; 32]));
        let oplog = vec![op1, op2, op3];

        let local = digest_from_hashes([Hash32([1u8; 32]), Hash32([2u8; 32]), Hash32([3u8; 32])]);
        let remote = digest_from_hashes([Hash32([2u8; 32])]);

        let push_a = compute_ops_to_push(&oplog, &local, &remote).unwrap();
        let push_b = compute_ops_to_push(&oplog, &local, &remote).unwrap();
        assert_eq!(push_a.len(), push_b.len());
        for (a, b) in push_a.iter().zip(push_b.iter()) {
            assert_eq!(a.op.parent_commitment, b.op.parent_commitment);
        }

        let pull_a = compute_cids_to_pull(&local, &remote);
        let pull_b = compute_cids_to_pull(&local, &remote);
        assert_eq!(pull_a, pull_b);
    }

    /// Symmetric reconciliation: A→B push set and B→A pull set should
    /// identify the same missing CIDs (just from different perspectives).
    #[test]
    fn reconciliation_is_symmetric() {
        let local = digest_from_hashes([Hash32([1u8; 32]), Hash32([2u8; 32])]);
        let remote = digest_from_hashes([Hash32([2u8; 32]), Hash32([3u8; 32])]);

        // A pushes to B: ops that A has but B doesn't
        let op1 = create_test_op(Hash32([1u8; 32]));
        let op2 = create_test_op(Hash32([2u8; 32]));
        let push_result = compute_ops_to_push(&[op1, op2], &local, &remote).unwrap();

        // B pulls from A: CIDs that A has but B doesn't (B's perspective)
        let pull_result = compute_cids_to_pull(&remote, &local);

        // Both should identify CID [1u8; 32] as the missing piece
        let push_cids: BTreeSet<Hash32> = push_result
            .iter()
            .map(|op| Hash32::from(op.op.parent_commitment))
            .collect();
        assert_eq!(push_cids, pull_result);
    }
}
