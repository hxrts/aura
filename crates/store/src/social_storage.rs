//! Storage via Rendezvous Relationships
//!
//! Integrates storage with SSB relationships to enable trust-based replica placement.
//! Once SSB establishes a relationship, storage can use that authenticated channel
//! for replica operations without additional handshakes.
//!
//! Reference: docs/040_storage.md Section 8.1 "SBB Integration Benefits"
//!          work/ssb_storage.md Phase 4.3

use crate::manifest::{AccountId, DeviceId, PeerId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Storage capability announcement in SSB Offer envelopes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageCapabilityAnnouncement {
    /// Available storage capacity in bytes
    pub available_capacity_bytes: u64,

    /// Minimum trust level required for storage relationships
    pub min_trust_level: TrustLevel,

    /// Supported storage operations
    pub supported_operations: Vec<StorageOperation>,

    /// Maximum chunk size accepted
    pub max_chunk_size: u32,

    /// Rate limits (chunks per second)
    pub rate_limit_chunks_per_sec: u32,

    /// Whether this peer accepts new storage relationships
    pub accepting_new_relationships: bool,

    /// Optional pricing information (for future economic features)
    pub pricing: Option<PricingInfo>,
}

impl StorageCapabilityAnnouncement {
    /// Create a new storage capability announcement
    pub fn new(
        available_capacity_bytes: u64,
        min_trust_level: TrustLevel,
        max_chunk_size: u32,
    ) -> Self {
        Self {
            available_capacity_bytes,
            min_trust_level,
            supported_operations: vec![
                StorageOperation::Store,
                StorageOperation::Retrieve,
                StorageOperation::Delete,
            ],
            max_chunk_size,
            rate_limit_chunks_per_sec: 100,
            accepting_new_relationships: true,
            pricing: None,
        }
    }

    /// Check if this announcement is compatible with requirements
    pub fn is_compatible_with(&self, requirements: &StorageRequirements) -> bool {
        // Check capacity
        if self.available_capacity_bytes < requirements.min_capacity_bytes {
            return false;
        }

        // Check if peer meets our trust requirements
        // If we require High trust but peer only provides Medium, incompatible
        if self.min_trust_level < requirements.trust_level {
            return false;
        }

        // Check chunk size
        if self.max_chunk_size < requirements.max_chunk_size {
            return false;
        }

        // Check operations
        requirements
            .required_operations
            .iter()
            .all(|op| self.supported_operations.contains(op))
    }
}

/// Storage operation types
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum StorageOperation {
    Store,
    Retrieve,
    Delete,
    ProofOfStorage,
}

/// Trust level for storage relationships
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustLevel {
    Low,
    Medium,
    High,
    Verified,
}

/// Storage requirements for peer selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageRequirements {
    pub min_capacity_bytes: u64,
    pub trust_level: TrustLevel,
    pub max_chunk_size: u32,
    pub required_operations: Vec<StorageOperation>,
}

impl StorageRequirements {
    pub fn basic(min_capacity_bytes: u64) -> Self {
        Self {
            min_capacity_bytes,
            trust_level: TrustLevel::Low,
            max_chunk_size: 4 * 1024 * 1024,
            required_operations: vec![StorageOperation::Store, StorageOperation::Retrieve],
        }
    }

    pub fn with_trust_level(mut self, trust_level: TrustLevel) -> Self {
        self.trust_level = trust_level;
        self
    }

    pub fn with_operations(mut self, operations: Vec<StorageOperation>) -> Self {
        self.required_operations = operations;
        self
    }
}

/// Pricing information for storage (future feature)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PricingInfo {
    pub cost_per_gb_month: u64,
    pub currency: String,
}

/// Storage confirmation in Answer envelopes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageConfirmation {
    /// Whether storage request is accepted
    pub accepted: bool,

    /// Allocated capacity in bytes
    pub allocated_capacity_bytes: u64,

    /// Storage relationship identifier
    pub storage_relationship_id: Vec<u8>,

    /// Optional rejection reason
    pub rejection_reason: Option<String>,
}

impl StorageConfirmation {
    pub fn accepted(allocated_capacity_bytes: u64, storage_relationship_id: Vec<u8>) -> Self {
        Self {
            accepted: true,
            allocated_capacity_bytes,
            storage_relationship_id,
            rejection_reason: None,
        }
    }

    pub fn rejected(reason: String) -> Self {
        Self {
            accepted: false,
            allocated_capacity_bytes: 0,
            storage_relationship_id: vec![],
            rejection_reason: Some(reason),
        }
    }
}

