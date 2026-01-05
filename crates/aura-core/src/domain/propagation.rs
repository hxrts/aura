//! Propagation Status Types
//!
//! This module tracks anti-entropy sync status for facts. Propagation
//! is orthogonal to Agreement - a fact can be:
//!
//! - `Propagation::Local` but `Agreement::Finalized` (consensus done, not synced)
//! - `Propagation::Complete` but `Agreement::Provisional` (synced optimistically)
//!
//! Propagation answers: "Has this fact reached peers via gossip/sync?"
//! Agreement answers: "Is this fact durably agreed upon?"

use crate::time::PhysicalTime;
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Propagation Status
// ─────────────────────────────────────────────────────────────────────────────

/// Anti-entropy sync status for facts.
///
/// Tracks whether facts have propagated to peers via gossip/sync protocols.
/// This is distinct from Acknowledgment, which tracks explicit per-peer
/// delivery confirmation.
///
/// | Aspect | Propagation | Acknowledgment |
/// |--------|-------------|----------------|
/// | What it tracks | Gossip sync reached peers | Peer explicitly confirmed |
/// | How it's known | Transport layer observes | Requires ack protocol |
/// | Granularity | Aggregate (count) | Per-peer with timestamp |
/// | Opt-in | Always available | Fact must request acks |
/// | Use case | "Is sync complete?" | "Did Alice receive this?" |
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Propagation {
    /// Only on this device.
    ///
    /// The fact has been written locally but has not yet been synced
    /// to any peers. This is the initial state for all new facts.
    Local,

    /// Sync in progress.
    ///
    /// The fact is being propagated to peers. The counts indicate
    /// progress toward complete propagation.
    Syncing {
        /// Number of peers that have received this fact
        peers_reached: u16,
        /// Total number of known peers to sync with
        peers_known: u16,
    },

    /// Reached all known peers.
    ///
    /// The fact has been synced to all peers known at the time of
    /// checking. Note that new peers may join and not have this fact.
    Complete,

    /// Sync failed, will retry.
    ///
    /// The sync encountered an error and will be retried. The retry
    /// information helps track sync health and debugging.
    Failed {
        /// When the next retry is scheduled
        retry_at: PhysicalTime,
        /// Number of retry attempts so far
        retry_count: u32,
        /// Description of the failure
        error: String,
    },
}

impl Default for Propagation {
    fn default() -> Self {
        Self::Local
    }
}

impl Propagation {
    /// Create a local propagation status
    pub fn local() -> Self {
        Self::Local
    }

    /// Create a syncing status
    pub fn syncing(peers_reached: u16, peers_known: u16) -> Self {
        Self::Syncing {
            peers_reached,
            peers_known,
        }
    }

    /// Create a complete propagation status
    pub fn complete() -> Self {
        Self::Complete
    }

    /// Create a failed propagation status
    pub fn failed(retry_at: PhysicalTime, retry_count: u32, error: impl Into<String>) -> Self {
        Self::Failed {
            retry_at,
            retry_count,
            error: error.into(),
        }
    }

