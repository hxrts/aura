//! Snapshot coordination protocol
//!
//! Provides coordinated garbage collection with threshold approval.
//! Implements writer fencing to ensure consistent snapshot capture.
//!
//! # Architecture
//!
//! The snapshot protocol coordinates:
//! 1. Proposal of snapshot at target epoch
//! 2. Writer fence to block concurrent writes
//! 3. State digest verification
//! 4. Threshold approval from M-of-N devices
//! 5. Snapshot commit and cleanup
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_sync::protocols::{SnapshotProtocol, SnapshotConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = SnapshotConfig::default();
//! let protocol = SnapshotProtocol::new(config);
//!
//! // Propose snapshot
//! let proposal = protocol.propose(target_epoch, state_digest)?;
//!
//! // Collect approvals (threshold ceremony)
//! // ...
//!
//! // Commit snapshot
//! protocol.commit(proposal)?;
//! # Ok(())
//! # }
//! ```

use parking_lot::{Mutex, RwLock};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::{sync_protocol_error, SyncResult};
use aura_core::types::Epoch;
use aura_core::{AuraError, AuraResult, AuthorityId, Hash32};

// =============================================================================
// Types
// =============================================================================

/// Snapshot proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotProposal {
    /// Authority proposing the snapshot
    pub proposer: AuthorityId,

    /// Unique proposal identifier
    pub proposal_id: Uuid,

    /// Target epoch for snapshot
    pub target_epoch: Epoch,

    /// State digest at target epoch
    pub state_digest: Hash32,
}

impl SnapshotProposal {
    /// Create a new snapshot proposal
    ///
    /// Note: Callers should generate UUIDs via `RandomEffects::random_uuid()` and use `with_id()`
    pub fn new(
        proposer: AuthorityId,
        target_epoch: Epoch,
        state_digest: Hash32,
        proposal_uuid: Uuid,
    ) -> Self {
        Self {
            proposer,
            proposal_id: proposal_uuid,
            target_epoch,
            state_digest,
        }
    }
}

/// Snapshot approval from a device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotApproval {
    /// Authority approving the snapshot
    pub approver: AuthorityId,

    /// Proposal being approved
    pub proposal_id: Uuid,

    /// Signature over proposal (threshold signature component)
    pub signature: Vec<u8>,
}

/// Snapshot result after commit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotResult {
    /// Completed proposal
    pub proposal: SnapshotProposal,

    /// Approvals collected
    pub approvals: Vec<SnapshotApproval>,

    /// Whether snapshot was successfully committed
    pub committed: bool,

    /// Completion timestamp
    pub completion_id: Uuid,
}

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for snapshot protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotConfig {
    /// Threshold M for M-of-N approval
    pub approval_threshold: u32,

    /// Total N devices in quorum
    pub quorum_size: u32,

    /// Enable writer fence during snapshot
    pub use_writer_fence: bool,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            approval_threshold: 2,
            quorum_size: 3,
            use_writer_fence: true,
        }
    }
}

// =============================================================================
// Writer Fence
// =============================================================================

/// Tracks whether writers are fenced during snapshot
#[derive(Debug, Default, Clone)]
pub struct WriterFence {
    inner: Arc<RwLock<bool>>,
}

impl WriterFence {
    /// Create a new fence
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(false)),
        }
    }

    /// Acquire the fence (block writes)
    pub fn acquire(&self) -> AuraResult<WriterFenceGuard> {
        let mut guard = self.inner.write();
        if *guard {
            return Err(AuraError::coordination_failed(
                "writer fence already active for snapshot proposal",
            ));
        }
        *guard = true;
        Ok(WriterFenceGuard {
            inner: self.inner.clone(),
        })
    }

    /// Ensure writers are currently allowed
    pub fn ensure_open(&self, context: &str) -> AuraResult<()> {
        if *self.inner.read() {
            return Err(AuraError::coordination_failed(format!(
                "{context} blocked by snapshot writer fence"
            )));
        }
        Ok(())
    }

    /// Check if fence is active
    pub fn is_active(&self) -> bool {
        *self.inner.read()
    }
}

/// RAII guard that releases the fence when dropped
#[derive(Debug)]
pub struct WriterFenceGuard {
    inner: Arc<RwLock<bool>>,
}

impl Drop for WriterFenceGuard {
    fn drop(&mut self) {
        *self.inner.write() = false;
    }
}

// =============================================================================
// Snapshot Protocol
// =============================================================================

/// Snapshot coordination protocol
pub struct SnapshotProtocol {
    config: SnapshotConfig,
    fence: WriterFence,
    pending: Mutex<Option<SnapshotProposal>>,
}

impl SnapshotProtocol {
    /// Create a new snapshot protocol
    pub fn new(config: SnapshotConfig) -> Self {
        Self {
            config,
            fence: WriterFence::new(),
            pending: Mutex::new(None),
        }
    }

    /// Access the writer fence
    pub fn fence(&self) -> WriterFence {
        self.fence.clone()
    }