/// Authenticated peer with storage capability
#[derive(Debug, Clone)]
pub struct StoragePeer {
    pub peer_id: PeerId,
    pub device_id: DeviceId,
    pub account_id: AccountId,
    pub announcement: StorageCapabilityAnnouncement,
    pub relationship_established_at: u64,
    pub trust_score: f64,
    pub storage_metrics: StorageMetrics,
}

impl StoragePeer {
    /// Check if peer meets storage requirements
    pub fn meets_requirements(&self, requirements: &StorageRequirements) -> bool {
        self.announcement.is_compatible_with(requirements)
    }

    /// Calculate suitability score for storage placement (0.0 to 1.0)
    pub fn suitability_score(&self, requirements: &StorageRequirements) -> f64 {
        let mut score = 0.0;

        // Trust score (40%)
        score += self.trust_score * 0.4;

        // Reliability (30%)
        score += self.storage_metrics.reliability_score() * 0.3;

        // Capacity (20%)
        let capacity_ratio = (self.announcement.available_capacity_bytes as f64)
            / (requirements.min_capacity_bytes as f64).max(1.0);
        score += capacity_ratio.min(1.0) * 0.2;

        // Performance (10%)
        score += self.storage_metrics.performance_score() * 0.1;

        score.min(1.0)
    }
}

/// Storage metrics for peer evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageMetrics {
    pub total_chunks_stored: u64,
    pub total_chunks_retrieved: u64,
    pub failed_stores: u64,
    pub failed_retrievals: u64,
    pub avg_store_latency_ms: u32,
    pub avg_retrieve_latency_ms: u32,
    pub last_successful_interaction: u64,
}

impl StorageMetrics {
    pub fn new() -> Self {
        Self {
            total_chunks_stored: 0,
            total_chunks_retrieved: 0,
            failed_stores: 0,
            failed_retrievals: 0,
            avg_store_latency_ms: 0,
            avg_retrieve_latency_ms: 0,
            last_successful_interaction: 0,
        }
    }

    /// Calculate reliability score (0.0 to 1.0)
    pub fn reliability_score(&self) -> f64 {
        let total_ops = self.total_chunks_stored + self.total_chunks_retrieved;
        if total_ops == 0 {
            return 0.5; // Neutral score for new peers
        }

        let failed_ops = self.failed_stores + self.failed_retrievals;
        let success_rate = 1.0 - (failed_ops as f64 / total_ops as f64);
        success_rate.max(0.0).min(1.0)
    }

    /// Calculate performance score (0.0 to 1.0)
    pub fn performance_score(&self) -> f64 {
        // Lower latency = higher score
        // Assume 1000ms is poor, 100ms is excellent
        let avg_latency = (self.avg_store_latency_ms + self.avg_retrieve_latency_ms) / 2;
        let score = 1.0 - (avg_latency as f64 / 1000.0).min(1.0);
        score.max(0.0)
    }

    /// Update metrics after a store operation
    pub fn record_store(&mut self, latency_ms: u32, success: bool) {
        if success {
            self.total_chunks_stored += 1;
            self.avg_store_latency_ms =
                ((self.avg_store_latency_ms as u64 * self.total_chunks_stored + latency_ms as u64)
                    / (self.total_chunks_stored + 1)) as u32;
        } else {
            self.failed_stores += 1;
        }
    }

    /// Update metrics after a retrieve operation
    pub fn record_retrieve(&mut self, latency_ms: u32, success: bool) {
        if success {
            self.total_chunks_retrieved += 1;
            self.avg_retrieve_latency_ms =
                ((self.avg_retrieve_latency_ms as u64 * self.total_chunks_retrieved
                    + latency_ms as u64)
                    / (self.total_chunks_retrieved + 1)) as u32;
        } else {
            self.failed_retrievals += 1;
        }
    }
}

impl Default for StorageMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Storage peer discovery from SSB relationships
#[derive(Debug, Clone)]
pub struct SocialStoragePeerDiscovery {
    /// Map from peer_id to storage peer info
    peers: BTreeMap<PeerId, StoragePeer>,

    /// Relationship graph for trust evaluation
    relationship_graph: RelationshipGraph,
}

impl SocialStoragePeerDiscovery {
    pub fn new() -> Self {
        Self {
            peers: BTreeMap::new(),
            relationship_graph: RelationshipGraph::new(),
        }
    }

    /// Add a peer from SSB relationship
    pub fn add_peer(&mut self, peer: StoragePeer) {
        self.peers.insert(peer.peer_id.clone(), peer);
    }

