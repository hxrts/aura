//! Static Replication Strategy
//!
//! Replication to a predetermined set of peers, useful for:
//! - Predictable replication topology
//! - High-trust environments where peer lists are pre-agreed
//! - Bootstrap replication before social graph is established
//! - System accounts or important data requiring specific guardians
//!
//! Unlike social replication which dynamically selects peers based on trust,
//! static replication uses a fixed peer list that must be explicitly configured.

use std::collections::{HashMap, HashSet};

/// Configuration for static replication strategy
#[derive(Clone, Debug)]
pub struct StaticReplicationConfig {
    /// Fixed set of peer IDs that will always receive replicas
    pub peer_ids: HashSet<String>,
    /// Required number of replicas (must be <= peer_ids.len())
    pub replication_factor: usize,
    /// Optional priority ordering for replica selection (higher priority peers first)
    pub peer_priorities: HashMap<String, u32>,
}

/// Result of replica placement
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplicaPlacement {
    /// Peer IDs selected to receive replicas
    pub replica_peers: Vec<String>,
    /// Whether all required replicas could be placed
    pub is_complete: bool,
    /// Number of replicas actually placed
    pub replica_count: usize,
}

impl StaticReplicationConfig {
    /// Create a new static replication config
    pub fn new(peer_ids: HashSet<String>, replication_factor: usize) -> Result<Self, String> {
        if replication_factor == 0 {
            return Err("replication_factor must be > 0".to_string());
        }
        if replication_factor > peer_ids.len() {
            return Err(format!(
                "replication_factor {} exceeds available peers {}",
                replication_factor,
                peer_ids.len()
            ));
        }
        Ok(Self {
            peer_ids,
            replication_factor,
            peer_priorities: HashMap::new(),
        })
    }

    /// Create with default priority (higher lexicographical = higher priority)
    pub fn with_default_priorities(mut self) -> Self {
        for peer in &self.peer_ids {
            self.peer_priorities
                .insert(peer.clone(), (peer.len() as u32) * 1000);
        }
        self
    }

    /// Set priority for a specific peer
    pub fn set_peer_priority(mut self, peer_id: String, priority: u32) -> Self {
        self.peer_priorities.insert(peer_id, priority);
        self
    }

    /// Place replicas according to static configuration
    pub fn place_replicas(&self) -> ReplicaPlacement {
        let mut peers_with_priority: Vec<(String, u32)> = self
            .peer_ids
            .iter()
            .map(|peer| {
                let priority = self.peer_priorities.get(peer).copied().unwrap_or(0);
                (peer.clone(), priority)
            })
            .collect();

        // Sort by priority (descending)
        peers_with_priority.sort_by(|a, b| b.1.cmp(&a.1));

        let replica_peers: Vec<String> = peers_with_priority
            .iter()
            .take(self.replication_factor)
            .map(|(peer, _)| peer.clone())
            .collect();

        let replica_count = replica_peers.len();
        let is_complete = replica_count == self.replication_factor;

        ReplicaPlacement {
            replica_peers,
            is_complete,
            replica_count,
        }
    }

    /// Check if a peer is in the static peer set
    pub fn contains_peer(&self, peer_id: &str) -> bool {
        self.peer_ids.contains(peer_id)
    }

    /// Update the set of static peers
    pub fn update_peers(&mut self, new_peers: HashSet<String>) -> Result<(), String> {
        if new_peers.len() < self.replication_factor {
            return Err(format!(
                "new peer set size {} is less than replication factor {}",
                new_peers.len(),
                self.replication_factor
            ));
        }
        self.peer_ids = new_peers;
        Ok(())
    }

    /// Get the priority of a peer (higher = more likely to be selected)
    pub fn get_peer_priority(&self, peer_id: &str) -> u32 {
        self.peer_priorities.get(peer_id).copied().unwrap_or(0)
    }
}

/// Static replicator for managing predetermined replica placement
pub struct StaticReplicator {
    config: StaticReplicationConfig,
    placement_history: Vec<ReplicaPlacement>,
}

impl StaticReplicator {
    /// Create a new static replicator
    pub fn new(config: StaticReplicationConfig) -> Self {
        Self {
            config,
            placement_history: Vec::new(),
        }
    }

    /// Get current configuration
    pub fn config(&self) -> &StaticReplicationConfig {
        &self.config
    }

    /// Update configuration
    pub fn update_config(&mut self, config: StaticReplicationConfig) -> Result<(), String> {
        self.config = config;
        Ok(())
    }

    /// Place a replica for a content ID
    pub fn place_replica_for(&mut self, _content_id: &str) -> ReplicaPlacement {
        let placement = self.config.place_replicas();
        self.placement_history.push(placement.clone());
        placement
    }

