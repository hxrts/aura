//! Replication and erasure coding coordination
//!
//! This module provides coordination for replication strategies,
//! erasure coding, and distributed storage reliability.

use aura_core::{AuraResult, ChunkId, DeviceId};
use aura_store::{ChunkLayout, ErasureConfig};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Replication coordinator for managing distributed storage
pub struct ReplicationCoordinator {
    /// Device ID for this coordinator
    device_id: DeviceId,
    /// Replication strategy
    strategy: ReplicationStrategy,
    /// Known storage nodes
    storage_nodes: HashMap<DeviceId, StorageNodeInfo>,
    /// Chunk placement tracking
    chunk_placement: HashMap<ChunkId, HashSet<DeviceId>>,
}

/// Replication strategy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplicationStrategy {
    /// Simple replication to N nodes
    SimpleReplication {
        /// Number of replicas
        replica_count: usize,
    },
    /// Erasure coding with data and parity chunks
    ErasureCoding {
        /// Erasure coding configuration
        config: ErasureConfig,
    },
    /// Hybrid strategy with both replication and erasure coding
    Hybrid {
        /// Critical data replication count
        critical_replicas: usize,
        /// Non-critical data erasure config
        erasure_config: ErasureConfig,
    },
}

impl Default for ReplicationStrategy {
    fn default() -> Self {
        Self::SimpleReplication { replica_count: 3 }
    }
}

/// Information about storage nodes in the network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageNodeInfo {
    /// Node device ID
    pub device_id: DeviceId,
    /// Available storage capacity in bytes
    pub available_capacity: u64,
    /// Current utilization (0.0 to 1.0)
    pub utilization: f64,
    /// Node reliability score (0.0 to 1.0)
    pub reliability_score: f64,
    /// Last seen timestamp
    pub last_seen: u64,
    /// Supported storage features
    pub features: HashSet<String>,
}

impl StorageNodeInfo {
    /// Create new storage node info
    pub fn new(device_id: DeviceId, available_capacity: u64) -> Self {
        Self {
            device_id,
            available_capacity,
            utilization: 0.0,
            reliability_score: 1.0,
            last_seen: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            features: HashSet::new(),
        }
    }

    /// Calculate node score for placement decisions
    pub fn placement_score(&self) -> f64 {
        // Combine reliability and utilization (prefer reliable, underutilized nodes)
        self.reliability_score * (1.0 - self.utilization)
    }

    /// Check if node is healthy and available
    pub fn is_healthy(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Node is healthy if seen in last 5 minutes and has reasonable reliability
        (now - self.last_seen) < 300 && self.reliability_score > 0.5
    }
}

impl ReplicationCoordinator {
    /// Create new replication coordinator
    pub fn new(device_id: DeviceId, strategy: ReplicationStrategy) -> Self {
        Self {
            device_id,
            strategy,
            storage_nodes: HashMap::new(),
            chunk_placement: HashMap::new(),
        }
    }

    /// Register a storage node
    pub fn register_node(&mut self, node_info: StorageNodeInfo) {
        self.storage_nodes.insert(node_info.device_id, node_info);
    }

    /// Update node status
    pub fn update_node(&mut self, device_id: DeviceId, capacity: u64, utilization: f64) {
        if let Some(node) = self.storage_nodes.get_mut(&device_id) {
            node.available_capacity = capacity;
            node.utilization = utilization;
            node.last_seen = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
        }
    }