    /// Remove a peer
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.peers.remove(peer_id);
    }

    /// Update peer's storage announcement
    pub fn update_announcement(
        &mut self,
        peer_id: &PeerId,
        announcement: StorageCapabilityAnnouncement,
    ) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.announcement = announcement;
        }
    }

    /// Update peer's storage metrics
    pub fn update_metrics(&mut self, peer_id: &PeerId, metrics: StorageMetrics) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.storage_metrics = metrics;
        }
    }

    /// Select best peers for storage based on requirements
    pub fn select_peers(
        &self,
        requirements: &StorageRequirements,
        count: usize,
    ) -> Vec<StoragePeer> {
        let mut suitable_peers: Vec<_> = self
            .peers
            .values()
            .filter(|p| p.meets_requirements(requirements))
            .cloned()
            .collect();

        // Sort by suitability score (descending)
        suitable_peers.sort_by(|a, b| {
            let score_b = b.suitability_score(requirements);
            let score_a = a.suitability_score(requirements);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        suitable_peers.into_iter().take(count).collect()
    }

    /// Get peer by ID
    pub fn get_peer(&self, peer_id: &PeerId) -> Option<&StoragePeer> {
        self.peers.get(peer_id)
    }

    /// Get all peers
    pub fn all_peers(&self) -> Vec<&StoragePeer> {
        self.peers.values().collect()
    }
}

impl Default for SocialStoragePeerDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

/// Relationship graph for trust evaluation
#[derive(Debug, Clone)]
pub struct RelationshipGraph {
    edges: BTreeMap<PeerId, Vec<PeerId>>,
}

impl RelationshipGraph {
    pub fn new() -> Self {
        Self {
            edges: BTreeMap::new(),
        }
    }

    pub fn add_relationship(&mut self, peer_a: PeerId, peer_b: PeerId) {
        self.edges
            .entry(peer_a.clone())
            .or_insert_with(Vec::new)
            .push(peer_b.clone());
        self.edges
            .entry(peer_b)
            .or_insert_with(Vec::new)
            .push(peer_a);
    }

    pub fn remove_relationship(&mut self, peer_a: &PeerId, peer_b: &PeerId) {
        if let Some(neighbors) = self.edges.get_mut(peer_a) {
            neighbors.retain(|p| p != peer_b);
        }
        if let Some(neighbors) = self.edges.get_mut(peer_b) {
            neighbors.retain(|p| p != peer_a);
        }
    }

    pub fn get_neighbors(&self, peer: &PeerId) -> Vec<PeerId> {
        self.edges.get(peer).cloned().unwrap_or_default()
    }
}

impl Default for RelationshipGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_peer(id: u8, capacity: u64, trust_level: TrustLevel, trust_score: f64) -> StoragePeer {
        StoragePeer {
            peer_id: vec![id],
            device_id: vec![id],
            account_id: vec![id],
            announcement: StorageCapabilityAnnouncement::new(
                capacity,
                trust_level,
                4 * 1024 * 1024,
            ),
            relationship_established_at: 0,
            trust_score,
            storage_metrics: StorageMetrics::new(),
        }
    }

    #[test]
    fn test_storage_capability_announcement() {
        let announcement = StorageCapabilityAnnouncement::new(
            1024 * 1024 * 1024, // 1 GB
            TrustLevel::Medium,
            4 * 1024 * 1024,
        );

        assert_eq!(announcement.available_capacity_bytes, 1024 * 1024 * 1024);
        assert_eq!(announcement.min_trust_level, TrustLevel::Medium);
        assert!(announcement.accepting_new_relationships);
    }

    #[test]
    fn test_compatibility_check() {
        let announcement = StorageCapabilityAnnouncement::new(
            1024 * 1024 * 1024,
            TrustLevel::Medium,
            4 * 1024 * 1024,
        );

        // Compatible requirements
        let requirements = StorageRequirements::basic(512 * 1024 * 1024)
            .with_trust_level(TrustLevel::Low)
            .with_operations(vec![StorageOperation::Store]);

        assert!(announcement.is_compatible_with(&requirements));

        // Incompatible - too much capacity required
        let requirements = StorageRequirements::basic(2 * 1024 * 1024 * 1024);
        assert!(!announcement.is_compatible_with(&requirements));

        // Incompatible - trust level too high
        let requirements =
            StorageRequirements::basic(512 * 1024 * 1024).with_trust_level(TrustLevel::Verified);
        assert!(!announcement.is_compatible_with(&requirements));
    }

    #[test]
    fn test_storage_confirmation() {
        let confirmation = StorageConfirmation::accepted(1024 * 1024 * 1024, vec![1, 2, 3]);
        assert!(confirmation.accepted);
        assert_eq!(confirmation.allocated_capacity_bytes, 1024 * 1024 * 1024);

        let rejection = StorageConfirmation::rejected("Insufficient capacity".to_string());
        assert!(!rejection.accepted);
        assert_eq!(
            rejection.rejection_reason,
            Some("Insufficient capacity".to_string())
        );
    }

    #[test]
    fn test_storage_metrics() {
        let mut metrics = StorageMetrics::new();

        // Record successful operations
        metrics.record_store(100, true);
        metrics.record_store(200, true);
        metrics.record_retrieve(50, true);

        assert_eq!(metrics.total_chunks_stored, 2);
        assert_eq!(metrics.total_chunks_retrieved, 1);
        assert_eq!(metrics.reliability_score(), 1.0);

        // Record failures
        metrics.record_store(100, false);
        assert_eq!(metrics.failed_stores, 1);
        assert!(metrics.reliability_score() < 1.0);
    }

    #[test]
    fn test_peer_suitability_score() {
        let mut peer = test_peer(1, 1024 * 1024 * 1024, TrustLevel::High, 0.9);

        // Set good metrics
        peer.storage_metrics.total_chunks_stored = 100;
        peer.storage_metrics.total_chunks_retrieved = 100;
        peer.storage_metrics.avg_store_latency_ms = 100;
        peer.storage_metrics.avg_retrieve_latency_ms = 100;

        let requirements = StorageRequirements::basic(512 * 1024 * 1024);
        let score = peer.suitability_score(&requirements);

        assert!(score > 0.7); // Should be a good score
        assert!(score <= 1.0);
    }

    #[test]
    fn test_peer_discovery() {
        let mut discovery = SocialStoragePeerDiscovery::new();

        // Add peers with different characteristics
        discovery.add_peer(test_peer(1, 2_000_000_000, TrustLevel::High, 0.9));
        discovery.add_peer(test_peer(2, 1_000_000_000, TrustLevel::Medium, 0.7));
        discovery.add_peer(test_peer(3, 500_000_000, TrustLevel::Low, 0.5));

        // Select peers for storage
        let requirements =
            StorageRequirements::basic(500_000_000).with_trust_level(TrustLevel::Medium);

        let selected = discovery.select_peers(&requirements, 2);

        // Should select the two most suitable peers
        assert_eq!(selected.len(), 2);
        // Peer 1 should be first (highest trust and capacity)
        assert_eq!(selected[0].peer_id, vec![1]);
    }

    #[test]
    fn test_peer_selection_with_requirements() {
        let mut discovery = SocialStoragePeerDiscovery::new();

        discovery.add_peer(test_peer(1, 100_000_000, TrustLevel::Low, 0.9));
        discovery.add_peer(test_peer(2, 1_000_000_000, TrustLevel::High, 0.8));

        // Require high capacity
        let requirements = StorageRequirements::basic(500_000_000);
        let selected = discovery.select_peers(&requirements, 10);

        // Only peer 2 has sufficient capacity
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].peer_id, vec![2]);
    }

    #[test]
    fn test_relationship_graph() {
        let mut graph = RelationshipGraph::new();

        let peer_a = vec![1];
        let peer_b = vec![2];
        let peer_c = vec![3];

        graph.add_relationship(peer_a.clone(), peer_b.clone());
        graph.add_relationship(peer_b.clone(), peer_c.clone());

        let neighbors_a = graph.get_neighbors(&peer_a);
        assert_eq!(neighbors_a, vec![vec![2]]);

        let neighbors_b = graph.get_neighbors(&peer_b);
        assert_eq!(neighbors_b.len(), 2); // Connected to both A and C

        graph.remove_relationship(&peer_a, &peer_b);
        let neighbors_a = graph.get_neighbors(&peer_a);
        assert_eq!(neighbors_a.len(), 0);
    }

    #[test]
    fn test_peer_meets_requirements() {
        let peer = test_peer(1, 1_000_000_000, TrustLevel::Medium, 0.8);

        let requirements =
            StorageRequirements::basic(500_000_000).with_trust_level(TrustLevel::Low);
        assert!(peer.meets_requirements(&requirements));

        let requirements = StorageRequirements::basic(2_000_000_000);
        assert!(!peer.meets_requirements(&requirements));

        let requirements =
            StorageRequirements::basic(500_000_000).with_trust_level(TrustLevel::Verified);
        assert!(!peer.meets_requirements(&requirements));
    }
}
