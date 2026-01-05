//! Fact propagation tracking for journal consistency metadata.
//!
//! This module provides infrastructure for tracking the propagation status
//! of facts across peers, enabling the `Propagation` field in the journal
//! consistency model (see `aura-core::domain::Propagation`).
//!
//! # Overview
//!
//! Propagation tracking occurs at two levels:
//! 1. **Aggregate**: How many peers have received our facts (via anti-entropy)
//! 2. **Per-fact**: Whether specific facts have reached all known peers
//!
//! # Integration Points
//!
//! - **Anti-entropy**: Emits `PropagationEvent::SyncedWithPeer` on peer sync completion
//! - **Fact sync**: Emits events for individual fact propagation
//! - **Journal**: Updates `Propagation` field based on received events
//!
//! # Example
//!
//! ```rust,ignore
//! use aura_sync::protocols::propagation::{PropagationTracker, PropagationEvent};
//! use aura_core::domain::Propagation;
//!
//! let mut tracker = PropagationTracker::new(known_peers.len() as u16);
//!
//! // After syncing with a peer
//! tracker.record_peer_synced(peer_id);
//!
//! // Query propagation status
//! let status = tracker.propagation_status();
//! match status {
//!     Propagation::Complete => println!("All peers synced"),
//!     Propagation::Syncing { peers_reached, peers_known } => {
//!         println!("{}/{} peers", peers_reached, peers_known);
//!     }
//!     _ => {}
//! }
//! ```

use aura_core::domain::Propagation;
use aura_core::identifiers::AuthorityId;
use aura_core::time::{OrderTime, PhysicalTime};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

// =============================================================================
// Propagation Events
// =============================================================================

/// Events emitted during fact propagation.
///
/// These events are consumed by the journal layer to update
/// the `Propagation` field on facts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PropagationEvent {
    /// A peer has been synced (aggregate propagation)
    PeerSynced {
        /// The peer that was synced
        peer_id: AuthorityId,
        /// Timestamp of the sync
        synced_at: PhysicalTime,
        /// Number of facts synced to this peer
        facts_synced: u64,
    },

    /// A specific fact has been propagated to a peer
    FactPropagated {
        /// The fact that was propagated
        fact_id: OrderTime,
        /// The peer that received the fact
        peer_id: AuthorityId,
        /// Timestamp of propagation
        propagated_at: PhysicalTime,
    },

    /// All known peers have been synced for a set of facts
    PropagationComplete {
        /// Facts that are now fully propagated
        fact_ids: Vec<OrderTime>,
        /// Timestamp when propagation completed
        completed_at: PhysicalTime,
    },

    /// Sync with a peer failed
    PeerSyncFailed {
        /// The peer that failed
        peer_id: AuthorityId,
        /// Error description
        error: String,
        /// When to retry
        retry_at: PhysicalTime,
        /// Retry count so far
        retry_count: u32,
    },
}

// =============================================================================
// Propagation Callback Trait
// =============================================================================

/// Callback trait for receiving propagation events.
///
/// Implement this trait to receive real-time propagation updates.
/// The journal layer implements this to update `Propagation` fields.
pub trait PropagationCallback: Send + Sync {
    /// Called when a propagation event occurs
    fn on_propagation(&self, event: PropagationEvent);
}

/// No-op implementation for when propagation tracking isn't needed
pub struct NoOpPropagationCallback;

impl PropagationCallback for NoOpPropagationCallback {
    fn on_propagation(&self, _event: PropagationEvent) {
        // Intentionally empty
    }
}

/// Logging implementation for debugging
pub struct LoggingPropagationCallback;

impl PropagationCallback for LoggingPropagationCallback {
    fn on_propagation(&self, event: PropagationEvent) {
        match &event {
            PropagationEvent::PeerSynced {
                peer_id,
                facts_synced,
                ..
            } => {
                tracing::debug!("Synced {} facts with peer {:?}", facts_synced, peer_id);
            }
            PropagationEvent::FactPropagated {
                fact_id, peer_id, ..
            } => {
                tracing::trace!("Fact {:?} propagated to peer {:?}", fact_id, peer_id);
            }
            PropagationEvent::PropagationComplete { fact_ids, .. } => {
                tracing::info!("Propagation complete for {} facts", fact_ids.len());
            }
            PropagationEvent::PeerSyncFailed {
                peer_id,
                error,
                retry_count,
                ..
            } => {
                tracing::warn!(
                    "Sync with peer {:?} failed: {} (retry {})",
                    peer_id,
                    error,
                    retry_count
                );
            }
        }
    }
}