    /// Check if propagation is complete
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }

    /// Check if propagation is local (not yet synced)
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local)
    }

    /// Check if sync is in progress
    pub fn is_syncing(&self) -> bool {
        matches!(self, Self::Syncing { .. })
    }

    /// Check if sync has failed
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    /// Get sync progress as a percentage (0.0 to 1.0)
    ///
    /// Returns:
    /// - 0.0 for Local
    /// - Progress ratio for Syncing
    /// - 1.0 for Complete
    /// - Previous progress for Failed (based on retry count heuristic)
    pub fn progress(&self) -> f64 {
        match self {
            Self::Local => 0.0,
            Self::Syncing {
                peers_reached,
                peers_known,
            } => {
                if *peers_known == 0 {
                    0.0
                } else {
                    *peers_reached as f64 / *peers_known as f64
                }
            }
            Self::Complete => 1.0,
            Self::Failed { .. } => 0.0, // Failed doesn't indicate progress
        }
    }

    /// Get the number of peers reached (if applicable)
    pub fn peers_reached(&self) -> Option<u16> {
        match self {
            Self::Syncing { peers_reached, .. } => Some(*peers_reached),
            Self::Complete => None, // All peers reached, but count not tracked
            _ => None,
        }
    }

    /// Get the number of known peers (if applicable)
    pub fn peers_known(&self) -> Option<u16> {
        match self {
            Self::Syncing { peers_known, .. } => Some(*peers_known),
            _ => None,
        }
    }

    /// Get retry information (if failed)
    pub fn retry_info(&self) -> Option<(PhysicalTime, u32, &str)> {
        match self {
            Self::Failed {
                retry_at,
                retry_count,
                error,
            } => Some((retry_at.clone(), *retry_count, error)),
            _ => None,
        }
    }

    /// Update syncing progress
    ///
    /// If more peers are reached, transitions to Complete when all are reached.
    pub fn update_progress(self, new_reached: u16, new_known: u16) -> Self {
        if new_reached >= new_known && new_known > 0 {
            Self::Complete
        } else {
            Self::Syncing {
                peers_reached: new_reached,
                peers_known: new_known,
            }
        }
    }
}

impl std::fmt::Display for Propagation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local => write!(f, "Local"),
            Self::Syncing {
                peers_reached,
                peers_known,
            } => {
                write!(f, "Syncing({peers_reached}/{peers_known})")
            }
            Self::Complete => write!(f, "Complete"),
            Self::Failed {
                retry_count, error, ..
            } => {
                write!(f, "Failed(retry={retry_count}, {error})")
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_time(millis: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms: millis,
            uncertainty: None,
        }
    }

    #[test]
    fn test_propagation_local() {
        let prop = Propagation::local();
        assert!(prop.is_local());
        assert!(!prop.is_complete());
        assert!(!prop.is_syncing());
        assert!(!prop.is_failed());
        assert_eq!(prop.progress(), 0.0);
    }

    #[test]
    fn test_propagation_syncing() {
        let prop = Propagation::syncing(3, 10);
        assert!(!prop.is_local());
        assert!(!prop.is_complete());
        assert!(prop.is_syncing());
        assert_eq!(prop.peers_reached(), Some(3));
        assert_eq!(prop.peers_known(), Some(10));
        assert!((prop.progress() - 0.3).abs() < 0.001);
    }

    #[test]
    fn test_propagation_complete() {
        let prop = Propagation::complete();
        assert!(!prop.is_local());
        assert!(prop.is_complete());
        assert!(!prop.is_syncing());
        assert_eq!(prop.progress(), 1.0);
    }

    #[test]
    fn test_propagation_failed() {
        let retry_at = test_time(5000);
        let prop = Propagation::failed(retry_at.clone(), 3, "network error");

        assert!(prop.is_failed());
        assert!(!prop.is_complete());

        let (rt, count, err) = prop.retry_info().unwrap();
        assert_eq!(rt, retry_at);
        assert_eq!(count, 3);
        assert_eq!(err, "network error");
    }

    #[test]
    fn test_propagation_update_progress() {
        let prop = Propagation::syncing(2, 5);

        // Update to more progress
        let prop2 = prop.update_progress(4, 5);
        assert!(prop2.is_syncing());
        assert_eq!(prop2.peers_reached(), Some(4));

        // Update to complete
        let prop3 = prop2.update_progress(5, 5);
        assert!(prop3.is_complete());
    }

    #[test]
    fn test_propagation_display() {
        assert_eq!(Propagation::local().to_string(), "Local");
        assert_eq!(Propagation::syncing(2, 5).to_string(), "Syncing(2/5)");
        assert_eq!(Propagation::complete().to_string(), "Complete");
        assert!(Propagation::failed(test_time(0), 1, "err")
            .to_string()
            .contains("Failed"));
    }

    #[test]
    fn test_propagation_syncing_zero_peers() {
        let prop = Propagation::syncing(0, 0);
        assert_eq!(prop.progress(), 0.0);
    }
}