    /// Get placement history (last 10 placements)
    pub fn get_placement_history(&self) -> Vec<ReplicaPlacement> {
        self.placement_history
            .iter()
            .rev()
            .take(10)
            .cloned()
            .collect()
    }

    /// Check if a peer is eligible for replication
    pub fn is_peer_eligible(&self, peer_id: &str) -> bool {
        self.config.contains_peer(peer_id)
    }

    /// Get all eligible peers
    pub fn get_eligible_peers(&self) -> Vec<String> {
        let mut peers: Vec<_> = self.config.peer_ids.iter().cloned().collect();
        peers.sort();
        peers
    }

    /// Get the preferred replicas (those with highest priority)
    pub fn get_preferred_replicas(&self) -> Vec<String> {
        self.config.place_replicas().replica_peers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_config_creation() {
        let peers = vec![
            "peer1".to_string(),
            "peer2".to_string(),
            "peer3".to_string(),
        ]
        .into_iter()
        .collect();
        let config = StaticReplicationConfig::new(peers, 2).unwrap();

        assert_eq!(config.replication_factor, 2);
        assert_eq!(config.peer_ids.len(), 3);
    }

    #[test]
    fn test_invalid_replication_factor_zero() {
        let peers = vec!["peer1".to_string(), "peer2".to_string()]
            .into_iter()
            .collect();
        let result = StaticReplicationConfig::new(peers, 0);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must be > 0"));
    }

    #[test]
    fn test_invalid_replication_factor_exceeds_peers() {
        let peers = vec!["peer1".to_string(), "peer2".to_string()]
            .into_iter()
            .collect();
        let result = StaticReplicationConfig::new(peers, 5);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exceeds available peers"));
    }

    #[test]
    fn test_place_replicas_respects_count() {
        let peers = vec![
            "peer1".to_string(),
            "peer2".to_string(),
            "peer3".to_string(),
            "peer4".to_string(),
        ]
        .into_iter()
        .collect();
        let config = StaticReplicationConfig::new(peers, 3).unwrap();

        let placement = config.place_replicas();
        assert_eq!(placement.replica_count, 3);
        assert!(placement.is_complete);
        assert_eq!(placement.replica_peers.len(), 3);
    }

    #[test]
    fn test_peer_priorities_affect_placement() {
        let peers = vec![
            "alice".to_string(),
            "bob".to_string(),
            "charlie".to_string(),
        ]
        .into_iter()
        .collect();
        let config = StaticReplicationConfig::new(peers, 2)
            .unwrap()
            .set_peer_priority("charlie".to_string(), 100)
            .set_peer_priority("bob".to_string(), 50)
            .set_peer_priority("alice".to_string(), 10);

        let placement = config.place_replicas();
        assert_eq!(placement.replica_peers[0], "charlie");
        assert_eq!(placement.replica_peers[1], "bob");
    }

    #[test]
    fn test_contains_peer() {
        let peers = vec!["peer1".to_string(), "peer2".to_string()]
            .into_iter()
            .collect();
        let config = StaticReplicationConfig::new(peers, 1).unwrap();

        assert!(config.contains_peer("peer1"));
        assert!(config.contains_peer("peer2"));
        assert!(!config.contains_peer("peer3"));
    }

    #[test]
    fn test_static_replicator_placement_history() {
        let peers = vec![
            "peer1".to_string(),
            "peer2".to_string(),
            "peer3".to_string(),
        ]
        .into_iter()
        .collect();
        let config = StaticReplicationConfig::new(peers, 2).unwrap();
        let mut replicator = StaticReplicator::new(config);

        for i in 0..5 {
            replicator.place_replica_for(&format!("content_{}", i));
        }

        let history = replicator.get_placement_history();
        assert_eq!(history.len(), 5);
    }

    #[test]
    fn test_static_replicator_eligible_peers() {
        let peers = vec!["peer1".to_string(), "peer2".to_string()]
            .into_iter()
            .collect();
        let config = StaticReplicationConfig::new(peers, 1).unwrap();
        let replicator = StaticReplicator::new(config);

        assert!(replicator.is_peer_eligible("peer1"));
        assert!(replicator.is_peer_eligible("peer2"));
        assert!(!replicator.is_peer_eligible("peer3"));

        let peers = replicator.get_eligible_peers();
        assert_eq!(peers.len(), 2);
    }

    #[test]
    fn test_update_peers_validates_replication_factor() {
        let peers = vec![
            "peer1".to_string(),
            "peer2".to_string(),
            "peer3".to_string(),
        ]
        .into_iter()
        .collect();
        let mut config = StaticReplicationConfig::new(peers, 3).unwrap();

        let new_peers = vec!["peer1".to_string(), "peer2".to_string()]
            .into_iter()
            .collect();
        let result = config.update_peers(new_peers);

        assert!(result.is_err());
    }
}
