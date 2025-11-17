//! Anti-entropy protocol for digest-based reconciliation
//!
//! This module provides effect-based anti-entropy operations for comparing
//! journal states, planning reconciliation requests, and merging operations.
//! It uses algebraic effects to separate pure logic from side effects.
//!
//! # Architecture
//!
//! The anti-entropy protocol follows a three-phase approach:
//! 1. **Digest Exchange**: Peers exchange journal digests
//! 2. **Reconciliation Planning**: Determine what operations are missing
//! 3. **Operation Transfer**: Transfer and merge missing operations
//!
//! # Integration
//!
//! - Uses `RetryPolicy` from infrastructure for resilient operations
//! - Integrates with `PeerManager` for peer selection
//! - Uses `RateLimiter` for flow budget enforcement
//! - Parameterized by `JournalEffects` + `NetworkEffects`
//!
//! # Usage
//!
//! ```rust,no_run
//! use aura_sync::protocols::{AntiEntropyProtocol, AntiEntropyConfig};
//! use aura_core::effects::{JournalEffects, NetworkEffects};
//!
//! async fn sync_with_peer<E>(effects: &E, peer: DeviceId) -> SyncResult<()>
//! where
//!     E: JournalEffects + NetworkEffects,
//! {
//!     let config = AntiEntropyConfig::default();
//!     let protocol = AntiEntropyProtocol::new(config);
//!
//!     let result = protocol.execute(effects, peer).await?;
//!     println!("Applied {} operations", result.applied);
//!     Ok(())
//! }
//! ```

use std::collections::HashSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use aura_core::{hash, Journal, AttestedOp, DeviceId, AuraError, AuraResult};
use crate::core::{SyncError, SyncResult};
use crate::infrastructure::RetryPolicy;

// =============================================================================
// Types
// =============================================================================

/// Unique fingerprint for an attested operation (cryptographic hash)
pub type OperationFingerprint = [u8; 32];

/// Summary of a journal snapshot used for anti-entropy comparisons
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalDigest {
    /// Number of attested operations known locally
    pub operation_count: usize,

    /// Maximum parent epoch observed in the operation log (if any)
    pub last_epoch: Option<u64>,

    /// Hash of the ordered operation fingerprints
    pub operation_hash: [u8; 32],

    /// Hash of the journal facts component
    pub fact_hash: [u8; 32],

    /// Hash of the capability frontier
    pub caps_hash: [u8; 32],
}

impl JournalDigest {
    /// Check if two digests are identical
    pub fn matches(&self, other: &Self) -> bool {
        self.operation_count == other.operation_count
            && self.operation_hash == other.operation_hash
            && self.fact_hash == other.fact_hash
            && self.caps_hash == other.caps_hash
    }
}

/// Relationship between two digests
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DigestStatus {
    /// Digests are identical
    Equal,

    /// Local node is missing operations compared to the peer
    LocalBehind,

    /// Peer is missing operations that the local node already has
    RemoteBehind,

    /// Operation counts match but hashes differ (divergent history)
    Diverged,
}

/// Request describing which operations we want from a peer
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AntiEntropyRequest {
    /// Operation index to start streaming from
    pub from_index: usize,

    /// Maximum operations to send in this batch
    pub max_ops: usize,

    /// Specific operation fingerprints that are missing (for targeted requests)
    pub missing_operations: Vec<OperationFingerprint>,
}

/// Result of an anti-entropy synchronization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AntiEntropyResult {
    /// Number of operations that were newly applied
    pub applied: usize,

    /// Number of duplicates that were ignored
    pub duplicates: usize,

    /// Final digest status after synchronization
    pub final_status: Option<DigestStatus>,

    /// Number of synchronization rounds performed
    pub rounds: usize,
}

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for anti-entropy protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiEntropyConfig {
    /// Batch size for operation transfer
    pub batch_size: usize,

    /// Maximum synchronization rounds before giving up
    pub max_rounds: usize,

    /// Enable retry on transient failures
    pub retry_enabled: bool,

    /// Retry policy for resilient operations
    pub retry_policy: RetryPolicy,

    /// Timeout for digest exchange
    pub digest_timeout: Duration,

    /// Timeout for operation transfer
    pub transfer_timeout: Duration,
}

