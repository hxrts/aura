//! Flow budget windows, modelled on the AMP ratchet (docs/111 §3.1).
//!
//! A sender stamps each receipt with its budget epoch and a monotone
//! generation (the receipt nonce, starting at 1). Each epoch is anchored by a
//! checkpoint at `base_gen`; the receiver accepts a generation only if it is
//! unseen and lies in `(base_gen, base_gen + window]` for the current epoch,
//! or for the previous epoch across a boundary (dual window).
//!
//! Only the receiver advances the epoch, and only after it has *accepted*
//! half a window of receipts in the current epoch (spacing rule). Counting
//! accepted receipts rather than trusting stamped generations means a sender
//! cannot skip generations or forge epochs to send more than its allowance.
//! The receiver hands the new checkpoint back to the sender, which then sends
//! up to `base_gen + window`. A sender that loses more than half a window of
//! receipts in one epoch stalls until the receiver bumps.
//!
//! This module is pure accounting: storage and delivery of checkpoints live
//! with their runtime owner.

use crate::types::Epoch;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Default flow window (allowance) per budget epoch, as AMP's skip window.
pub const DEFAULT_FLOW_WINDOW: u64 = 1024;

/// Resolve the window size: per-peer override, then context policy, then the
/// default. A zero override or policy means no sends are allowed.
#[must_use]
pub fn resolve_flow_window(peer_override: Option<u64>, context_policy: Option<u64>) -> u64 {
    peer_override
        .or(context_policy)
        .unwrap_or(DEFAULT_FLOW_WINDOW)
}

/// The anchor of one budget epoch's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowWindowCheckpoint {
    /// Budget epoch this window belongs to.
    pub epoch: Epoch,
    /// Generation the window starts above.
    pub base_gen: u64,
    /// Number of generations the window admits.
    pub window: u64,
}

impl FlowWindowCheckpoint {
    /// The initial checkpoint: epoch 0 anchored at generation 0.
    #[must_use]
    pub fn initial(window: u64) -> Self {
        Self {
            epoch: Epoch::initial(),
            base_gen: 0,
            window,
        }
    }

    /// Highest generation this window admits; the sender's absolute limit.
    #[must_use]
    pub fn limit(&self) -> u64 {
        self.base_gen.saturating_add(self.window)
    }

    /// Whether `generation` lies inside this window.
    #[must_use]
    pub fn contains(&self, generation: u64) -> bool {
        generation > self.base_gen && generation <= self.limit()
    }
}

/// Why a receipt generation was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowWindowRejection {
    /// The receipt names an epoch the receiver has not opened.
    UnknownEpoch,
    /// The generation is at or below the open window (stale).
    BelowWindow,
    /// The generation is beyond the open window (more than the allowance).
    BeyondWindow,
    /// The generation was already accepted (a replay).
    Replay,
}

impl std::fmt::Display for FlowWindowRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UnknownEpoch => "receipt names an unknown flow budget epoch",
            Self::BelowWindow => "receipt generation is below the flow window",
            Self::BeyondWindow => "receipt generation exceeds the flow allowance",
            Self::Replay => "receipt generation was already accepted",
        })
    }
}

/// The receiver's view of one direction's budget: the current window and, at
/// an epoch boundary, the previous one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowWindowState {
    /// The window of the current epoch.
    pub current: FlowWindowCheckpoint,
    /// The previous epoch's window, kept open across the boundary.
    pub previous: Option<FlowWindowCheckpoint>,
}

impl FlowWindowState {
    /// The initial state: epoch 0 anchored at generation 0.
    #[must_use]
    pub fn initial(window: u64) -> Self {
        Self {
            current: FlowWindowCheckpoint::initial(window),
            previous: None,
        }
    }

    /// Accept or reject a receipt stamped with `epoch` and `generation`,
    /// ignoring replays (see [`FlowReceiveWindow`]).
    pub fn accepts(&self, epoch: Epoch, generation: u64) -> Result<(), FlowWindowRejection> {
        let window = if epoch == self.current.epoch {
            self.current
        } else if let Some(previous) = self.previous.filter(|previous| previous.epoch == epoch) {
            previous
        } else {
            return Err(FlowWindowRejection::UnknownEpoch);
        };
        if generation <= window.base_gen {
            Err(FlowWindowRejection::BelowWindow)
        } else if window.contains(generation) {
            Ok(())
        } else {
            Err(FlowWindowRejection::BeyondWindow)
        }
    }

