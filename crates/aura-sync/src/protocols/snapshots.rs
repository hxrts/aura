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
//! ```rust,no_run
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

use std::sync::Arc;
use parking_lot::{Mutex, RwLock};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use aura_core::{DeviceId, Hash32, AuraError, AuraResult};
use aura_core::tree::Epoch as TreeEpoch;
use crate::core::{SyncError, SyncResult};

// =============================================================================
// Types
// =============================================================================

/// Snapshot proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotProposal {
    /// Device proposing the snapshot
    pub proposer: DeviceId,

    /// Unique proposal identifier
    pub proposal_id: Uuid,

    /// Target epoch for snapshot
    pub target_epoch: TreeEpoch,

    /// State digest at target epoch
    pub state_digest: Hash32,
}

impl SnapshotProposal {
    /// Create a new snapshot proposal
    ///
    /// Note: Callers should generate UUIDs via `RandomEffects::random_uuid()` and use `with_id()`
    pub fn new(proposer: DeviceId, target_epoch: TreeEpoch, state_digest: Hash32, proposal_uuid: Uuid) -> Self {
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
    /// Device approving the snapshot
    pub approver: DeviceId,

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
    pub approval_threshold: usize,

    /// Total N devices in quorum
    pub quorum_size: usize,

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
                "{} blocked by snapshot writer fence",
                context
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
        proposer: DeviceId,
        target_epoch: TreeEpoch,
        state_digest: Hash32,
    ) -> SyncResult<(Option<WriterFenceGuard>, SnapshotProposal)> {
        let mut pending = self.pending.lock();
        if pending.is_some() {
            return Err(SyncError::protocol("sync", 
                "snapshot proposal already in progress".to_string()
            ));
        }

        let proposal = SnapshotProposal::new(proposer, target_epoch, state_digest);

        let guard = if self.config.use_writer_fence {
            Some(self.fence.acquire()
                .map_err(|e| SyncError::protocol("sync", e.to_string()))?)
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
    /// Note: Callers should obtain `completion_id` via `RandomEffects` or use `Uuid::new_v4()` in tests
    pub fn commit(
        &self,
        proposal: SnapshotProposal,
        approvals: Vec<SnapshotApproval>,
        completion_id: Uuid,
    ) -> SyncResult<SnapshotResult> {
        let mut pending = self.pending.lock();

        // Verify this is the pending proposal
        match pending.as_ref() {
            Some(p) if p.proposal_id == proposal.proposal_id => {},
            _ => return Err(SyncError::protocol("sync", 
                "proposal does not match pending snapshot".to_string()
            )),
        }

        // Verify threshold
        if approvals.len() < self.config.approval_threshold {
            return Err(SyncError::protocol("sync", format!(
                "insufficient approvals: {} < {}",
                approvals.len(),
                self.config.approval_threshold
            )));
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
            return Err(SyncError::protocol("sync", 
                "no pending snapshot to abort".to_string()
            ));
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
        let device = DeviceId::from_bytes([1; 32]);

        assert!(!protocol.is_pending());

        let (_guard, proposal) = protocol.propose(
            device,
            10,
            Hash32([0; 32]),
        ).unwrap();

        assert!(protocol.is_pending());
        assert_eq!(protocol.get_pending().unwrap().proposal_id, proposal.proposal_id);

        // Second proposal should fail
        assert!(protocol.propose(device, 11, Hash32([0; 32])).is_err());
    }

    #[test]
    fn test_snapshot_commit() {
        let config = SnapshotConfig {
            approval_threshold: 2,
            quorum_size: 3,
            use_writer_fence: false,
        };
        let protocol = SnapshotProtocol::new(config);
        let device = DeviceId::from_bytes([1; 32]);

        let (_guard, proposal) = protocol.propose(
            device,
            10,
            Hash32([0; 32]),
        ).unwrap();

        let approvals = vec![
            SnapshotApproval {
                approver: DeviceId::from_bytes([2; 32]),
                proposal_id: proposal.proposal_id,
                signature: vec![],
            },
            SnapshotApproval {
                approver: DeviceId::from_bytes([3; 32]),
                proposal_id: proposal.proposal_id,
                signature: vec![],
            },
        ];

        let result = protocol.commit(proposal, approvals, Uuid::new_v4()).unwrap();
        assert!(result.committed);
        assert!(!protocol.is_pending());
    }
}
