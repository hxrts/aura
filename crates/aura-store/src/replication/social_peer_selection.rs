//! Trust-Based Peer Selection
//!
//! Implements peer selection logic using trust history and reputation scores.
//! Selects peers for storage replica placement based on:
//! - Historical interaction success rates
//! - Proof-of-storage verification history
//! - Relationship trust level
//! - Current availability
//!
//! Reference: work/ssb_storage.md Phase 6.1

use super::social_storage::{StorageCapabilityAnnouncement, TrustLevel};
use super::social_trust_scoring::TrustScore;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};

pub type PeerId = Vec<u8>;

/// Peer candidate for storage replica placement
#[derive(Debug, Clone)]
struct PeerCandidate {
    peer_id: PeerId,
    trust_score: f64,
    availability: u64,
}

impl PartialEq for PeerCandidate {
    fn eq(&self, other: &Self) -> bool {
        self.peer_id == other.peer_id
    }
}

impl Eq for PeerCandidate {}

impl PartialOrd for PeerCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PeerCandidate {
    fn cmp(&self, other: &Self) -> Ordering {
        // Sort by trust score (descending), then by availability (descending)
        other
            .trust_score
            .partial_cmp(&self.trust_score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| other.availability.cmp(&self.availability))
    }
}

/// Peer selector for trust-based replica placement
///
/// Maintains peer profiles and selects best candidates for storing replicas
/// based on historical performance and current availability.
pub struct PeerSelector {
    /// Known peers and their trust profiles
    peers: BTreeMap<PeerId, PeerProfile>,

    /// Minimum trust level required for selecting peers
    min_trust_level: TrustLevel,

    /// Number of replicas to maintain
    replica_count: usize,
}

/// Profile of a peer for selection purposes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerProfile {
    /// Peer's ID
    pub peer_id: PeerId,

    /// Trust score history
    pub trust_score: TrustScore,

    /// Current storage capability
    pub capability: StorageCapabilityAnnouncement,

    /// Last updated timestamp
    pub last_updated: u64,
}

impl PeerSelector {
    /// Create a new peer selector
    pub fn new(min_trust_level: TrustLevel, replica_count: usize) -> Self {
        Self {
            peers: BTreeMap::new(),
            min_trust_level,
            replica_count,
        }
    }

    /// Register or update a peer profile
    pub fn register_peer(
        &mut self,
        peer_id: PeerId,
        trust_score: TrustScore,
        capability: StorageCapabilityAnnouncement,
        timestamp: u64,
    ) {
        let profile = PeerProfile {
            peer_id,
            trust_score,
            capability,
            last_updated: timestamp,
        };
        self.peers.insert(profile.peer_id.clone(), profile);
    }

    /// Select best peers for replica placement
    pub fn select_peers(&self, chunk_size: u32) -> Vec<PeerId> {
        let mut candidates = BinaryHeap::new();

        // Filter peers by requirements and convert to candidates
        for (peer_id, profile) in self.peers.iter() {
            // Check trust level requirement
            if profile.trust_score.reliability_score < self.min_trust_level as u32 as f64 / 10.0 {
                continue;
            }

            // Check availability
            if !profile.capability.is_available() {
                continue;
            }

            // Check chunk size acceptance
            if !profile.capability.can_accept_chunk(chunk_size) {
                continue;
            }

            // Create candidate with composite score
            let candidate = PeerCandidate {
                peer_id: peer_id.clone(),
                trust_score: profile.trust_score.reliability_score,
                availability: profile.capability.remaining_capacity(),
            };

            candidates.push(candidate);
        }

        // Extract top N candidates
        let mut selected = Vec::new();
        while selected.len() < self.replica_count && !candidates.is_empty() {
            if let Some(candidate) = candidates.pop() {
                selected.push(candidate.peer_id);
            }
        }

        selected
    }

    /// Update peer trust score based on verification result
    pub fn update_peer_trust(
        &mut self,
        peer_id: &PeerId,
        success: bool,
        timestamp: u64,
    ) -> Result<(), String> {
        if let Some(profile) = self.peers.get_mut(peer_id) {
            if success {
                profile.trust_score.record_success(timestamp);
            } else {
                profile.trust_score.record_failure(timestamp);
            }
            profile.last_updated = timestamp;
            Ok(())
        } else {
            Err(format!("Peer not found: {:?}", peer_id))
        }
    }

    /// Get peer profile
    pub fn get_peer(&self, peer_id: &PeerId) -> Option<&PeerProfile> {
        self.peers.get(peer_id)
    }

    /// Get all peers
    pub fn all_peers(&self) -> Vec<&PeerProfile> {
        self.peers.values().collect()
    }

    /// Get number of available peers
    pub fn available_peers(&self) -> usize {
        self.peers
            .values()
            .filter(|p| p.capability.is_available())
            .count()
    }

    /// Set minimum trust level requirement
    pub fn set_min_trust_level(&mut self, level: TrustLevel) {
        self.min_trust_level = level;
    }

    /// Set number of replicas to maintain
    pub fn set_replica_count(&mut self, count: usize) {
        self.replica_count = count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_peer(id: u8) -> PeerProfile {
        let peer_id = vec![id; 32];
        PeerProfile {
            peer_id,
            trust_score: TrustScore::new(1000),
            capability: StorageCapabilityAnnouncement::new(
                1024 * 1024 * 1024,
                TrustLevel::Medium,
                4 * 1024 * 1024,
            ),
            last_updated: 1000,
        }
    }

    #[test]
    fn test_peer_selector_creation() {
        let selector = PeerSelector::new(TrustLevel::Medium, 3);
        assert_eq!(selector.replica_count, 3);
    }

    #[test]
    fn test_register_peer() {
        let mut selector = PeerSelector::new(TrustLevel::Medium, 3);
        let peer = create_test_peer(1);

        selector.register_peer(
            peer.peer_id.clone(),
            peer.trust_score.clone(),
            peer.capability,
            1000,
        );

        assert!(selector.get_peer(&peer.peer_id).is_some());
    }

    #[test]
    fn test_select_peers() {
        let mut selector = PeerSelector::new(TrustLevel::Low, 3);

        // Register 5 peers
        for i in 1..=5 {
            let peer = create_test_peer(i);
            selector.register_peer(
                peer.peer_id.clone(),
                peer.trust_score.clone(),
                peer.capability,
                1000,
            );
        }

        let selected = selector.select_peers(1024 * 1024);
        assert!(selected.len() <= 3);
    }

    #[test]
    fn test_update_peer_trust() {
        let mut selector = PeerSelector::new(TrustLevel::Low, 3);
        let peer = create_test_peer(1);

        selector.register_peer(
            peer.peer_id.clone(),
            peer.trust_score.clone(),
            peer.capability,
            1000,
        );

        selector
            .update_peer_trust(&peer.peer_id, true, 2000)
            .unwrap();

        let updated = selector.get_peer(&peer.peer_id).unwrap();
        assert!(updated.trust_score.reliability_score > 0.0);
    }

    #[test]
    fn test_available_peers() {
        let mut selector = PeerSelector::new(TrustLevel::Low, 3);

        for i in 1..=3 {
            let peer = create_test_peer(i);
            selector.register_peer(
                peer.peer_id.clone(),
                peer.trust_score.clone(),
                peer.capability,
                1000,
            );
        }

        assert_eq!(selector.available_peers(), 3);
    }
}
