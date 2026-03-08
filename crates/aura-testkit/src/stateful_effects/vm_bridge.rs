#![allow(clippy::disallowed_types)]

//! Stateful VM bridge effects for deterministic testing.

use aura_core::effects::{
    VmBridgeBlockedEdge, VmBridgeEffects, VmBridgeLeaseMetadataSnapshot, VmBridgePendingSend,
    VmBridgeSchedulerSignals, VmBridgeTransferMetadataSnapshot,
};
use std::collections::VecDeque;
use std::sync::Mutex;

/// Deterministic in-memory implementation of `VmBridgeEffects` for tests.
#[derive(Debug, Default)]
pub struct MockVmBridgeEffects {
    outbound_payloads: Mutex<VecDeque<Vec<u8>>>,
    inbound_payloads: Mutex<VecDeque<Vec<u8>>>,
    branch_choices: Mutex<VecDeque<String>>,
    pending_sends: Mutex<VecDeque<VmBridgePendingSend>>,
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
        self.pending_sends
            .lock()
            .expect("mock VM bridge pending-send mutex poisoned")
            .push_back(send);
    }

    fn drain_pending_sends(&self) -> Vec<VmBridgePendingSend> {
        self.pending_sends
            .lock()
            .expect("mock VM bridge pending-send mutex poisoned")
            .drain(..)
            .collect()
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