// =============================================================================
// Propagation Tracker
// =============================================================================

/// Tracks propagation status for facts.
///
/// This struct maintains state about which peers have been synced,
/// enabling computation of `Propagation` status.
#[derive(Debug, Clone, Default)]
pub struct PropagationTracker {
    /// Known peers that should receive facts
    known_peers: HashSet<AuthorityId>,
    /// Peers that have been synced
    synced_peers: HashSet<AuthorityId>,
    /// Per-fact propagation status (fact_id -> set of peers that have it)
    /// Uses BTreeMap because OrderTime implements Ord but not Hash
    fact_peers: BTreeMap<OrderTime, HashSet<AuthorityId>>,
    /// Failed peers with retry info
    failed_peers: BTreeMap<AuthorityId, FailedPeerInfo>,
}

/// Information about a failed peer sync
#[derive(Debug, Clone)]
pub struct FailedPeerInfo {
    /// Last error message
    pub error: String,
    /// When to retry
    pub retry_at: PhysicalTime,
    /// Retry count
    pub retry_count: u32,
}

impl PropagationTracker {
    /// Create a new propagation tracker with the given known peers
    pub fn new(known_peers: impl IntoIterator<Item = AuthorityId>) -> Self {
        Self {
            known_peers: known_peers.into_iter().collect(),
            synced_peers: HashSet::new(),
            fact_peers: BTreeMap::new(),
            failed_peers: BTreeMap::new(),
        }
    }

    /// Add a known peer
    pub fn add_peer(&mut self, peer_id: AuthorityId) {
        self.known_peers.insert(peer_id);
    }

    /// Remove a known peer
    pub fn remove_peer(&mut self, peer_id: &AuthorityId) {
        self.known_peers.remove(peer_id);
        self.synced_peers.remove(peer_id);
        self.failed_peers.remove(peer_id);
    }

    /// Record that a peer has been synced
    pub fn record_peer_synced(&mut self, peer_id: AuthorityId) {
        self.synced_peers.insert(peer_id);
        self.failed_peers.remove(&peer_id);
    }

    /// Record that a specific fact was propagated to a peer
    pub fn record_fact_propagated(&mut self, fact_id: OrderTime, peer_id: AuthorityId) {
        self.fact_peers.entry(fact_id).or_default().insert(peer_id);
    }

    /// Record a peer sync failure
    pub fn record_peer_failed(
        &mut self,
        peer_id: AuthorityId,
        error: String,
        retry_at: PhysicalTime,
    ) {
        let info = self
            .failed_peers
            .entry(peer_id)
            .or_insert_with(|| FailedPeerInfo {
                error: String::new(),
                retry_at: PhysicalTime {
                    ts_ms: 0,
                    uncertainty: None,
                },
                retry_count: 0,
            });
        info.error = error;
        info.retry_at = retry_at;
        info.retry_count += 1;
    }

    /// Get the aggregate propagation status
    pub fn propagation_status(&self) -> Propagation {
        if self.known_peers.is_empty() {
            return Propagation::Complete;
        }

        let peers_reached = self.synced_peers.len() as u16;
        let peers_known = self.known_peers.len() as u16;

        // Check for failures
        if let Some((_peer_id, info)) = self.failed_peers.iter().next() {
            return Propagation::Failed {
                retry_at: info.retry_at.clone(),
                retry_count: info.retry_count,
                error: info.error.clone(),
            };
        }

        // Check if all known peers are synced
        if self.synced_peers.len() >= self.known_peers.len() {
            Propagation::Complete
        } else if peers_reached > 0 {
            Propagation::Syncing {
                peers_reached,
                peers_known,
            }
        } else {
            Propagation::Local
        }
    }