    /// Propose a new snapshot
    pub fn propose(
        &self,
        proposer: AuthorityId,
        target_epoch: Epoch,
        state_digest: Hash32,
        proposal_id: Uuid,
    ) -> SyncResult<(Option<WriterFenceGuard>, SnapshotProposal)> {
        let mut pending = self.pending.lock();
        if pending.is_some() {
            return Err(sync_protocol_error(
                "sync",
                "snapshot proposal already in progress",
            ));
        }

        let proposal = SnapshotProposal::new(proposer, target_epoch, state_digest, proposal_id);

        let guard = if self.config.use_writer_fence {
            Some(
                self.fence
                    .acquire()
                    .map_err(|e| sync_protocol_error("sync", e.to_string()))?,
            )
        } else {
            None
        };

        *pending = Some(proposal.clone());

        Ok((guard, proposal))
    }

    /// Check if a proposal is pending
    pub fn is_pending(&self) -> bool {
        self.pending.lock().is_some()
    }

    /// Get the pending proposal
    pub fn get_pending(&self) -> Option<SnapshotProposal> {
        self.pending.lock().clone()
    }

    /// Commit a snapshot after collecting approvals
    ///
    /// Note: Callers should obtain `completion_id` via `RandomEffects` or use `Uuid::from_bytes(10u128.to_be_bytes())` in tests
    pub fn commit(
        &self,
        proposal: SnapshotProposal,
        approvals: Vec<SnapshotApproval>,
        completion_id: Uuid,
    ) -> SyncResult<SnapshotResult> {
        let mut pending = self.pending.lock();

        // Verify this is the pending proposal
        match pending.as_ref() {
            Some(p) if p.proposal_id == proposal.proposal_id => {}
            _ => {
                return Err(sync_protocol_error(
                    "sync",
                    "proposal does not match pending snapshot",
                ))
            }
        }

        // Verify threshold
        let approvals_len = approvals.len() as u32;
        if approvals_len < self.config.approval_threshold {
            return Err(sync_protocol_error(
                "sync",
                format!(
                    "insufficient approvals: {} < {}",
                    approvals_len, self.config.approval_threshold
                ),
            ));
        }

        // Clear pending
        *pending = None;

        Ok(SnapshotResult {
            proposal,
            approvals,
            committed: true,
            completion_id,
        })
    }

    /// Abort a pending snapshot
    pub fn abort(&self) -> SyncResult<()> {
        let mut pending = self.pending.lock();
        if pending.is_none() {
            return Err(sync_protocol_error("sync", "no pending snapshot to abort"));
        }
        *pending = None;
        Ok(())
    }
}

impl Default for SnapshotProtocol {
    fn default() -> Self {
        Self::new(SnapshotConfig::default())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // Test code uses Uuid::from_bytes(11u128.to_be_bytes()) for test data generation
mod tests {
    use super::*;

    #[test]
    fn test_writer_fence() {
        let fence = WriterFence::new();
        assert!(!fence.is_active());

        let _guard = fence.acquire().unwrap();
        assert!(fence.is_active());

        // Second acquire should fail
        assert!(fence.acquire().is_err());

        drop(_guard);
        assert!(!fence.is_active());
    }

    #[test]
    fn test_snapshot_proposal() {
        let protocol = SnapshotProtocol::default();
        let authority = AuthorityId::new_from_entropy([1; 32]);

        assert!(!protocol.is_pending());

        let (_guard, proposal) = protocol
            .propose(
                authority,
                Epoch::new(10),
                Hash32([0; 32]),
                Uuid::from_bytes(12u128.to_be_bytes()),
            )
            .unwrap();

        assert!(protocol.is_pending());
        assert_eq!(
            protocol.get_pending().unwrap().proposal_id,
            proposal.proposal_id
        );

        // Second proposal should fail
        assert!(protocol
            .propose(
                authority,
                Epoch::new(11),
                Hash32([0; 32]),
                uuid::Uuid::from_bytes(13u128.to_be_bytes())
            )
            .is_err());
    }

    #[test]
    fn test_snapshot_commit() {
        let config = SnapshotConfig {
            approval_threshold: 2,
            quorum_size: 3,
            use_writer_fence: false,
        };
        let protocol = SnapshotProtocol::new(config);
        let authority = AuthorityId::new_from_entropy([1; 32]);

        let (_guard, proposal) = protocol
            .propose(
                authority,
                Epoch::new(10),
                Hash32([0; 32]),
                Uuid::from_bytes(30u128.to_be_bytes()),
            )
            .unwrap();

        let approvals = vec![
            SnapshotApproval {
                approver: AuthorityId::new_from_entropy([2; 32]),
                proposal_id: proposal.proposal_id,
                signature: vec![],
            },
            SnapshotApproval {
                approver: AuthorityId::new_from_entropy([3; 32]),
                proposal_id: proposal.proposal_id,
                signature: vec![],
            },
        ];

        let result = protocol
            .commit(proposal, approvals, Uuid::from_bytes(31u128.to_be_bytes()))
            .unwrap();
        assert!(result.committed);
        assert!(!protocol.is_pending());
    }
}