    /// Open the successor epoch at `base_gen` with `window`, keeping the
    /// current window open as the previous one.
    pub fn advance(&mut self, base_gen: u64, window: u64) {
        self.previous = Some(self.current);
        self.current = FlowWindowCheckpoint {
            epoch: Epoch::new(self.current.epoch.value().saturating_add(1)),
            base_gen,
            window,
        };
    }
}

/// Receiver-side enforcement for one (context, sender) direction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowReceiveWindow {
    /// Current and previous windows.
    pub state: FlowWindowState,
    /// Generations accepted in the open windows, for replay rejection.
    seen: BTreeSet<u64>,
    /// Receipts accepted in the current epoch; drives the spacing rule.
    accepted_in_epoch: u64,
}

impl FlowReceiveWindow {
    /// A fresh direction at the initial checkpoint.
    #[must_use]
    pub fn initial(window: u64) -> Self {
        Self::from_checkpoint(FlowWindowCheckpoint::initial(window))
    }

    /// Resume enforcement at a known checkpoint, such as one adopted on first
    /// contact after the receiver lost its local window state.
    #[must_use]
    pub fn from_checkpoint(checkpoint: FlowWindowCheckpoint) -> Self {
        Self {
            state: FlowWindowState {
                current: checkpoint,
                previous: None,
            },
            seen: BTreeSet::new(),
            accepted_in_epoch: 0,
        }
    }

    /// Resume enforcement from persisted windows after a restart. Replay
    /// tracking and the spacing count start empty, so at most one window of
    /// receipts already accepted before the restart could be accepted again;
    /// the window bounds themselves are kept.
    #[must_use]
    pub fn from_state(state: FlowWindowState) -> Self {
        Self {
            state,
            seen: BTreeSet::new(),
            accepted_in_epoch: 0,
        }
    }

