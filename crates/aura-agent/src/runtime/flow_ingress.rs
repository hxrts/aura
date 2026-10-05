//! Receiver-side flow budget enforcement (docs/111 §3.1).
//!
//! Each inbound receipt is admitted against the receiver's window for its
//! (context, sender authority, sender device) direction: every device keeps
//! its own generation counter, so devices of one authority must not share a
//! window. A sender authority gets at most [`MAX_DEVICE_WINDOWS`] device
//! windows per context, so inventing device ids buys no extra allowance
//! beyond that bound. When the receiver opens a successor epoch it queues the
//! checkpoint for delivery back to that device; checkpoints this runtime
//! receives are queued for adoption into its journal budget. Both queues are
//! flushed from async receive paths by the effect system.
//!
//! Window state is runtime-owned and in memory. After a restart the receiver
//! adopts the first receipt it sees from a sender device as that direction's
//! checkpoint, which resets accounting by at most one window per restart.

// Runtime-owned state guarded by short synchronous critical sections that
// never cross an `.await` (clippy.toml: allowed in aura-agent/src/runtime).
#![allow(clippy::disallowed_types)]

use aura_core::effects::transport::TransportReceipt;
use aura_core::types::flow_window::{
    resolve_flow_window, FlowReceiveWindow, FlowWindowCheckpoint, FlowWindowRejection,
};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::Epoch;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Content type of a checkpoint a receiver sends back to a sender.
pub(crate) const FLOW_CHECKPOINT_CONTENT_TYPE: &str = "application/aura-flow-checkpoint";

/// Device windows a sender authority may hold per context.
pub(crate) const MAX_DEVICE_WINDOWS: usize = 16;

/// One direction's checkpoint, addressed by the context the receipts use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FlowCheckpointNotice {
    /// Context of the receipts in the direction this checkpoint governs.
    pub context: ContextId,
    /// The other party: the sender to notify (outbound) or the receiver that
    /// issued the checkpoint (inbound).
    pub peer: AuthorityId,
    /// The sending device the checkpoint is for, when known.
    pub device: Option<String>,
    /// The successor window.
    pub checkpoint: FlowWindowCheckpoint,
}

type Direction = (ContextId, AuthorityId, Option<String>);

#[derive(Debug, Default)]
pub(crate) struct FlowIngress {
    windows: Mutex<HashMap<Direction, FlowReceiveWindow>>,
    allowances: Mutex<HashMap<(ContextId, AuthorityId), u64>>,
    readmit: Mutex<HashSet<(Direction, u64, u64)>>,
    outbound: Mutex<Vec<FlowCheckpointNotice>>,
    inbound: Mutex<Vec<FlowCheckpointNotice>>,
    /// Directions whose window bounds changed since the last persist.
    dirty_windows: Mutex<HashSet<Direction>>,
    /// Whether allowances changed since the last persist.
    dirty_allowances: std::sync::atomic::AtomicBool,
    /// Whether persisted state was restored after start.
    restored: std::sync::atomic::AtomicBool,
}

/// A persisted receive window (work/8.md Task 54).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PersistedFlowWindow {
    pub context: ContextId,
    pub peer: AuthorityId,
    pub device: Option<String>,
    pub state: aura_core::types::flow_window::FlowWindowState,
}

/// Persisted allowance overrides (work/8.md Task 54).
pub(crate) type PersistedAllowances = Vec<(ContextId, AuthorityId, u64)>;