    /// Select storage nodes for chunk placement
    pub fn select_storage_nodes(&self, chunk_size: u64) -> AuraResult<Vec<DeviceId>> {
        let healthy_nodes: Vec<&StorageNodeInfo> = self.storage_nodes
            .values()
            .filter(|node| node.is_healthy() && node.available_capacity >= chunk_size)
            .collect();

        if healthy_nodes.is_empty() {
            return Err(aura_core::AuraError::internal("No healthy storage nodes available"));
        }

        let target_count = match &self.strategy {
            ReplicationStrategy::SimpleReplication { replica_count } => *replica_count,
            ReplicationStrategy::ErasureCoding { config } => config.total_chunks() as usize,
            ReplicationStrategy::Hybrid { critical_replicas, .. } => *critical_replicas,
        };

        if healthy_nodes.len() < target_count {
            return Err(aura_core::AuraError::internal(
                format!("Insufficient healthy nodes: need {}, have {}", target_count, healthy_nodes.len())
            ));
        }

        // Sort by placement score (best first)
        let mut sorted_nodes = healthy_nodes;
        sorted_nodes.sort_by(|a, b| b.placement_score().partial_cmp(&a.placement_score()).unwrap_or(std::cmp::Ordering::Equal));

        // Select top N nodes
        let selected: Vec<DeviceId> = sorted_nodes
            .into_iter()
            .take(target_count)
            .map(|node| node.device_id)
            .collect();

        Ok(selected)
    }

    /// Plan chunk layout for content based on replication strategy
    pub fn plan_chunk_layout(&self, content_size: u64) -> AuraResult<ChunkLayout> {
        let config = match &self.strategy {
            ReplicationStrategy::SimpleReplication { .. } => {
                // Use simple chunking for replication
                ErasureConfig::new(1, 0, 1024 * 1024) // 1MB chunks, no parity
            }
            ReplicationStrategy::ErasureCoding { config } => config.clone(),
            ReplicationStrategy::Hybrid { erasure_config, .. } => erasure_config.clone(),
        };

        // For now, create a simple layout - in practice this would compute actual chunk layout
        use aura_store::compute_chunk_layout;
        let dummy_content = vec![0u8; content_size as usize];
        compute_chunk_layout(&dummy_content, config)
            .map_err(|e| aura_core::AuraError::internal(format!("Failed to plan chunk layout: {}", e)))
    }

    /// Record chunk placement
    pub fn record_chunk_placement(&mut self, chunk_id: ChunkId, nodes: Vec<DeviceId>) {
        self.chunk_placement.insert(chunk_id, nodes.into_iter().collect());
    }