    /// Accept a receipt or reject it. When this receipt completes half the
    /// current window, the receiver opens the successor epoch with
    /// `next_window` and returns its checkpoint for delivery to the sender.
    pub fn accept(
        &mut self,
        epoch: Epoch,
        generation: u64,
        next_window: u64,
    ) -> Result<Option<FlowWindowCheckpoint>, FlowWindowRejection> {
        self.state.accepts(epoch, generation)?;
        if !self.seen.insert(generation) {
            return Err(FlowWindowRejection::Replay);
        }
        if epoch != self.state.current.epoch {
            return Ok(None);
        }
        self.accepted_in_epoch += 1;
        let current = self.state.current;
        if current.window == 0 || self.accepted_in_epoch < current.window.div_ceil(2) {
            return Ok(None);
        }
        let highest = self
            .seen
            .range(current.base_gen.saturating_add(1)..)
            .next_back()
            .copied()
            .unwrap_or(generation);
        self.state.advance(highest, next_window);
        self.accepted_in_epoch = 0;
        let floor = self
            .state
            .previous
            .map_or(self.state.current.base_gen, |previous| previous.base_gen);
        self.seen = self.seen.split_off(&floor.saturating_add(1));
        Ok(Some(self.state.current))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_resolves_override_then_policy_then_default() {
        assert_eq!(resolve_flow_window(Some(10), Some(20)), 10);
        assert_eq!(resolve_flow_window(None, Some(20)), 20);
        assert_eq!(resolve_flow_window(None, None), DEFAULT_FLOW_WINDOW);
    }

    #[test]
    fn receipts_are_accepted_only_inside_the_window() {
        let state = FlowWindowState::initial(4);
        let epoch = Epoch::initial();
        assert_eq!(
            state.accepts(epoch, 0),
            Err(FlowWindowRejection::BelowWindow)
        );
        assert_eq!(state.accepts(epoch, 1), Ok(()));
        assert_eq!(state.accepts(epoch, 4), Ok(()));
        assert_eq!(
            state.accepts(epoch, 5),
            Err(FlowWindowRejection::BeyondWindow)
        );
        assert_eq!(
            state.accepts(Epoch::new(7), 1),
            Err(FlowWindowRejection::UnknownEpoch)
        );
    }

    #[test]
    fn a_sender_within_its_allowance_keeps_sending_through_bumps() {
        let mut receiver = FlowReceiveWindow::initial(4);
        let mut sender = FlowWindowCheckpoint::initial(4);
        let mut generation = 0;
        for _ in 0..100 {
            assert!(generation < sender.limit(), "sender stays within allowance");
            generation += 1;
            if let Some(checkpoint) = receiver
                .accept(sender.epoch, generation, 4)
                .expect("in-window receipt accepted")
            {
                sender = checkpoint;
            }
        }
        assert!(receiver.state.current.epoch.value() >= 40);
    }

    #[test]
    fn dual_window_keeps_in_flight_receipts_across_an_epoch_bump() {
        let mut receiver = FlowReceiveWindow::initial(4);
        let epoch0 = Epoch::initial();
        assert_eq!(receiver.accept(epoch0, 1, 4), Ok(None));
        let bump = receiver
            .accept(epoch0, 2, 4)
            .expect("accepted")
            .expect("half the window opens the successor");
        assert_eq!((bump.epoch.value(), bump.base_gen), (1, 2));

        // Stamped before the sender saw the bump: still accepted.
        assert_eq!(receiver.accept(epoch0, 3, 4), Ok(None));
        // The successor window starts above its base generation.
        assert_eq!(receiver.accept(Epoch::new(1), 5, 4), Ok(None));
        assert_eq!(
            receiver.accept(Epoch::new(1), 2, 4),
            Err(FlowWindowRejection::BelowWindow)
        );
    }

    #[test]
    fn replays_are_rejected() {
        let mut receiver = FlowReceiveWindow::initial(8);
        assert_eq!(receiver.accept(Epoch::initial(), 1, 8), Ok(None));
        assert_eq!(
            receiver.accept(Epoch::initial(), 1, 8),
            Err(FlowWindowRejection::Replay)
        );
    }

    #[test]
    fn skipped_generations_and_forged_epochs_do_not_buy_extra_sends() {
        let mut receiver = FlowReceiveWindow::initial(8);
        let epoch0 = Epoch::initial();
        // Jumping to the top of the window is accepted once but does not
        // count as half a window of traffic.
        assert_eq!(receiver.accept(epoch0, 8, 8), Ok(None));
        assert_eq!(
            receiver.accept(epoch0, 9, 8),
            Err(FlowWindowRejection::BeyondWindow)
        );
        // Claiming an epoch the receiver never opened is rejected.
        assert_eq!(
            receiver.accept(Epoch::new(1), 9, 8),
            Err(FlowWindowRejection::UnknownEpoch)
        );
        assert_eq!(receiver.state.current.epoch, epoch0);
    }

    #[test]
    fn a_sender_that_loses_receipts_still_gets_replenished() {
        // Every other receipt is lost: the receiver still sees half a window
        // before the sender reaches its limit.
        let mut receiver = FlowReceiveWindow::initial(8);
        let mut sender = FlowWindowCheckpoint::initial(8);
        let mut generation = 0;
        for send in 0..200 {
            assert!(generation < sender.limit(), "send {send} within allowance");
            generation += 1;
            if generation % 2 == 0 {
                continue;
            }
            if let Some(checkpoint) = receiver
                .accept(sender.epoch, generation, 8)
                .expect("accepted")
            {
                sender = checkpoint;
            }
        }
    }

    #[test]
    fn a_zero_allowance_admits_nothing() {
        let state = FlowWindowState::initial(resolve_flow_window(Some(0), None));
        assert_eq!(
            state.accepts(Epoch::initial(), 1),
            Err(FlowWindowRejection::BeyondWindow)
        );
    }

    #[test]
    fn an_override_changes_the_next_window() {
        let mut receiver = FlowReceiveWindow::initial(4);
        receiver.accept(Epoch::initial(), 1, 4).expect("accepted");
        let bump = receiver
            .accept(Epoch::initial(), 2, 16)
            .expect("accepted")
            .expect("bumped");
        assert_eq!(bump.window, 16);
        assert_eq!(bump.limit(), 18);
    }
}