    /// Get the propagation status for a specific fact
    pub fn fact_propagation_status(&self, fact_id: &OrderTime) -> Propagation {
        let Some(peers) = self.fact_peers.get(fact_id) else {
            return Propagation::Local;
        };

        let peers_reached = peers.len() as u16;
        let peers_known = self.known_peers.len() as u16;

        if peers.len() >= self.known_peers.len() {
            Propagation::Complete
        } else if peers_reached > 0 {
            Propagation::Syncing {
                peers_reached,
                peers_known,
            }
        } else {
            Propagation::Local
        }
    }

    /// Get facts that have reached all known peers
    pub fn fully_propagated_facts(&self) -> Vec<OrderTime> {
        self.fact_peers
            .iter()
            .filter(|(_, peers)| peers.len() >= self.known_peers.len())
            .map(|(fact_id, _)| fact_id.clone())
            .collect()
    }

    /// Clear tracking for a fact (e.g., after it's finalized)
    pub fn clear_fact(&mut self, fact_id: &OrderTime) {
        self.fact_peers.remove(fact_id);
    }

    /// Get the number of known peers
    pub fn known_peer_count(&self) -> usize {
        self.known_peers.len()
    }

    /// Get the number of synced peers
    pub fn synced_peer_count(&self) -> usize {
        self.synced_peers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_authority(n: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([n; 32])
    }

    fn test_order_time(n: u8) -> OrderTime {
        OrderTime([n; 32])
    }

    #[test]
    fn test_propagation_tracker_empty() {
        let tracker = PropagationTracker::new(Vec::new());
        assert_eq!(tracker.propagation_status(), Propagation::Complete);
    }

    #[test]
    fn test_propagation_tracker_local() {
        let peers = vec![test_authority(1), test_authority(2), test_authority(3)];
        let tracker = PropagationTracker::new(peers);
        assert_eq!(tracker.propagation_status(), Propagation::Local);
    }

    #[test]
    fn test_propagation_tracker_syncing() {
        let peers = vec![test_authority(1), test_authority(2), test_authority(3)];
        let mut tracker = PropagationTracker::new(peers);

        tracker.record_peer_synced(test_authority(1));
        assert_eq!(
            tracker.propagation_status(),
            Propagation::Syncing {
                peers_reached: 1,
                peers_known: 3
            }
        );

        tracker.record_peer_synced(test_authority(2));
        assert_eq!(
            tracker.propagation_status(),
            Propagation::Syncing {
                peers_reached: 2,
                peers_known: 3
            }
        );
    }

    #[test]
    fn test_propagation_tracker_complete() {
        let peers = vec![test_authority(1), test_authority(2)];
        let mut tracker = PropagationTracker::new(peers);

        tracker.record_peer_synced(test_authority(1));
        tracker.record_peer_synced(test_authority(2));
        assert_eq!(tracker.propagation_status(), Propagation::Complete);
    }

    #[test]
    fn test_per_fact_propagation() {
        let peers = vec![test_authority(1), test_authority(2)];
        let mut tracker = PropagationTracker::new(peers);
        let fact_id = test_order_time(1);

        assert_eq!(
            tracker.fact_propagation_status(&fact_id),
            Propagation::Local
        );

        tracker.record_fact_propagated(fact_id.clone(), test_authority(1));
        assert_eq!(
            tracker.fact_propagation_status(&fact_id),
            Propagation::Syncing {
                peers_reached: 1,
                peers_known: 2
            }
        );

        tracker.record_fact_propagated(fact_id.clone(), test_authority(2));
        assert_eq!(
            tracker.fact_propagation_status(&fact_id),
            Propagation::Complete
        );
    }

    #[test]
    fn test_fully_propagated_facts() {
        let peers = vec![test_authority(1), test_authority(2)];
        let mut tracker = PropagationTracker::new(peers);

        let fact1 = test_order_time(1);
        let fact2 = test_order_time(2);

        // fact1 to one peer
        tracker.record_fact_propagated(fact1, test_authority(1));

        // fact2 to both peers
        tracker.record_fact_propagated(fact2.clone(), test_authority(1));
        tracker.record_fact_propagated(fact2.clone(), test_authority(2));

        let fully_propagated = tracker.fully_propagated_facts();
        assert_eq!(fully_propagated.len(), 1);
        assert!(fully_propagated.contains(&fact2));
    }
}
