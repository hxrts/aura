//! Flow budget windows, modelled on the AMP ratchet (docs/111 §3.1).
//!
//! A sender's receipt nonces are monotone generations. Each budget epoch is
//! anchored by a checkpoint at `base_gen`; the receiver accepts a receipt only
//! if its generation lies inside the epoch's window, keeps the previous window
//! open across an epoch boundary (dual window), and advances the epoch once
//! half the window is used (spacing rule). This module is pure accounting: the
//! facts that carry checkpoints and overrides live with their journal owner.

use crate::types::Epoch;
use serde::{Deserialize, Serialize};

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
    /// First generation of the window.
    pub base_gen: u64,
    /// Number of generations the window admits.
    pub window: u64,
}

impl FlowWindowCheckpoint {
    /// Whether `generation` lies inside this window.
    #[must_use]
    pub fn contains(&self, generation: u64) -> bool {
        generation >= self.base_gen && generation - self.base_gen < self.window
    }
}

/// Why a receipt generation was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowWindowRejection {
    /// The receipt names an epoch the receiver has not opened.
    UnknownEpoch,
    /// The generation is below the open window (a replay or stale receipt).
    BelowWindow,
    /// The generation is beyond the open window (more than the allowance).
    BeyondWindow,
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
            current: FlowWindowCheckpoint {
                epoch: Epoch::initial(),
                base_gen: 0,
                window,
            },
            previous: None,
        }
    }

    /// Accept or reject a receipt stamped with `epoch` and `generation`.
    pub fn accepts(&self, epoch: Epoch, generation: u64) -> Result<(), FlowWindowRejection> {
        let window = if epoch == self.current.epoch {
            self.current
        } else if let Some(previous) = self.previous.filter(|previous| previous.epoch == epoch) {
            previous
        } else {
            return Err(FlowWindowRejection::UnknownEpoch);
        };
        if generation < window.base_gen {
            Err(FlowWindowRejection::BelowWindow)
        } else if window.contains(generation) {
            Ok(())
        } else {
            Err(FlowWindowRejection::BeyondWindow)
        }
    }

    /// Spacing rule: the successor epoch is due once half the current window
    /// has been used by the highest accepted generation.
    #[must_use]
    pub fn should_advance(&self, highest_generation: u64) -> bool {
        highest_generation.saturating_sub(self.current.base_gen) >= self.current.window / 2
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
        assert_eq!(state.accepts(epoch, 0), Ok(()));
        assert_eq!(state.accepts(epoch, 3), Ok(()));
        assert_eq!(
            state.accepts(epoch, 4),
            Err(FlowWindowRejection::BeyondWindow)
        );
        assert_eq!(
            state.accepts(Epoch::new(7), 0),
            Err(FlowWindowRejection::UnknownEpoch)
        );
    }

    #[test]
    fn dual_window_keeps_in_flight_receipts_across_an_epoch_bump() {
        let mut state = FlowWindowState::initial(4);
        assert!(!state.should_advance(1));
        assert!(state.should_advance(2));
        state.advance(2, 4);

        // The sender had not seen the bump yet: epoch 0 receipts still land.
        assert_eq!(state.accepts(Epoch::initial(), 3), Ok(()));
        // The new epoch's window starts at its base generation.
        assert_eq!(state.accepts(Epoch::new(1), 2), Ok(()));
        assert_eq!(
            state.accepts(Epoch::new(1), 1),
            Err(FlowWindowRejection::BelowWindow)
        );

        // A second bump closes epoch 0 for good: its receipts are now stale.
        state.advance(4, 4);
        assert_eq!(
            state.accepts(Epoch::initial(), 3),
            Err(FlowWindowRejection::UnknownEpoch)
        );
    }

    #[test]
    fn a_zero_allowance_admits_nothing() {
        let state = FlowWindowState::initial(resolve_flow_window(Some(0), None));
        assert_eq!(
            state.accepts(Epoch::initial(), 0),
            Err(FlowWindowRejection::BeyondWindow)
        );
    }
}
