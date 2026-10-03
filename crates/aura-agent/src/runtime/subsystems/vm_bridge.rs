#![allow(clippy::disallowed_types)]

//! Session-local bridge state for the Aura and Telltale runtime boundary.

use aura_core::effects::{
    VmBridgeBlockedEdge, VmBridgeEffects, VmBridgeLeaseMetadataSnapshot, VmBridgePendingSend,
    VmBridgeSchedulerSignals, VmBridgeSendError, VmBridgeSendLease,
    VmBridgeTransferMetadataSnapshot,
};
use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard};

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

/// Production in-memory implementation of the synchronous VM bridge effect surface.
#[derive(Debug, Default)]
pub struct VmBridgeState {
    outbound_payloads: Mutex<VecDeque<Vec<u8>>>,
    inbound_payloads: Mutex<VecDeque<Vec<u8>>>,
    branch_choices: Mutex<VecDeque<String>>,
    pending_sends: Mutex<PendingSendQueue>,
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

        let drained = state.pending_send_snapshot();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].payload, vec![1, 2, 3]);
        let mut lease = state.lease_pending_sends().unwrap();
        lease.begin_delivery().unwrap();
        lease.acknowledge().unwrap();
        assert!(state.pending_send_snapshot().is_empty());
    }

    #[test]
    fn delivery_lease_retains_order_and_requires_acknowledgment() {
        let state = VmBridgeState::new();
        let frame = |value| VmBridgePendingSend {
            payload: vec![value],
            ..Default::default()
        };
        state.record_pending_send(frame(1));
        state.record_pending_send(frame(2));
        let mut lease = state.lease_pending_sends().unwrap();
        assert!(matches!(
            state.lease_pending_sends(),
            Err(VmBridgeSendError::AlreadyOwned)
        ));
        assert!(matches!(
            lease.acknowledge(),
            Err(VmBridgeSendError::InvalidTransition)
        ));
        lease.begin_delivery().unwrap();
        state.record_pending_send(frame(3));
        lease.acknowledge().unwrap();
        lease.begin_delivery().unwrap();
        lease.definitely_unsent().unwrap();
        drop(lease);
        assert_eq!(state.pending_send_snapshot(), vec![frame(2), frame(3)]);
        let lease = state.lease_pending_sends().unwrap();
        assert_eq!(lease.pending(), Some(&frame(2)));
    }

    #[test]
    fn cancelled_delivery_retains_frame_and_prevents_automatic_replay() {
        let state = VmBridgeState::new();
        let frame = VmBridgePendingSend {
            payload: vec![9],
            ..Default::default()
        };
        state.record_pending_send(frame.clone());
        let mut lease = state.lease_pending_sends().unwrap();
        lease.begin_delivery().unwrap();
        drop(lease);
        assert_eq!(state.pending_send_snapshot(), vec![frame]);
        assert!(matches!(
            state.lease_pending_sends(),
            Err(VmBridgeSendError::DeliveryUnknown)
        ));
    }

    #[tokio::test]
    async fn cancelling_an_awaited_delivery_future_retains_unknown_outcome() {
        let state = VmBridgeState::new();
        let frame = VmBridgePendingSend {
            payload: vec![7],
            ..Default::default()
        };
        state.record_pending_send(frame.clone());
        {
            let delivery = async {
                let mut lease = state.lease_pending_sends().unwrap();
                lease.begin_delivery().unwrap();
                std::future::pending::<()>().await;
                lease.acknowledge().unwrap();
            };
            tokio::pin!(delivery);
            tokio::select! {
                biased;
                _ = &mut delivery => panic!("delivery must remain in flight"),
                _ = std::future::ready(()) => {}
            }
        }
        assert_eq!(state.pending_send_snapshot(), vec![frame]);
        assert!(matches!(
            state.lease_pending_sends(),
            Err(VmBridgeSendError::DeliveryUnknown)
        ));
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
