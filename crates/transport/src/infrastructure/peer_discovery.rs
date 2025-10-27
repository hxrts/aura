//! Unified Peer Discovery
//!
//! Provides use-case-specific peer selection for both storage and communication.
//! Single discovery API with explicit selection criteria.
//!
//! Reference: docs/040_storage.md Section 5 "Unified Transport Architecture"

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub type PeerId = Vec<u8>;
pub type AccountId = Vec<u8>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerInfo {
    pub peer_id: PeerId,
    pub account_id: AccountId,
    pub last_seen: u64,
    pub capabilities: PeerCapabilities,
    pub metrics: PeerMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerCapabilities {
    pub storage_available: bool,
    pub storage_capacity_bytes: u64,
    pub relay_available: bool,
    pub communication_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct PeerMetrics {
    pub reliability_score: u32,
    pub average_latency_ms: u32,
    pub trust_level: TrustLevel,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustLevel {
    Unknown = 0,
    LowTrust = 1,
    MediumTrust = 2,
    HighTrust = 3,
}

#[derive(Debug, Clone)]
pub enum SelectionCriteria {
    Storage {
        min_capacity_bytes: u64,
        min_reliability: u32,
        min_trust: TrustLevel,
    },
    Communication {
        max_latency_ms: u32,
        require_online: bool,
    },
    Relay {
        min_trust: TrustLevel,
        require_high_capacity: bool,
    },
}

pub trait PeerDiscovery {
    fn discover_peers(&self, criteria: &SelectionCriteria) -> Vec<PeerInfo>;
    fn get_peer_info(&self, peer_id: &PeerId) -> Option<PeerInfo>;
    fn update_peer_metrics(&mut self, peer_id: &PeerId, metrics: PeerMetrics);
}

/// Unified peer cache combining SSB known peers with storage replica lists
#[derive(Debug, Clone)]
pub struct UnifiedPeerCache {
    peers: BTreeMap<PeerId, PeerInfo>,
    ssb_peers: BTreeSet<PeerId>,
    storage_replicas: BTreeSet<PeerId>,
}

impl UnifiedPeerCache {
    pub fn new() -> Self {
        Self {
            peers: BTreeMap::new(),
            ssb_peers: BTreeSet::new(),
            storage_replicas: BTreeSet::new(),
        }
    }

    pub fn add_peer(&mut self, peer_info: PeerInfo) {
        self.peers.insert(peer_info.peer_id.clone(), peer_info);
    }

    pub fn mark_ssb_peer(&mut self, peer_id: PeerId) {
        self.ssb_peers.insert(peer_id);
    }

    pub fn mark_storage_replica(&mut self, peer_id: PeerId) {
        self.storage_replicas.insert(peer_id);
    }

    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.peers.remove(peer_id);
        self.ssb_peers.remove(peer_id);
        self.storage_replicas.remove(peer_id);
    }

    pub fn get_ssb_peers(&self) -> Vec<PeerInfo> {
        self.ssb_peers
            .iter()
            .filter_map(|id| self.peers.get(id).cloned())
            .collect()
    }

    pub fn get_storage_replicas(&self) -> Vec<PeerInfo> {
        self.storage_replicas
            .iter()
            .filter_map(|id| self.peers.get(id).cloned())
            .collect()
    }

    fn select_storage_peers(&self, criteria: &SelectionCriteria) -> Vec<PeerInfo> {
        if let SelectionCriteria::Storage {
            min_capacity_bytes,
            min_reliability,
            min_trust,
        } = criteria
        {
            let mut candidates: Vec<PeerInfo> = self
                .peers
                .values()
                .filter(|p| {
                    p.capabilities.storage_available
                        && p.capabilities.storage_capacity_bytes >= *min_capacity_bytes
                        && p.metrics.reliability_score >= *min_reliability
                        && p.metrics.trust_level >= *min_trust
                })
                .cloned()
                .collect();

            candidates.sort_by(|a, b| {
                b.metrics
                    .reliability_score
                    .cmp(&a.metrics.reliability_score)
                    .then(b.metrics.trust_level.cmp(&a.metrics.trust_level))
                    .then(
                        b.capabilities
                            .storage_capacity_bytes
                            .cmp(&a.capabilities.storage_capacity_bytes),
                    )
            });

            candidates
        } else {
            vec![]
        }
    }

    fn select_communication_peers(&self, criteria: &SelectionCriteria) -> Vec<PeerInfo> {
        if let SelectionCriteria::Communication {
            max_latency_ms,
            require_online,
        } = criteria
        {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let mut candidates: Vec<PeerInfo> = self
                .peers
                .values()
                .filter(|p| {
                    p.capabilities.communication_available
                        && p.metrics.average_latency_ms <= *max_latency_ms
                        && (!require_online || (now - p.last_seen) < 300)
                })
                .cloned()
                .collect();

            candidates.sort_by(|a, b| {
                a.metrics
                    .average_latency_ms
                    .cmp(&b.metrics.average_latency_ms)
                    .then(b.metrics.trust_level.cmp(&a.metrics.trust_level))
            });

            candidates
        } else {
            vec![]
        }
    }

    fn select_relay_peers(&self, criteria: &SelectionCriteria) -> Vec<PeerInfo> {
        if let SelectionCriteria::Relay {
            min_trust,
            require_high_capacity,
        } = criteria
        {
            let mut candidates: Vec<PeerInfo> = self
                .peers
                .values()
                .filter(|p| {
                    p.capabilities.relay_available
                        && p.metrics.trust_level >= *min_trust
                        && (!require_high_capacity
                            || p.capabilities.storage_capacity_bytes > 1_000_000_000)
                })
                .cloned()
                .collect();

            candidates.sort_by(|a, b| {
                b.metrics.trust_level.cmp(&a.metrics.trust_level).then(
                    b.metrics
                        .reliability_score
                        .cmp(&a.metrics.reliability_score),
                )
            });

            candidates
        } else {
            vec![]
        }
    }
}

impl PeerDiscovery for UnifiedPeerCache {
    fn discover_peers(&self, criteria: &SelectionCriteria) -> Vec<PeerInfo> {
        match criteria {
            SelectionCriteria::Storage { .. } => self.select_storage_peers(criteria),
            SelectionCriteria::Communication { .. } => self.select_communication_peers(criteria),
            SelectionCriteria::Relay { .. } => self.select_relay_peers(criteria),
        }
    }

    fn get_peer_info(&self, peer_id: &PeerId) -> Option<PeerInfo> {
        self.peers.get(peer_id).cloned()
    }

    fn update_peer_metrics(&mut self, peer_id: &PeerId, metrics: PeerMetrics) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.metrics = metrics;
        }
    }
}

