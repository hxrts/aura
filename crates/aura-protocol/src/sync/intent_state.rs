//! IntentState - Typestate Lattice for Tree Operation Proposals
//!
//! Implements a state machine for tracking tree operation proposals through
//! their lifecycle from proposal to finalization or abortion. The states form
//! a partial order that only allows forward progression (no rollbacks).
//!
//! ## State Transitions
//!
//! ```text
//! Proposed → Attesting → Finalized
//!     ↓
//!  Aborted
//! ```
//!
//! ## Partial Ordering
//!
//! - `Proposed < Attesting < Finalized`
//! - `Proposed < Aborted`
//! - `Attesting` and `Aborted` are incomparable (concurrent states)
//! - `Finalized` and `Aborted` are incomparable (terminal states)
//!
//! ## Design Principles
//!
//! - **No Rollbacks**: States only move forward
//! - **LWW Tie-Breaker**: Last-Write-Wins for concurrent updates
//! - **Terminal States**: Finalized and Aborted are final
//! - **Monotonic**: State progression never decreases

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// State of a tree operation intent/proposal
///
/// IntentState tracks where a tree operation is in its lifecycle.
/// States form a partial order with only forward transitions allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntentState {
    /// Operation has been proposed but not yet signed
    Proposed { timestamp: u64 },

    /// Operation is being signed by threshold participants
    Attesting { timestamp: u64, collected: u16 },

    /// Operation has been finalized with threshold signatures
    Finalized { timestamp: u64 },

    /// Operation was aborted (not enough signatures, timeout, etc.)
    Aborted { timestamp: u64, reason: u8 },
}

impl IntentState {
    /// Create a new proposed intent
    pub fn proposed(timestamp: u64) -> Self {
        Self::Proposed { timestamp }
    }

    /// Create a new attesting intent
    pub fn attesting(timestamp: u64, collected: u16) -> Self {
        Self::Attesting {
            timestamp,
            collected,
        }
    }

    /// Create a new finalized intent
    pub fn finalized(timestamp: u64) -> Self {
        Self::Finalized { timestamp }
    }

    /// Create a new aborted intent
    pub fn aborted(timestamp: u64, reason: u8) -> Self {
        Self::Aborted { timestamp, reason }
    }

    /// Get the timestamp of this state
    pub fn timestamp(&self) -> u64 {
        match self {
            Self::Proposed { timestamp } => *timestamp,
            Self::Attesting { timestamp, .. } => *timestamp,
            Self::Finalized { timestamp } => *timestamp,
            Self::Aborted { timestamp, .. } => *timestamp,
        }
    }

    /// Check if this state is terminal (no further transitions possible)
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Finalized { .. } | Self::Aborted { .. })
    }

    /// Check if this state can transition to another state
    ///
    /// Allowed transitions:
    /// - Proposed → Attesting, Finalized, Aborted
    /// - Attesting → Finalized, Aborted
    /// - Terminal states cannot transition
    pub fn can_transition_to(&self, next: &IntentState) -> bool {
        use IntentState::*;

        match (self, next) {
            // Terminal states cannot transition
            (Finalized { .. }, _) | (Aborted { .. }, _) => false,

            // Proposed can transition to anything except Proposed
            (Proposed { .. }, Proposed { .. }) => false,
            (Proposed { .. }, _) => true,

            // Attesting can transition to Finalized or Aborted
            (Attesting { .. }, Finalized { .. }) => true,
            (Attesting { .. }, Aborted { .. }) => true,
            (Attesting { .. }, _) => false,
        }
    }

    /// Merge two intent states using LWW (Last-Write-Wins) tie-breaker
    ///
    /// Takes the "greater" state according to the partial order.
    /// For concurrent states (e.g., Attesting vs Aborted), uses timestamp.
    pub fn merge(&self, other: &IntentState) -> IntentState {
        

        // If states are equal, return self
        if self == other {
            return *self;
        }

        // Use partial ordering first
        match self.partial_cmp(other) {
            Some(Ordering::Less) => *other,
            Some(Ordering::Greater) => *self,
            Some(Ordering::Equal) => *self,
            // For incomparable states, use LWW
            None => {
                if self.timestamp() >= other.timestamp() {
                    *self
                } else {
                    *other
                }
            }
        }
    }
}

