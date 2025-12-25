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