impl Default for UnifiedPeerCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_peer(
        id: u8,
        storage_bytes: u64,
        reliability: u32,
        trust: TrustLevel,
    ) -> PeerInfo {
        PeerInfo {
            peer_id: vec![id],
            account_id: vec![id],
            last_seen: 1000,
            capabilities: PeerCapabilities {
                storage_available: storage_bytes > 0,
                storage_capacity_bytes: storage_bytes,
                relay_available: trust >= TrustLevel::MediumTrust,
                communication_available: true,
            },
            metrics: PeerMetrics {
                reliability_score: reliability,
                average_latency_ms: 50,
                trust_level: trust,
            },
        }
    }

    #[test]
    fn test_unified_cache_creation() {
        let cache = UnifiedPeerCache::new();
        assert_eq!(cache.peers.len(), 0);
        assert_eq!(cache.ssb_peers.len(), 0);
        assert_eq!(cache.storage_replicas.len(), 0);
    }

    #[test]
    fn test_add_and_retrieve_peer() {
        let mut cache = UnifiedPeerCache::new();
        let peer = create_test_peer(1, 1_000_000, 80, TrustLevel::HighTrust);

        cache.add_peer(peer.clone());

        let retrieved = cache.get_peer_info(&vec![1]);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().peer_id, vec![1]);
    }

    #[test]
    fn test_ssb_peer_marking() {
        let mut cache = UnifiedPeerCache::new();
        let peer = create_test_peer(1, 1_000_000, 80, TrustLevel::HighTrust);

        cache.add_peer(peer);
        cache.mark_ssb_peer(vec![1]);

        let ssb_peers = cache.get_ssb_peers();
        assert_eq!(ssb_peers.len(), 1);
        assert_eq!(ssb_peers[0].peer_id, vec![1]);
    }

    #[test]
    fn test_storage_replica_marking() {
        let mut cache = UnifiedPeerCache::new();
        let peer = create_test_peer(1, 1_000_000, 80, TrustLevel::HighTrust);

        cache.add_peer(peer);
        cache.mark_storage_replica(vec![1]);

        let replicas = cache.get_storage_replicas();
        assert_eq!(replicas.len(), 1);
        assert_eq!(replicas[0].peer_id, vec![1]);
    }

    #[test]
    fn test_storage_peer_selection() {
        let mut cache = UnifiedPeerCache::new();

        cache.add_peer(create_test_peer(1, 10_000_000, 90, TrustLevel::HighTrust));
        cache.add_peer(create_test_peer(2, 5_000_000, 70, TrustLevel::MediumTrust));
        cache.add_peer(create_test_peer(3, 1_000_000, 50, TrustLevel::LowTrust));

        let criteria = SelectionCriteria::Storage {
            min_capacity_bytes: 5_000_000,
            min_reliability: 70,
            min_trust: TrustLevel::MediumTrust,
        };

        let selected = cache.discover_peers(&criteria);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].peer_id, vec![1]);
        assert_eq!(selected[1].peer_id, vec![2]);
    }

    #[test]
    fn test_communication_peer_selection() {
        let mut cache = UnifiedPeerCache::new();

        let mut peer1 = create_test_peer(1, 0, 90, TrustLevel::HighTrust);
        peer1.metrics.average_latency_ms = 20;

        let mut peer2 = create_test_peer(2, 0, 80, TrustLevel::MediumTrust);
        peer2.metrics.average_latency_ms = 100;

        cache.add_peer(peer1);
        cache.add_peer(peer2);

        let criteria = SelectionCriteria::Communication {
            max_latency_ms: 50,
            require_online: false,
        };

        let selected = cache.discover_peers(&criteria);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].peer_id, vec![1]);
    }

    #[test]
    fn test_relay_peer_selection() {
        let mut cache = UnifiedPeerCache::new();

        cache.add_peer(create_test_peer(
            1,
            2_000_000_000,
            90,
            TrustLevel::HighTrust,
        ));
        cache.add_peer(create_test_peer(
            2,
            500_000_000,
            80,
            TrustLevel::MediumTrust,
        ));
        cache.add_peer(create_test_peer(3, 100_000_000, 70, TrustLevel::LowTrust));

        let criteria = SelectionCriteria::Relay {
            min_trust: TrustLevel::MediumTrust,
            require_high_capacity: true,
        };

        let selected = cache.discover_peers(&criteria);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].peer_id, vec![1]);
    }

    #[test]
    fn test_peer_removal() {
        let mut cache = UnifiedPeerCache::new();
        let peer = create_test_peer(1, 1_000_000, 80, TrustLevel::HighTrust);

        cache.add_peer(peer);
        cache.mark_ssb_peer(vec![1]);
        cache.mark_storage_replica(vec![1]);

        cache.remove_peer(&vec![1]);

        assert_eq!(cache.peers.len(), 0);
        assert_eq!(cache.ssb_peers.len(), 0);
        assert_eq!(cache.storage_replicas.len(), 0);
    }

    #[test]
    fn test_update_peer_metrics() {
        let mut cache = UnifiedPeerCache::new();
        let peer = create_test_peer(1, 1_000_000, 80, TrustLevel::HighTrust);

        cache.add_peer(peer);

        let new_metrics = PeerMetrics {
            reliability_score: 95,
            average_latency_ms: 25,
            trust_level: TrustLevel::HighTrust,
        };

        cache.update_peer_metrics(&vec![1], new_metrics.clone());

        let updated = cache.get_peer_info(&vec![1]).unwrap();
        assert_eq!(updated.metrics, new_metrics);
    }

    #[test]
    fn test_deterministic_selection() {
        let mut cache = UnifiedPeerCache::new();

        cache.add_peer(create_test_peer(1, 10_000_000, 90, TrustLevel::HighTrust));
        cache.add_peer(create_test_peer(2, 10_000_000, 90, TrustLevel::HighTrust));

        let criteria = SelectionCriteria::Storage {
            min_capacity_bytes: 5_000_000,
            min_reliability: 80,
            min_trust: TrustLevel::MediumTrust,
        };

        let selected1 = cache.discover_peers(&criteria);
        let selected2 = cache.discover_peers(&criteria);

        assert_eq!(selected1, selected2);
    }
}
