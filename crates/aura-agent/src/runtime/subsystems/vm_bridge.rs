#![allow(clippy::disallowed_types)]

//! Session-local bridge state for the Aura and Telltale runtime boundary.

use aura_core::effects::{
    VmBridgeBlockedEdge, VmBridgeEffects, VmBridgeLeaseMetadataSnapshot, VmBridgePendingSend,
    VmBridgeSchedulerSignals, VmBridgeTransferMetadataSnapshot,
};
use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard};

/// Production in-memory implementation of the synchronous VM bridge effect surface.
#[derive(Debug, Default)]
pub struct VmBridgeState {
    outbound_payloads: Mutex<VecDeque<Vec<u8>>>,
    inbound_payloads: Mutex<VecDeque<Vec<u8>>>,
    branch_choices: Mutex<VecDeque<String>>,
    pending_sends: Mutex<VecDeque<VmBridgePendingSend>>,
    blocked_edge: Mutex<Option<VmBridgeBlockedEdge>>,
    scheduler_signals: Mutex<VmBridgeSchedulerSignals>,
}

impl VmBridgeState {
    /// Create an empty bridge state for one admitted VM fragment.
    pub fn new() -> Self {
        Self::default()
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .expect("VM bridge state mutex poisoned during deterministic runtime execution")
}

impl VmBridgeEffects for VmBridgeState {
    fn enqueue_outbound_payload(&self, payload: Vec<u8>) {
        lock_unpoisoned(&self.outbound_payloads).push_back(payload);
    }

    fn dequeue_outbound_payload(&self) -> Option<Vec<u8>> {
        lock_unpoisoned(&self.outbound_payloads).pop_front()
    }

    fn enqueue_inbound_payload(&self, payload: Vec<u8>) {
        lock_unpoisoned(&self.inbound_payloads).push_back(payload);
    }

    fn dequeue_inbound_payload(&self) -> Option<Vec<u8>> {
        lock_unpoisoned(&self.inbound_payloads).pop_front()
    }

    fn enqueue_branch_choice(&self, label: String) {
        lock_unpoisoned(&self.branch_choices).push_back(label);
    }

    fn dequeue_branch_choice(&self) -> Option<String> {
        lock_unpoisoned(&self.branch_choices).pop_front()
    }

    fn record_pending_send(&self, send: VmBridgePendingSend) {
        lock_unpoisoned(&self.pending_sends).push_back(send);
    }

    fn drain_pending_sends(&self) -> Vec<VmBridgePendingSend> {
        lock_unpoisoned(&self.pending_sends).drain(..).collect()
    }

    fn set_blocked_edge(&self, edge: Option<VmBridgeBlockedEdge>) {
        *lock_unpoisoned(&self.blocked_edge) = edge;
    }

    fn blocked_edge(&self) -> Option<VmBridgeBlockedEdge> {
        lock_unpoisoned(&self.blocked_edge).clone()
    }

    fn set_scheduler_signals(&self, signals: VmBridgeSchedulerSignals) {
        *lock_unpoisoned(&self.scheduler_signals) = signals.normalized();
    }

    fn scheduler_signals(&self) -> VmBridgeSchedulerSignals {
        *lock_unpoisoned(&self.scheduler_signals)
    }

    fn lease_metadata_snapshot(&self) -> VmBridgeLeaseMetadataSnapshot {
        VmBridgeLeaseMetadataSnapshot::default()
    }

    fn transfer_metadata_snapshot(&self) -> VmBridgeTransferMetadataSnapshot {
        VmBridgeTransferMetadataSnapshot::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_drains_pending_sends() {
        let state = VmBridgeState::new();
        state.record_pending_send(VmBridgePendingSend {
            from_role: "A".to_string(),
            to_role: "B".to_string(),
            label: "Msg".to_string(),
            payload: vec![1, 2, 3],
        });

        let drained = state.drain_pending_sends();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].payload, vec![1, 2, 3]);
        assert!(state.drain_pending_sends().is_empty());
    }

    #[test]
    fn normalizes_scheduler_signals() {
        let state = VmBridgeState::new();
        state.set_scheduler_signals(VmBridgeSchedulerSignals {
            guard_contention_events: 5,
            flow_budget_pressure_bps: 12_000,
            leakage_budget_pressure_bps: 50,
        });

        assert_eq!(
            state.scheduler_signals(),
            VmBridgeSchedulerSignals {
                guard_contention_events: 5,
                flow_budget_pressure_bps: 10_000,
                leakage_budget_pressure_bps: 50,
            }
        );
    }
}