impl FlowIngress {
    /// Override the window granted to `peer` in `context` from the next epoch.
    // Callers arrive with a user-facing allowance control (work/8.md Task 54).
    #[allow(dead_code)]
    pub(crate) fn set_allowance(&self, context: ContextId, peer: AuthorityId, window: u64) {
        self.allowances.lock().insert((context, peer), window);
        self.dirty_allowances
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether persisted windows still need restoring after start.
    pub(crate) fn needs_restore(&self) -> bool {
        !self.restored.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Restore persisted windows and allowances. Windows already live in
    /// memory (admitted before the restore ran) are kept.
    pub(crate) fn restore(
        &self,
        windows: Vec<PersistedFlowWindow>,
        allowances: PersistedAllowances,
    ) {
        {
            let mut live = self.windows.lock();
            for persisted in windows {
                live.entry((persisted.context, persisted.peer, persisted.device))
                    .or_insert_with(|| FlowReceiveWindow::from_state(persisted.state));
            }
        }
        {
            let mut live = self.allowances.lock();
            for (context, peer, window) in allowances {
                live.entry((context, peer)).or_insert(window);
            }
        }
        self.restored
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Windows whose bounds changed since the last call, for persistence.
    pub(crate) fn take_dirty_windows(&self) -> Vec<PersistedFlowWindow> {
        let dirty = std::mem::take(&mut *self.dirty_windows.lock());
        let windows = self.windows.lock();
        dirty
            .into_iter()
            .filter_map(|direction| {
                let state = windows.get(&direction)?.state.clone();
                let (context, peer, device) = direction;
                Some(PersistedFlowWindow {
                    context,
                    peer,
                    device,
                    state,
                })
            })
            .collect()
    }

    /// All allowances if any changed since the last call.
    pub(crate) fn take_dirty_allowances(&self) -> Option<PersistedAllowances> {
        self.dirty_allowances
            .swap(false, std::sync::atomic::Ordering::Relaxed)
            .then(|| {
                self.allowances
                    .lock()
                    .iter()
                    .map(|((context, peer), window)| (*context, *peer, *window))
                    .collect()
            })
    }

    fn window_for(&self, context: ContextId, peer: AuthorityId) -> u64 {
        resolve_flow_window(self.allowances.lock().get(&(context, peer)).copied(), None)
    }

    /// Admit an inbound receipt from `device` or reject it (over allowance,
    /// replayed, stale or forged epoch, or too many device windows).
    pub(crate) fn admit(
        &self,
        receipt: &TransportReceipt,
        device: Option<&str>,
    ) -> Result<(), FlowWindowRejection> {
        let direction = (receipt.context, receipt.src, device.map(str::to_string));
        if self
            .readmit
            .lock()
            .remove(&(direction.clone(), receipt.epoch, receipt.nonce))
        {
            return Ok(());
        }
        let window = self.window_for(receipt.context, receipt.src);
        let epoch = Epoch::new(receipt.epoch);
        let mut windows = self.windows.lock();
        if !windows.contains_key(&direction)
            && windows
                .keys()
                .filter(|(context, src, _)| *context == receipt.context && *src == receipt.src)
                .count()
                >= MAX_DEVICE_WINDOWS
        {
            return Err(FlowWindowRejection::BeyondWindow);
        }
        if !windows.contains_key(&direction) {
            self.dirty_windows.lock().insert(direction.clone());
        }
        let state = windows.entry(direction.clone()).or_insert_with(|| {
            FlowReceiveWindow::from_checkpoint(FlowWindowCheckpoint {
                epoch,
                base_gen: receipt.nonce.saturating_sub(1),
                window,
            })
        });
        let current = state.state.current;
        match state.accept(epoch, receipt.nonce, window)? {
            Some(checkpoint) => {
                self.dirty_windows.lock().insert(direction.clone());
                self.queue_outbound(direction, checkpoint);
            }
            // The sender is still stamping the previous epoch: it has not
            // adopted the current checkpoint, so offer it again.
            None if epoch != current.epoch => self.queue_outbound(direction, current),
            None => {}
        }
        Ok(())
    }

    /// Let a receipt this runtime already admitted be admitted once more, for
    /// envelopes taken from the inbox and put back unconsumed.
    pub(crate) fn allow_readmit(&self, receipt: &TransportReceipt, device: Option<&str>) {
        self.readmit.lock().insert((
            (receipt.context, receipt.src, device.map(str::to_string)),
            receipt.epoch,
            receipt.nonce,
        ));
    }

    fn queue_outbound(&self, (context, peer, device): Direction, checkpoint: FlowWindowCheckpoint) {
        let mut outbound = self.outbound.lock();
        outbound.retain(|notice| {
            !(notice.context == context && notice.peer == peer && notice.device == device)
        });
        outbound.push(FlowCheckpointNotice {
            context,
            peer,
            device,
            checkpoint,
        });
    }

    /// Record a checkpoint received from `issuer` for adoption.
    pub(crate) fn record_inbound(&self, issuer: AuthorityId, payload: &[u8]) -> bool {
        let Ok(notice) =
            aura_core::util::serialization::from_slice::<FlowCheckpointNotice>(payload)
        else {
            return false;
        };
        self.inbound.lock().push(FlowCheckpointNotice {
            peer: issuer,
            ..notice
        });
        true
    }

    pub(crate) fn take_outbound(&self) -> Vec<FlowCheckpointNotice> {
        std::mem::take(&mut *self.outbound.lock())
    }

    pub(crate) fn take_inbound(&self) -> Vec<FlowCheckpointNotice> {
        std::mem::take(&mut *self.inbound.lock())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(src: u8, epoch: u64, nonce: u64) -> TransportReceipt {
        TransportReceipt {
            context: ContextId::new_from_entropy([9; 32]),
            src: AuthorityId::new_from_entropy([src; 32]),
            dst: AuthorityId::new_from_entropy([1; 32]),
            epoch,
            cost: 1,
            nonce,
            prev: [0; 32],
            sig: vec![1],
        }
    }

    #[test]
    fn receipts_beyond_the_allowance_are_rejected_until_a_checkpoint() {
        let ingress = FlowIngress::default();
        let sender = AuthorityId::new_from_entropy([2; 32]);
        let context = ContextId::new_from_entropy([9; 32]);
        ingress.set_allowance(context, sender, 4);
        // The first receipt fixes the window (4 above generation 0).
        for nonce in 1..=4 {
            ingress
                .admit(&receipt(2, 0, nonce), None)
                .expect("in window");
        }
        assert_eq!(
            ingress.admit(&receipt(2, 0, 5), None),
            Err(FlowWindowRejection::BeyondWindow)
        );
        let outbound = ingress.take_outbound();
        assert_eq!(outbound.len(), 1, "one checkpoint for the sender");
        assert_eq!(outbound[0].peer, sender);
        assert_eq!(outbound[0].checkpoint.epoch.value(), 1);
        ingress
            .admit(&receipt(2, 1, 5), None)
            .expect("the successor window admits it");
    }

    #[test]
    fn replays_are_rejected_but_a_requeued_envelope_is_readmitted_once() {
        let ingress = FlowIngress::default();
        let first = receipt(3, 0, 1);
        ingress.admit(&first, None).expect("admitted");
        assert_eq!(
            ingress.admit(&first, None),
            Err(FlowWindowRejection::Replay)
        );
        ingress.allow_readmit(&first, None);
        ingress
            .admit(&first, None)
            .expect("readmitted after requeue");
        assert_eq!(
            ingress.admit(&first, None),
            Err(FlowWindowRejection::Replay)
        );
    }

    #[test]
    fn a_lagging_sender_is_offered_the_current_checkpoint_again() {
        let ingress = FlowIngress::default();
        let context = ContextId::new_from_entropy([9; 32]);
        let sender = AuthorityId::new_from_entropy([4; 32]);
        ingress.set_allowance(context, sender, 4);
        ingress.admit(&receipt(4, 0, 1), None).expect("admitted");
        ingress
            .admit(&receipt(4, 0, 2), None)
            .expect("admitted, bumps");
        assert_eq!(ingress.take_outbound().len(), 1);
        ingress
            .admit(&receipt(4, 0, 3), None)
            .expect("old epoch still open");
        let resent = ingress.take_outbound();
        assert_eq!(resent.len(), 1);
        assert_eq!(resent[0].checkpoint.epoch.value(), 1);
    }

    #[test]
    fn inbound_checkpoints_are_attributed_to_their_issuer() {
        let ingress = FlowIngress::default();
        let issuer = AuthorityId::new_from_entropy([5; 32]);
        let forged_peer = AuthorityId::new_from_entropy([6; 32]);
        let notice = FlowCheckpointNotice {
            context: ContextId::new_from_entropy([9; 32]),
            peer: forged_peer,
            device: None,
            checkpoint: FlowWindowCheckpoint::initial(8),
        };
        let payload = aura_core::util::serialization::to_vec(&notice).expect("encode");
        assert!(ingress.record_inbound(issuer, &payload));
        assert!(!ingress.record_inbound(issuer, b"garbage"));
        let inbound = ingress.take_inbound();
        assert_eq!(inbound.len(), 1);
        assert_eq!(inbound[0].peer, issuer);
    }

    #[test]
    fn devices_of_one_authority_have_separate_windows() {
        let ingress = FlowIngress::default();
        // Both devices count their own generations from 1.
        ingress
            .admit(&receipt(7, 0, 1), Some("device-a"))
            .expect("first device");
        ingress
            .admit(&receipt(7, 0, 1), Some("device-b"))
            .expect("second device is not a replay of the first");
        assert_eq!(
            ingress.admit(&receipt(7, 0, 1), Some("device-a")),
            Err(FlowWindowRejection::Replay)
        );
    }

    #[test]
    fn invented_device_ids_are_capped_per_authority() {
        let ingress = FlowIngress::default();
        for device in 0..MAX_DEVICE_WINDOWS {
            ingress
                .admit(&receipt(8, 0, 1), Some(&format!("device-{device}")))
                .expect("within the device cap");
        }
        assert_eq!(
            ingress.admit(&receipt(8, 0, 1), Some("one-too-many")),
            Err(FlowWindowRejection::BeyondWindow)
        );
    }

    #[test]
    fn restored_windows_survive_a_restart_without_resetting_accounting() {
        let context = ContextId::new_from_entropy([9; 32]);
        let sender = AuthorityId::new_from_entropy([12; 32]);
        let before = FlowIngress::default();
        before.set_allowance(context, sender, 4);
        for nonce in 1..=2 {
            before
                .admit(&receipt(12, 0, nonce), None)
                .expect("admitted");
        }
        let windows = before.take_dirty_windows();
        let allowances = before.take_dirty_allowances().expect("allowance changed");
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].state.current.epoch.value(), 1, "bump persisted");
        assert!(before.take_dirty_windows().is_empty(), "dirty set drained");

        // A fresh runtime restores instead of trusting the next receipt.
        let after = FlowIngress::default();
        assert!(after.needs_restore());
        after.restore(windows, allowances);
        assert!(!after.needs_restore());
        // Epoch 0 is still open as the previous window; epoch 7 was never
        // opened, so it is not adopted as a fresh checkpoint.
        after
            .admit(&receipt(12, 0, 3), None)
            .expect("previous window");
        assert_eq!(
            after.admit(&receipt(12, 7, 50), None),
            Err(FlowWindowRejection::UnknownEpoch)
        );
        // The restored allowance still bounds the current window.
        assert_eq!(
            after.admit(&receipt(12, 1, 7), None),
            Err(FlowWindowRejection::BeyondWindow)
        );
    }
}
