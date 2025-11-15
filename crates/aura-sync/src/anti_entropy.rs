//! Anti-entropy protocols for digest-based reconciliation.
//!
//! This module provides effect-based anti-entropy operations for comparing
//! journal states, planning reconciliation requests, and merging operations.
//! It uses the algebraic effects pattern to separate pure logic from side effects.

use std::collections::HashSet;

use aura_core::hash;
use aura_core::Journal;
use aura_core::{AttestedOp, AuraError, AuraResult};
use serde::{Deserialize, Serialize};

/// Unique fingerprint for an attested operation (cryptographic hash).
pub type OperationFingerprint = [u8; 32];

/// Summary of a journal snapshot used for anti-entropy comparisons.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalDigest {
    /// Number of attested operations known locally.
    pub operation_count: usize,
    /// Maximum parent epoch observed in the operation log (if any).
    pub last_epoch: Option<u64>,
    /// Hash of the ordered operation fingerprints.
    pub operation_hash: [u8; 32],
    /// Hash of the journal facts component.
    pub fact_hash: [u8; 32],
    /// Hash of the capability frontier.
    pub caps_hash: [u8; 32],
}

impl JournalDigest {
    /// Convenience helper for equality checks.
    pub fn matches(&self, other: &Self) -> bool {
        self.operation_count == other.operation_count
            && self.operation_hash == other.operation_hash
            && self.fact_hash == other.fact_hash
            && self.caps_hash == other.caps_hash
    }
}

/// Relationship between two digests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigestStatus {
    /// Digests are identical.
    Equal,
    /// Local node is missing operations compared to the peer.
    LocalBehind,
    /// Peer is missing operations that the local node already has.
    RemoteBehind,
    /// Operation counts match but hashes differ (divergent history).
    Diverged,
}

/// Request describing which operations we want from a peer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AntiEntropyRequest {
    /// Operation index to start streaming from.
    pub from_index: usize,
    /// Maximum operations to send in this batch.
    pub max_ops: usize,
    /// Specific operation fingerprints that are missing (for targeted requests)
    pub missing_operations: Vec<OperationFingerprint>,
}

/// Result of merging a batch of operations into the local log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AntiEntropyReport {
    /// Number of operations that were newly applied.
    pub applied: usize,
    /// Number of duplicates that were ignored.
    pub duplicates: usize,
}

/// Digest-based anti-entropy engine.
///
/// The choreography keeps no mutable state; it only records the desired
/// batch size for planning requests. Callers retain ownership of journals
/// and operation logs.
#[derive(Debug, Clone)]
pub struct AntiEntropyChoreography {
    batch_size: usize,
}

impl AntiEntropyChoreography {
    /// Create a new anti-entropy planner with the desired batch size.
    pub fn new(batch_size: usize) -> Self {
        Self {
            batch_size: batch_size.max(1),
        }
    }

    /// Compute a digest for the given journal state and operation log.
    pub fn compute_digest(
        &self,
        journal: &Journal,
        operations: &[AttestedOp],
    ) -> AuraResult<JournalDigest> {
        let fact_hash = hash_serialized(&journal.facts)?;
        let caps_hash = hash_serialized(&journal.caps)?;

        let mut h = hash::hasher();
        let mut last_epoch: Option<u64> = None;

        for op in operations {
            h.update(&fingerprint(op)?);
            let epoch = op.op.parent_epoch;
            last_epoch = Some(match last_epoch {
                Some(existing) => existing.max(epoch),
                None => epoch,
            });
        }

        let operation_hash = h.finalize();

        Ok(JournalDigest {
            operation_count: operations.len(),
            last_epoch,
            operation_hash,
            fact_hash,
            caps_hash,
        })
    }

    /// Compare two digests and classify their relationship.
    pub fn compare(local: &JournalDigest, remote: &JournalDigest) -> DigestStatus {
        if local.matches(remote) {
            return DigestStatus::Equal;
        }

        match local.operation_count.cmp(&remote.operation_count) {
            std::cmp::Ordering::Less => DigestStatus::LocalBehind,
            std::cmp::Ordering::Greater => DigestStatus::RemoteBehind,
            std::cmp::Ordering::Equal => DigestStatus::Diverged,
        }
    }

    /// Plan the next anti-entropy request based on digest comparison.
    pub fn next_request(
        &self,
        local: &JournalDigest,
        remote: &JournalDigest,
    ) -> Option<AntiEntropyRequest> {
        match Self::compare(local, remote) {
            DigestStatus::LocalBehind => {
                let remaining = remote.operation_count.saturating_sub(local.operation_count);
                Some(AntiEntropyRequest {
                    from_index: local.operation_count,
                    max_ops: remaining.min(self.batch_size),
                    missing_operations: Vec::new(), // TODO: Add specific fingerprints
                })
            }
            DigestStatus::Diverged => Some(AntiEntropyRequest {
                from_index: 0,
                max_ops: self.batch_size,
                missing_operations: Vec::new(), // TODO: Add specific fingerprints
            }),
            _ => None,
        }
    }