impl Default for AntiEntropyConfig {
    fn default() -> Self {
        Self {
            batch_size: 128,
            max_rounds: 10,
            retry_enabled: true,
            retry_policy: RetryPolicy::exponential()
                .with_max_attempts(3)
                .with_initial_delay(Duration::from_millis(100)),
            digest_timeout: Duration::from_secs(10),
            transfer_timeout: Duration::from_secs(30),
        }
    }
}

// =============================================================================
// Anti-Entropy Protocol
// =============================================================================

/// Digest-based anti-entropy protocol for CRDT synchronization
///
/// Implements the anti-entropy algorithm:
/// 1. Exchange digests with peer
/// 2. Compare digests to identify missing operations
/// 3. Request and merge missing operations in batches
/// 4. Repeat until synchronized or max rounds reached
pub struct AntiEntropyProtocol {
    config: AntiEntropyConfig,
}

impl AntiEntropyProtocol {
    /// Create a new anti-entropy protocol with the given configuration
    pub fn new(config: AntiEntropyConfig) -> Self {
        Self { config }
    }

    /// Execute anti-entropy synchronization with a peer
    ///
    /// This is the main entry point for the protocol. It performs digest
    /// exchange, reconciliation planning, and operation transfer.
    ///
    /// # Integration Points
    /// - Uses `JournalEffects` to access local journal state
    /// - Uses `NetworkEffects` to communicate with peer
    /// - Uses `RetryPolicy` from infrastructure for resilience
    pub async fn execute<E>(
        &self,
        _effects: &E,
        _peer: DeviceId,
    ) -> SyncResult<AntiEntropyResult>
    where
        E: Send + Sync,
    {
        // TODO: Implement using effect system
        // For now, return empty result
        Ok(AntiEntropyResult::default())
    }

