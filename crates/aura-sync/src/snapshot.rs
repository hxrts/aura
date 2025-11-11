//! Snapshot proposal helper with writer-fence tracking.

use std::collections::BTreeSet;
use std::sync::Arc;

use aura_core::{
    maintenance::{MaintenanceEvent, SnapshotCompleted, SnapshotProposed},
    tree::{Epoch as TreeEpoch, NodeIndex, Snapshot},
    AuraError, AuraResult, DeviceId, Hash32,
};
use parking_lot::{Mutex, RwLock};
use uuid::Uuid;

/// Tracks whether writers are fenced while a snapshot proposal is active.
#[derive(Debug, Default, Clone)]
pub struct WriterFence {
    inner: Arc<RwLock<bool>>,
}

impl WriterFence {
    /// Create a new fence.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(false)),
        }
    }

    /// Attempt to enter the fence (block writes). Returns a guard that releases on drop.
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

    /// Ensure writers are currently allowed.
    pub fn ensure_open(&self, context: &str) -> AuraResult<()> {
        if *self.inner.read() {
            return Err(AuraError::coordination_failed(format!(
                "{} blocked by snapshot writer fence",
                context
            )));
        }
        Ok(())
    }
}

/// RAII guard that releases the fence when dropped.
#[derive(Debug)]
pub struct WriterFenceGuard {
    inner: Arc<RwLock<bool>>,
}

impl Drop for WriterFenceGuard {
    fn drop(&mut self) {
        *self.inner.write() = false;
    }
}

/// Snapshot manager that emits journal-ready maintenance events.
#[derive(Debug, Default)]
pub struct SnapshotManager {
    fence: WriterFence,
    pending: Mutex<Option<SnapshotProposed>>, // only one proposal at a time for launch
}

impl SnapshotManager {
    /// Create a new manager.
    pub fn new() -> Self {
        Self {
            fence: WriterFence::new(),
            pending: Mutex::new(None),
        }
    }

    /// Access the underlying writer fence.
    pub fn fence(&self) -> WriterFence {
        self.fence.clone()
    }

    /// Start a new snapshot proposal, returning the maintenance event to append to the journal.
    pub fn propose(
        &self,
        proposer: DeviceId,
        target_epoch: TreeEpoch,
        state_digest: [u8; 32],
    ) -> AuraResult<(WriterFenceGuard, MaintenanceEvent)> {
        let mut pending = self.pending.lock();
        if pending.is_some() {
            return Err(AuraError::coordination_failed(
                "snapshot proposal already in progress",
            ));
        }
        let guard = self.fence.acquire()?;
        let proposal = SnapshotProposed::new(proposer, target_epoch, Hash32(state_digest));
        let event = MaintenanceEvent::SnapshotProposed(proposal.clone());
        *pending = Some(proposal);
        Ok((guard, event))
    }

    /// Emit a completion event if the provided snapshot matches the pending proposal.
    pub fn complete(
        &self,
        snapshot: Snapshot,
        participants: BTreeSet<DeviceId>,
        threshold_signature: Vec<u8>,
    ) -> AuraResult<(MaintenanceEvent, Uuid)> {
        let mut pending = self.pending.lock();
        let proposal = pending
            .take()
            .ok_or_else(|| AuraError::coordination_failed("no active snapshot proposal"))?;
        drop(pending); // release lock before building event

        let completion = SnapshotCompleted::new(
            proposal.proposal_id,
            snapshot,
            participants,
            threshold_signature,
        );
        Ok((
            MaintenanceEvent::SnapshotCompleted(completion),
            proposal.proposal_id,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::tree::{Epoch as TreeEpoch, LeafId, NodeIndex, Policy};
    use std::collections::BTreeMap;

    fn dummy_snapshot(epoch: TreeEpoch) -> Snapshot {
        let mut policies = BTreeMap::new();
        policies.insert(NodeIndex(0), Policy::Any);
        Snapshot::new(epoch, [1u8; 32], vec![LeafId(1)], policies, 0)
    }

    #[test]
    fn writer_fence_blocks_parallel_proposals() {
        let manager = SnapshotManager::new();
        let (_guard, _) = manager.propose(DeviceId::new(), 5_u64, [0u8; 32]).unwrap();
        let err = manager
            .propose(DeviceId::new(), 6_u64, [0u8; 32])
            .unwrap_err();
        assert!(format!("{}", err).contains("proposal already in progress"));
    }

    #[test]
    fn snapshot_completion_clears_pending() {
        let manager = SnapshotManager::new();
        let (_guard, _) = manager.propose(DeviceId::new(), 5_u64, [0u8; 32]).unwrap();
        let participants = BTreeSet::from([DeviceId::new()]);
        let (event, proposal_id) = manager
            .complete(dummy_snapshot(5_u64), participants.clone(), vec![1, 2, 3])
            .unwrap();
        match event {
            MaintenanceEvent::SnapshotCompleted(payload) => {
                assert_eq!(payload.participants, participants);
                assert_eq!(payload.proposal_id, proposal_id);
            }
            _ => panic!("expected SnapshotCompleted"),
        }

        // new proposal allowed now
        assert!(manager.propose(DeviceId::new(), 7_u64, [0u8; 32]).is_ok());
    }
}