    /// Merge a batch of operations, deduplicating already-seen entries.
    pub fn merge_batch(
        &self,
        local_ops: &mut Vec<AttestedOp>,
        incoming: Vec<AttestedOp>,
    ) -> AuraResult<AntiEntropyReport> {
        if incoming.is_empty() {
            return Ok(AntiEntropyReport::default());
        }

        let mut seen = HashSet::with_capacity(local_ops.len());
        for op in local_ops.iter() {
            seen.insert(fingerprint(op)?);
        }

        let mut applied = 0;
        let mut duplicates = 0;

        for op in incoming {
            let fp = fingerprint(&op)?;
            if seen.insert(fp) {
                local_ops.push(op);
                applied += 1;
            } else {
                duplicates += 1;
            }
        }

        Ok(AntiEntropyReport {
            applied,
            duplicates,
        })
    }
}

impl Default for AntiEntropyChoreography {
    fn default() -> Self {
        Self::new(128)
    }
}

fn hash_serialized<T: Serialize>(value: &T) -> AuraResult<[u8; 32]> {
    let bytes =
        bincode::serialize(value).map_err(|err| AuraError::serialization(err.to_string()))?;
    Ok(hash::hash(&bytes))
}

fn fingerprint(op: &AttestedOp) -> AuraResult<OperationFingerprint> {
    hash_serialized(op)
}

/// Build reconciliation request by comparing local and peer digests
pub fn build_reconciliation_request(
    local: &JournalDigest,
    peer: &JournalDigest,
) -> AuraResult<AntiEntropyRequest> {
    let choreography = AntiEntropyChoreography::new(128); // Default batch size
    match choreography.next_request(local, peer) {
        Some(request) => Ok(request),
        None => {
            // No sync needed - create empty request
            Ok(AntiEntropyRequest {
                from_index: 0,
                max_ops: 0,
                missing_operations: Vec::new(),
            })
        }
    }
}

/// Compute digest from journal state and operations
pub fn compute_digest(journal: &Journal, operations: &[AttestedOp]) -> AuraResult<JournalDigest> {
    let choreography = AntiEntropyChoreography::new(128);
    choreography.compute_digest(journal, operations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{journal::FactValue, TreeOp, TreeOpKind};

    fn sample_journal() -> Journal {
        let mut journal = Journal::new();
        journal.facts.insert("counter", FactValue::Number(1));
        journal.caps.add_permission("sync");
        journal
    }

    fn make_op(epoch: u64, salt: u8) -> AttestedOp {
        let tree_op = TreeOp {
            parent_epoch: epoch,
            parent_commitment: [salt; 32],
            op: TreeOpKind::RotateEpoch {
                affected: Vec::new(),
            },
            version: 1,
        };

        AttestedOp {
            op: tree_op,
            agg_sig: vec![salt],
            signer_count: 3,
        }
    }

    #[test]
    fn digest_equality_detection() {
        let engine = AntiEntropyChoreography::default();
        let journal = sample_journal();
        let ops = vec![make_op(1, 1), make_op(2, 2)];

        let digest_a = engine.compute_digest(&journal, &ops).unwrap();
        let digest_b = engine.compute_digest(&journal, &ops).unwrap();

        assert_eq!(
            DigestStatus::Equal,
            AntiEntropyChoreography::compare(&digest_a, &digest_b)
        );
    }

    #[test]
    fn next_request_when_local_is_behind() {
        let engine = AntiEntropyChoreography::new(10);

        let mut digest_local = JournalDigest {
            operation_count: 5,
            last_epoch: Some(5),
            operation_hash: [1; 32],
            fact_hash: [2; 32],
            caps_hash: [3; 32],
        };

        let digest_remote = JournalDigest {
            operation_count: 12,
            last_epoch: Some(6),
            operation_hash: [4; 32],
            fact_hash: [2; 32],
            caps_hash: [3; 32],
        };

        // Ensure digests differ even though counts differ already.
        digest_local.operation_hash = [5; 32];

        let request = engine.next_request(&digest_local, &digest_remote).unwrap();
        assert_eq!(5, request.from_index);
        assert_eq!(7, request.max_ops);
    }

    #[test]
    fn merge_batch_deduplicates_operations() {
        let engine = AntiEntropyChoreography::default();
        let mut local_ops = vec![make_op(1, 1)];
        let incoming = vec![make_op(1, 1), make_op(2, 2)];

        let report = engine.merge_batch(&mut local_ops, incoming).unwrap();
        assert_eq!(1, report.applied);
        assert_eq!(1, report.duplicates);
        assert_eq!(2, local_ops.len());
    }
}