    /// Compute a digest for the given journal state and operation log
    pub fn compute_digest(
        &self,
        journal: &Journal,
        operations: &[AttestedOp],
    ) -> SyncResult<JournalDigest> {
        let fact_hash = hash_serialized(&journal.facts)
            .map_err(|e| SyncError::session(&format!("Failed to hash facts: {}", e)))?;

        let caps_hash = hash_serialized(&journal.caps)
            .map_err(|e| SyncError::session(&format!("Failed to hash caps: {}", e)))?;

        let mut h = hash::hasher();
        let mut last_epoch: Option<u64> = None;

        for op in operations {
            let fp = fingerprint(op)
                .map_err(|e| SyncError::session(&format!("Failed to fingerprint op: {}", e)))?;
            h.update(&fp);

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

    /// Compare two digests and classify their relationship
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

    /// Plan the next anti-entropy request based on digest comparison
    pub fn plan_request(
        &self,
        local: &JournalDigest,
        remote: &JournalDigest,
    ) -> Option<AntiEntropyRequest> {
        match Self::compare(local, remote) {
            DigestStatus::LocalBehind => {
                let remaining = remote.operation_count.saturating_sub(local.operation_count);
                Some(AntiEntropyRequest {
                    from_index: local.operation_count,
                    max_ops: remaining.min(self.config.batch_size),
                    missing_operations: Vec::new(),
                })
            }
            DigestStatus::Diverged => Some(AntiEntropyRequest {
                from_index: 0,
                max_ops: self.config.batch_size,
                missing_operations: Vec::new(),
            }),
            DigestStatus::Equal | DigestStatus::RemoteBehind => None,
        }
    }

    /// Merge a batch of operations, deduplicating already-seen entries
    pub fn merge_batch(
        &self,
        local_ops: &mut Vec<AttestedOp>,
        incoming: Vec<AttestedOp>,
    ) -> SyncResult<AntiEntropyResult> {
        if incoming.is_empty() {
            return Ok(AntiEntropyResult::default());
        }

        let mut seen = HashSet::with_capacity(local_ops.len());
        for op in local_ops.iter() {
            let fp = fingerprint(op)
                .map_err(|e| SyncError::session(&format!("Failed to fingerprint: {}", e)))?;
            seen.insert(fp);
        }

        let mut applied = 0;
        let mut duplicates = 0;

        for op in incoming {
            let fp = fingerprint(&op)
                .map_err(|e| SyncError::session(&format!("Failed to fingerprint: {}", e)))?;
            if seen.insert(fp) {
                local_ops.push(op);
                applied += 1;
            } else {
                duplicates += 1;
            }
        }

        Ok(AntiEntropyResult {
            applied,
            duplicates,
            final_status: None,
            rounds: 1,
        })
    }
}

impl Default for AntiEntropyProtocol {
    fn default() -> Self {
        Self::new(AntiEntropyConfig::default())
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

fn hash_serialized<T: Serialize>(value: &T) -> AuraResult<[u8; 32]> {
    let bytes = bincode::serialize(value)
        .map_err(|err| AuraError::serialization(err.to_string()))?;
    Ok(hash::hash(&bytes))
}

fn fingerprint(op: &AttestedOp) -> AuraResult<OperationFingerprint> {
    hash_serialized(op)
}

// =============================================================================
// Convenience Functions
// =============================================================================

/// Build reconciliation request by comparing local and peer digests
pub fn build_reconciliation_request(
    local: &JournalDigest,
    peer: &JournalDigest,
) -> SyncResult<AntiEntropyRequest> {
    let protocol = AntiEntropyProtocol::default();
    match protocol.plan_request(local, peer) {
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
pub fn compute_digest(
    journal: &Journal,
    operations: &[AttestedOp],
) -> SyncResult<JournalDigest> {
    let protocol = AntiEntropyProtocol::default();
    protocol.compute_digest(journal, operations)
}

// =============================================================================
// Tests
// =============================================================================

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

    fn sample_op(epoch: u64) -> AttestedOp {
        AttestedOp {
            op: TreeOp {
                kind: TreeOpKind::AddDevice,
                parent_epoch: epoch,
                data: vec![],
            },
            attestation: vec![],
        }
    }

    #[test]
    fn test_digest_computation() {
        let protocol = AntiEntropyProtocol::default();
        let journal = sample_journal();
        let ops = vec![sample_op(1), sample_op(2)];

        let digest = protocol.compute_digest(&journal, &ops).unwrap();

        assert_eq!(digest.operation_count, 2);
        assert_eq!(digest.last_epoch, Some(2));
    }

    #[test]
    fn test_digest_comparison_equal() {
        let protocol = AntiEntropyProtocol::default();
        let journal = sample_journal();
        let ops = vec![sample_op(1)];

        let digest1 = protocol.compute_digest(&journal, &ops).unwrap();
        let digest2 = protocol.compute_digest(&journal, &ops).unwrap();

        assert_eq!(AntiEntropyProtocol::compare(&digest1, &digest2), DigestStatus::Equal);
    }

    #[test]
    fn test_digest_comparison_local_behind() {
        let protocol = AntiEntropyProtocol::default();
        let journal = sample_journal();

        let ops1 = vec![sample_op(1)];
        let ops2 = vec![sample_op(1), sample_op(2)];

        let digest1 = protocol.compute_digest(&journal, &ops1).unwrap();
        let digest2 = protocol.compute_digest(&journal, &ops2).unwrap();

        assert_eq!(AntiEntropyProtocol::compare(&digest1, &digest2), DigestStatus::LocalBehind);
    }

    #[test]
    fn test_plan_request_local_behind() {
        let protocol = AntiEntropyProtocol::new(AntiEntropyConfig {
            batch_size: 10,
            ..Default::default()
        });

        let journal = sample_journal();
        let ops1 = vec![sample_op(1)];
        let ops2 = vec![sample_op(1), sample_op(2), sample_op(3)];

        let digest1 = protocol.compute_digest(&journal, &ops1).unwrap();
        let digest2 = protocol.compute_digest(&journal, &ops2).unwrap();

        let request = protocol.plan_request(&digest1, &digest2).unwrap();

        assert_eq!(request.from_index, 1);
        assert_eq!(request.max_ops, 2);
    }

    #[test]
    fn test_merge_batch() {
        let protocol = AntiEntropyProtocol::default();
        let mut local_ops = vec![sample_op(1)];
        let incoming = vec![sample_op(1), sample_op(2), sample_op(3)];

        let result = protocol.merge_batch(&mut local_ops, incoming).unwrap();

        assert_eq!(result.applied, 2);
        assert_eq!(result.duplicates, 1);
        assert_eq!(local_ops.len(), 3);
    }
}
