#![allow(clippy::disallowed_types)]

//! Stateful VM bridge effects for deterministic testing.

use aura_core::effects::{
    VmBridgeBlockedEdge, VmBridgeEffects, VmBridgeLeaseMetadataSnapshot, VmBridgePendingSend,
    VmBridgeSchedulerSignals, VmBridgeSendError, VmBridgeSendLease,
    VmBridgeTransferMetadataSnapshot,
};
use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Debug, Default)]
struct PendingSendQueue {
    frames: VecDeque<VmBridgePendingSend>,
    owned: bool,
    unknown: bool,
}

struct PendingSendLease<'a> {
    queue: &'a Mutex<PendingSendQueue>,
    front: Option<VmBridgePendingSend>,
    in_flight: bool,
}

impl VmBridgeSendLease for PendingSendLease<'_> {
    fn pending(&self) -> Option<&VmBridgePendingSend> {
        self.front.as_ref()
    }
    fn begin_delivery(&mut self) -> Result<(), VmBridgeSendError> {
        if self.in_flight || self.front.is_none() {
            return Err(VmBridgeSendError::InvalidTransition);
        }
        self.in_flight = true;
        Ok(())
    }
    fn acknowledge(&mut self) -> Result<(), VmBridgeSendError> {
        if !self.in_flight {
            return Err(VmBridgeSendError::InvalidTransition);
        }
        let mut queue = lock_unpoisoned(self.queue);
        queue.frames.pop_front();
        self.front = queue.frames.front().cloned();
        self.in_flight = false;
        Ok(())
    }
    fn definitely_unsent(&mut self) -> Result<(), VmBridgeSendError> {
        if !self.in_flight {
            return Err(VmBridgeSendError::InvalidTransition);
        }
        self.in_flight = false;
        Ok(())
    }
}

impl Drop for PendingSendLease<'_> {
    fn drop(&mut self) {
        let mut queue = lock_unpoisoned(self.queue);
        queue.unknown |= self.in_flight;
        queue.owned = false;
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().expect("mock VM send custody mutex poisoned")
}

/// Deterministic in-memory implementation of `VmBridgeEffects` for tests.
#[derive(Debug, Default)]
pub struct MockVmBridgeEffects {
    outbound_payloads: Mutex<VecDeque<Vec<u8>>>,
    inbound_payloads: Mutex<VecDeque<Vec<u8>>>,
    branch_choices: Mutex<VecDeque<String>>,
    pending_sends: Mutex<PendingSendQueue>,
    blocked_edge: Mutex<Option<VmBridgeBlockedEdge>>,
    scheduler_signals: Mutex<VmBridgeSchedulerSignals>,
}

impl MockVmBridgeEffects {
    /// Create a new empty mock bridge state.
    pub fn new() -> Self {
        Self::default()
    }
}

impl VmBridgeEffects for MockVmBridgeEffects {
    fn enqueue_outbound_payload(&self, payload: Vec<u8>) {
        self.outbound_payloads
            .lock()
            .expect("mock VM bridge outbound mutex poisoned")
            .push_back(payload);
    }

    fn dequeue_outbound_payload(&self) -> Option<Vec<u8>> {
        self.outbound_payloads
            .lock()
            .expect("mock VM bridge outbound mutex poisoned")
            .pop_front()
    }

    fn enqueue_inbound_payload(&self, payload: Vec<u8>) {
        self.inbound_payloads
            .lock()
            .expect("mock VM bridge inbound mutex poisoned")
            .push_back(payload);
    }

    fn dequeue_inbound_payload(&self) -> Option<Vec<u8>> {
        self.inbound_payloads
            .lock()
            .expect("mock VM bridge inbound mutex poisoned")
            .pop_front()
    }

    fn enqueue_branch_choice(&self, label: String) {
        self.branch_choices
            .lock()
            .expect("mock VM bridge branch mutex poisoned")
            .push_back(label);
    }

    fn dequeue_branch_choice(&self) -> Option<String> {
        self.branch_choices
            .lock()
            .expect("mock VM bridge branch mutex poisoned")
            .pop_front()
    }

    fn record_pending_send(&self, send: VmBridgePendingSend) {
        lock_unpoisoned(&self.pending_sends).frames.push_back(send);
    }

    fn pending_send_snapshot(&self) -> Vec<VmBridgePendingSend> {
        lock_unpoisoned(&self.pending_sends)
            .frames
            .iter()
            .cloned()
            .collect()
    }

    fn lease_pending_sends(&self) -> Result<Box<dyn VmBridgeSendLease + '_>, VmBridgeSendError> {
        let mut queue = lock_unpoisoned(&self.pending_sends);
        if queue.unknown {
            return Err(VmBridgeSendError::DeliveryUnknown);
        }
        if queue.owned {
            return Err(VmBridgeSendError::AlreadyOwned);
        }
        queue.owned = true;
        Ok(Box::new(PendingSendLease {
            queue: &self.pending_sends,
            front: queue.frames.front().cloned(),
            in_flight: false,
        }))
    }

    fn set_blocked_edge(&self, edge: Option<VmBridgeBlockedEdge>) {
        *self
            .blocked_edge
            .lock()
            .expect("mock VM bridge blocked-edge mutex poisoned") = edge;
    }

    fn blocked_edge(&self) -> Option<VmBridgeBlockedEdge> {
        self.blocked_edge
            .lock()
            .expect("mock VM bridge blocked-edge mutex poisoned")
            .clone()
    }

    fn set_scheduler_signals(&self, signals: VmBridgeSchedulerSignals) {
        *self
            .scheduler_signals
            .lock()
            .expect("mock VM bridge scheduler mutex poisoned") = signals.normalized();
    }

    fn scheduler_signals(&self) -> VmBridgeSchedulerSignals {
        *self
            .scheduler_signals
            .lock()
            .expect("mock VM bridge scheduler mutex poisoned")
    }

    fn lease_metadata_snapshot(&self) -> VmBridgeLeaseMetadataSnapshot {
        VmBridgeLeaseMetadataSnapshot::default()
    }

    fn transfer_metadata_snapshot(&self) -> VmBridgeTransferMetadataSnapshot {
        VmBridgeTransferMetadataSnapshot::default()
    }
}