    /// Get nodes storing a specific chunk
    pub fn get_chunk_locations(&self, chunk_id: &ChunkId) -> Vec<DeviceId> {
        self.chunk_placement
            .get(chunk_id)
            .map(|nodes| nodes.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Check if chunk is adequately replicated
    pub fn is_chunk_replicated(&self, chunk_id: &ChunkId) -> bool {
        let current_replicas = self.chunk_placement
            .get(chunk_id)
            .map(|nodes| nodes.len())
            .unwrap_or(0);

        let required_replicas = match &self.strategy {
            ReplicationStrategy::SimpleReplication { replica_count } => *replica_count,
            ReplicationStrategy::ErasureCoding { config } => config.min_chunks() as usize,
            ReplicationStrategy::Hybrid { critical_replicas, .. } => *critical_replicas,
        };

        current_replicas >= required_replicas
    }

    /// Identify chunks needing repair
    pub fn chunks_needing_repair(&self) -> Vec<ChunkId> {
        self.chunk_placement
            .iter()
            .filter_map(|(chunk_id, nodes)| {
                // Filter out unhealthy nodes
                let healthy_nodes: Vec<&DeviceId> = nodes
                    .iter()
                    .filter(|device_id| {
                        self.storage_nodes
                            .get(device_id)
                            .map(|node| node.is_healthy())
                            .unwrap_or(false)
                    })
                    .collect();

                let required_replicas = match &self.strategy {
                    ReplicationStrategy::SimpleReplication { replica_count } => *replica_count,
                    ReplicationStrategy::ErasureCoding { config } => config.min_chunks() as usize,
                    ReplicationStrategy::Hybrid { critical_replicas, .. } => *critical_replicas,
                };

                if healthy_nodes.len() < required_replicas {
                    Some(chunk_id.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get replication statistics
    pub fn replication_stats(&self) -> ReplicationStats {
        let total_chunks = self.chunk_placement.len();
        let adequately_replicated = self.chunk_placement
            .keys()
            .filter(|chunk_id| self.is_chunk_replicated(chunk_id))
            .count();
        
        let under_replicated = total_chunks - adequately_replicated;

        ReplicationStats {
            total_chunks,
            adequately_replicated,
            under_replicated,
            healthy_nodes: self.storage_nodes.values().filter(|n| n.is_healthy()).count(),
            total_nodes: self.storage_nodes.len(),
        }
    }

    /// Get coordinator information
    pub fn info(&self) -> HashMap<String, String> {
        let mut info = HashMap::new();
        info.insert("device_id".to_string(), self.device_id.to_string());
        info.insert("strategy".to_string(), format!("{:?}", self.strategy));
        info.insert("total_nodes".to_string(), self.storage_nodes.len().to_string());
        info.insert("healthy_nodes".to_string(), 
            self.storage_nodes.values().filter(|n| n.is_healthy()).count().to_string());
        info.insert("tracked_chunks".to_string(), self.chunk_placement.len().to_string());
        info
    }
}

/// Replication statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationStats {
    /// Total number of chunks
    pub total_chunks: usize,
    /// Chunks with adequate replication
    pub adequately_replicated: usize,
    /// Chunks needing more replicas
    pub under_replicated: usize,
    /// Number of healthy storage nodes
    pub healthy_nodes: usize,
    /// Total number of registered nodes
    pub total_nodes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replication_coordinator_creation() {
        let device_id = DeviceId::new();
        let strategy = ReplicationStrategy::SimpleReplication { replica_count: 3 };
        let coordinator = ReplicationCoordinator::new(device_id, strategy);

        assert_eq!(coordinator.device_id, device_id);
        assert_eq!(coordinator.storage_nodes.len(), 0);
    }

    #[test]
    fn test_node_registration_and_selection() {
        let device_id = DeviceId::new();
        let strategy = ReplicationStrategy::SimpleReplication { replica_count: 2 };
        let mut coordinator = ReplicationCoordinator::new(device_id, strategy);

        // Register nodes
        let node1 = StorageNodeInfo::new(DeviceId::new(), 1000);
        let node2 = StorageNodeInfo::new(DeviceId::new(), 2000);
        let node3 = StorageNodeInfo::new(DeviceId::new(), 500);

        coordinator.register_node(node1);
        coordinator.register_node(node2);
        coordinator.register_node(node3);

        // Select nodes for 100 byte chunk
        let selected = coordinator.select_storage_nodes(100).unwrap();
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn test_chunk_placement_tracking() {
        let device_id = DeviceId::new();
        let strategy = ReplicationStrategy::SimpleReplication { replica_count: 2 };
        let mut coordinator = ReplicationCoordinator::new(device_id, strategy);

        let chunk_id = ChunkId::from_bytes(b"test_chunk");
        let nodes = vec![DeviceId::new(), DeviceId::new()];

        coordinator.record_chunk_placement(chunk_id, nodes.clone());
        let locations = coordinator.get_chunk_locations(&chunk_id);
        assert_eq!(locations.len(), 2);
    }

    #[test]
    fn test_node_health_assessment() {
        let device_id = DeviceId::new();
        let node = StorageNodeInfo::new(device_id, 1000);

        assert!(node.is_healthy()); // Fresh node should be healthy
        assert!(node.placement_score() > 0.0);
    }

    #[test]
    fn test_replication_stats() {
        let device_id = DeviceId::new();
        let strategy = ReplicationStrategy::SimpleReplication { replica_count: 2 };
        let mut coordinator = ReplicationCoordinator::new(device_id, strategy);

        // Add some test data
        let chunk1 = ChunkId::from_bytes(b"chunk1");
        let chunk2 = ChunkId::from_bytes(b"chunk2");
        
        coordinator.record_chunk_placement(chunk1, vec![DeviceId::new(), DeviceId::new()]);
        coordinator.record_chunk_placement(chunk2, vec![DeviceId::new()]); // Under-replicated

        let stats = coordinator.replication_stats();
        assert_eq!(stats.total_chunks, 2);
        assert_eq!(stats.adequately_replicated, 1);
        assert_eq!(stats.under_replicated, 1);
    }
}