impl PartialOrd for IntentState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        use IntentState::*;

        match (self, other) {
            // Same state - use timestamp for total order
            (Proposed { timestamp: t1 }, Proposed { timestamp: t2 }) => t1.partial_cmp(t2),
            (Attesting { timestamp: t1, .. }, Attesting { timestamp: t2, .. }) => {
                t1.partial_cmp(t2)
            }
            (Finalized { timestamp: t1 }, Finalized { timestamp: t2 }) => t1.partial_cmp(t2),
            (Aborted { timestamp: t1, .. }, Aborted { timestamp: t2, .. }) => t1.partial_cmp(t2),

            // Partial order: Proposed < Attesting < Finalized
            (Proposed { .. }, Attesting { .. }) => Some(Ordering::Less),
            (Proposed { .. }, Finalized { .. }) => Some(Ordering::Less),
            (Attesting { .. }, Proposed { .. }) => Some(Ordering::Greater),
            (Attesting { .. }, Finalized { .. }) => Some(Ordering::Less),
            (Finalized { .. }, Proposed { .. }) => Some(Ordering::Greater),
            (Finalized { .. }, Attesting { .. }) => Some(Ordering::Greater),

            // Partial order: Proposed < Aborted
            (Proposed { .. }, Aborted { .. }) => Some(Ordering::Less),
            (Aborted { .. }, Proposed { .. }) => Some(Ordering::Greater),

            // Attesting and Aborted are incomparable (concurrent)
            (Attesting { .. }, Aborted { .. }) => None,
            (Aborted { .. }, Attesting { .. }) => None,

            // Finalized and Aborted are incomparable (both terminal)
            (Finalized { .. }, Aborted { .. }) => None,
            (Aborted { .. }, Finalized { .. }) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_ordering() {
        let proposed = IntentState::proposed(100);
        let attesting = IntentState::attesting(200, 2);
        let finalized = IntentState::finalized(300);

        // Proposed < Attesting < Finalized
        assert!(proposed < attesting);
        assert!(attesting < finalized);
        assert!(proposed < finalized);
    }

    #[test]
    fn test_terminal_states() {
        let finalized = IntentState::finalized(100);
        let aborted = IntentState::aborted(100, 1);

        assert!(finalized.is_terminal());
        assert!(aborted.is_terminal());
        assert!(!IntentState::proposed(100).is_terminal());
        assert!(!IntentState::attesting(100, 1).is_terminal());
    }

    #[test]
    fn test_can_transition() {
        let proposed = IntentState::proposed(100);
        let attesting = IntentState::attesting(200, 2);
        let finalized = IntentState::finalized(300);
        let aborted = IntentState::aborted(300, 1);

        // Proposed can transition to anything except Proposed
        assert!(!proposed.can_transition_to(&proposed));
        assert!(proposed.can_transition_to(&attesting));
        assert!(proposed.can_transition_to(&finalized));
        assert!(proposed.can_transition_to(&aborted));

        // Attesting can only transition to terminal states
        assert!(!attesting.can_transition_to(&proposed));
        assert!(!attesting.can_transition_to(&attesting));
        assert!(attesting.can_transition_to(&finalized));
        assert!(attesting.can_transition_to(&aborted));

        // Terminal states cannot transition
        assert!(!finalized.can_transition_to(&proposed));
        assert!(!finalized.can_transition_to(&attesting));
        assert!(!finalized.can_transition_to(&aborted));
        assert!(!aborted.can_transition_to(&proposed));
        assert!(!aborted.can_transition_to(&finalized));
    }

    #[test]
    fn test_merge_ordered_states() {
        let proposed = IntentState::proposed(100);
        let attesting = IntentState::attesting(200, 2);

        // Merging ordered states takes the greater one
        assert_eq!(proposed.merge(&attesting), attesting);
        assert_eq!(attesting.merge(&proposed), attesting);
    }

    #[test]
    fn test_merge_concurrent_states_lww() {
        let attesting = IntentState::attesting(200, 2);
        let aborted = IntentState::aborted(300, 1); // Later timestamp

        // Concurrent states use LWW tie-breaker
        assert_eq!(attesting.merge(&aborted), aborted);
        assert_eq!(aborted.merge(&attesting), aborted);
    }

    #[test]
    fn test_merge_idempotent() {
        let state = IntentState::proposed(100);
        assert_eq!(state.merge(&state), state);
    }

    #[test]
    fn test_incomparable_states() {
        let attesting = IntentState::attesting(200, 2);
        let aborted = IntentState::aborted(300, 1);

        // Attesting and Aborted are incomparable
        assert_eq!(attesting.partial_cmp(&aborted), None);
        assert_eq!(aborted.partial_cmp(&attesting), None);
    }
}
