//! Social Replica Placement
//!
//! Trust-based replica placement that uses relationship history and peer reliability
//! to make intelligent storage decisions. This is Phase 6.1 of the SSB + Storage implementation.
//!
//! Reference: work/ssb_storage.md Phase 6.1

use crate::{
    manifest::PeerId,
    social_storage::{StoragePeer, StorageRequirements},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Trust scoring based on peer interaction history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustScore {
    /// Number of successful storage operations
    pub successful_operations: u64,

    /// Number of failed storage operations
    pub failed_operations: u64,

    /// Average response time in milliseconds
    pub avg_response_time_ms: u32,

    /// Uptime percentage (0.0-1.0)
    pub uptime_ratio: f64,

    /// Time-weighted reliability score (0.0-1.0)
    /// Recent interactions weighted more heavily
    pub reliability_score: f64,

    /// Last successful interaction timestamp
    pub last_success_timestamp: u64,

    /// Last failure timestamp
    pub last_failure_timestamp: u64,

    /// Relationship age in days
    pub relationship_age_days: u32,
}

impl TrustScore {
    /// Create a new trust score for a new peer
    pub fn new(current_time: u64) -> Self {
        Self {
            successful_operations: 0,
            failed_operations: 0,
            avg_response_time_ms: 0,
            uptime_ratio: 1.0,      // Optimistic initial assumption
            reliability_score: 0.5, // Neutral starting point
            last_success_timestamp: current_time,
            last_failure_timestamp: 0,
            relationship_age_days: 0,
        }
    }

    /// Update trust score after a successful operation
    pub fn record_success(&mut self, response_time_ms: u32, current_time: u64) {
        self.successful_operations += 1;
        self.last_success_timestamp = current_time;

        // Update average response time with exponential moving average
        if self.avg_response_time_ms == 0 {
            self.avg_response_time_ms = response_time_ms;
        } else {
            self.avg_response_time_ms =
                ((self.avg_response_time_ms as u64 * 9 + response_time_ms as u64) / 10) as u32;
        }

        // Recalculate reliability score
        self.update_reliability_score(current_time);
    }

    /// Update trust score after a failed operation
    pub fn record_failure(&mut self, current_time: u64) {
        self.failed_operations += 1;
        self.last_failure_timestamp = current_time;

        // Recalculate reliability score
        self.update_reliability_score(current_time);
    }

    /// Calculate time-weighted reliability score
    /// Recent interactions weighted more heavily (exponential decay)
    fn update_reliability_score(&mut self, current_time: u64) {
        let total_ops = self.successful_operations + self.failed_operations;
        if total_ops == 0 {
            self.reliability_score = 0.5;
            return;
        }

        // Base success rate
        let base_score = self.successful_operations as f64 / total_ops as f64;

        // Time decay factor: recent failures hurt more
        let time_since_last_failure = if self.last_failure_timestamp > 0 {
            current_time.saturating_sub(self.last_failure_timestamp)
        } else {
            u64::MAX
        };

        let decay_weight = if time_since_last_failure < 86400 {
            // Failure within 24 hours: significant penalty
            0.7
        } else if time_since_last_failure < 604800 {
            // Failure within 7 days: moderate penalty
            0.85
        } else {
            // Old failure: minimal penalty
            0.95
        };

        self.reliability_score = base_score * decay_weight;

        // Update uptime ratio
        self.uptime_ratio = self.successful_operations as f64 / total_ops as f64;
    }

    /// Calculate overall trust score (0.0-1.0)
    pub fn overall_score(&self) -> f64 {
        // Weight factors
        const RELIABILITY_WEIGHT: f64 = 0.5;
        const RESPONSE_TIME_WEIGHT: f64 = 0.2;
        const RELATIONSHIP_AGE_WEIGHT: f64 = 0.2;
        const UPTIME_WEIGHT: f64 = 0.1;

        // Normalize response time (assume 500ms is baseline, 100ms is excellent)
        let response_score = if self.avg_response_time_ms == 0 {
            1.0
        } else {
            (500.0 / (self.avg_response_time_ms as f64 + 100.0)).min(1.0)
        };

        // Relationship age bonus (up to 1.0 at 30 days)
        let age_score = (self.relationship_age_days as f64 / 30.0).min(1.0);

        // Weighted combination
        self.reliability_score * RELIABILITY_WEIGHT
            + response_score * RESPONSE_TIME_WEIGHT
            + age_score * RELATIONSHIP_AGE_WEIGHT
            + self.uptime_ratio * UPTIME_WEIGHT
    }

    /// Check if peer is currently healthy
    pub fn is_healthy(&self, current_time: u64) -> bool {
        // Peer is healthy if:
        // 1. No recent failures (within 1 hour)
        // 2. Reliability score above threshold OR no operations yet (optimistic for new peers)

        let recent_failure = self.last_failure_timestamp > 0
            && current_time.saturating_sub(self.last_failure_timestamp) < 3600;

        let has_good_reliability = self.reliability_score > 0.6
            || (self.successful_operations == 0 && self.failed_operations == 0);

        !recent_failure && has_good_reliability
    }
}

/// Social replica placement manager
#[derive(Debug, Clone)]
pub struct SocialReplicaPlacement {
    /// Trust scores per peer
    trust_scores: HashMap<PeerId, TrustScore>,

    /// Target replica count
    target_replicas: usize,

    /// Minimum trust score for replica placement
    min_trust_threshold: f64,
}

impl SocialReplicaPlacement {
    /// Create a new social replica placement manager
    pub fn new(target_replicas: usize) -> Self {
        Self {
            trust_scores: HashMap::new(),
            target_replicas,
            min_trust_threshold: 0.4, // Require at least 40% trust
        }
    }

    /// Initialize trust score for a new peer
    pub fn add_peer(&mut self, peer_id: PeerId, current_time: u64) {
        self.trust_scores
            .entry(peer_id)
            .or_insert_with(|| TrustScore::new(current_time));
    }

    /// Update trust score after successful operation
    pub fn record_success(&mut self, peer_id: &PeerId, response_time_ms: u32, current_time: u64) {
        if let Some(score) = self.trust_scores.get_mut(peer_id) {
            score.record_success(response_time_ms, current_time);
        }
    }

    /// Update trust score after failed operation
    pub fn record_failure(&mut self, peer_id: &PeerId, current_time: u64) {
        if let Some(score) = self.trust_scores.get_mut(peer_id) {
            score.record_failure(current_time);
        }
    }

    /// Select peers for replica placement based on trust scores
    pub fn select_replicas(
        &self,
        available_peers: &[StoragePeer],
        requirements: &StorageRequirements,
        current_time: u64,
    ) -> Vec<PeerId> {
        let mut scored_peers: Vec<(PeerId, f64)> = available_peers
            .iter()
            .filter(|peer| {
                // Filter by capacity and trust level
                peer.announcement.available_capacity_bytes >= requirements.min_capacity_bytes
                    && peer.announcement.min_trust_level <= requirements.trust_level
            })
            .filter_map(|peer| {
                // Get trust score if available
                let trust_score = self
                    .trust_scores
                    .get(&peer.peer_id)
                    .map(|ts| ts.overall_score())
                    .unwrap_or(0.5); // Default neutral score for new peers

                // Check if healthy
                let is_healthy = self
                    .trust_scores
                    .get(&peer.peer_id)
                    .map(|ts| ts.is_healthy(current_time))
                    .unwrap_or(true);

                if trust_score >= self.min_trust_threshold && is_healthy {
                    // Combine trust score with storage metrics
                    let capacity_score = peer.announcement.available_capacity_bytes as f64
                        / requirements.min_capacity_bytes as f64;
                    let reliability = peer.storage_metrics.reliability_score();

                    // Weighted combination: trust (50%), reliability (30%), capacity (20%)
                    let final_score =
                        trust_score * 0.5 + reliability * 0.3 + capacity_score.min(1.0) * 0.2;

                    Some((peer.peer_id.clone(), final_score))
                } else {
                    None
                }
            })
            .collect();

        // Sort by score (descending)
        scored_peers.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        // Select top N peers
        scored_peers
            .into_iter()
            .take(self.target_replicas)
            .map(|(peer_id, _)| peer_id)
            .collect()
    }

    /// Adjust replica count dynamically based on peer reliability
    pub fn adjust_replica_count(&mut self, failure_rate: f64) {
        // If failure rate is high, increase replica count
        if failure_rate > 0.2 && self.target_replicas < 10 {
            self.target_replicas += 1;
        } else if failure_rate < 0.05 && self.target_replicas > 3 {
            self.target_replicas -= 1;
        }
    }

    /// Get trust score for a peer
    pub fn get_trust_score(&self, peer_id: &PeerId) -> Option<&TrustScore> {
        self.trust_scores.get(peer_id)
    }

    /// Get current target replica count
    pub fn target_replicas(&self) -> usize {
        self.target_replicas
    }

    /// Get all healthy peers
    pub fn get_healthy_peers(&self, current_time: u64) -> Vec<PeerId> {
        self.trust_scores
            .iter()
            .filter(|(_, score)| score.is_healthy(current_time))
            .map(|(peer_id, _)| peer_id.clone())
            .collect()
    }
}

/// Social accountability tracker for storage failures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountabilityTracker {
    /// Failure reports per peer
    failure_reports: HashMap<PeerId, Vec<FailureReport>>,

    /// Accountability threshold (number of failures before action)
    accountability_threshold: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureReport {
    /// Type of failure
    pub failure_type: FailureType,

    /// Timestamp of failure
    pub timestamp: u64,

    /// Reporter peer ID
    pub reporter: PeerId,

    /// Evidence (e.g., missing chunk ID)
    pub evidence: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FailureType {
    /// Chunk not found when it should exist
    ChunkNotFound,

    /// Chunk corrupted (integrity check failed)
    ChunkCorrupted,

    /// Peer unresponsive
    PeerUnresponsive,

    /// Insufficient capacity reported
    InsufficientCapacity,

    /// Proof-of-storage challenge failed
    ProofOfStorageFailed,
}

impl AccountabilityTracker {
    /// Create new accountability tracker
    pub fn new(accountability_threshold: u32) -> Self {
        Self {
            failure_reports: HashMap::new(),
            accountability_threshold,
        }
    }

    /// Report a storage failure
    pub fn report_failure(
        &mut self,
        peer_id: PeerId,
        failure_type: FailureType,
        reporter: PeerId,
        evidence: String,
        timestamp: u64,
    ) {
        let report = FailureReport {
            failure_type,
            timestamp,
            reporter,
            evidence,
        };

        self.failure_reports
            .entry(peer_id)
            .or_insert_with(Vec::new)
            .push(report);
    }

    /// Check if peer should be held accountable (exceeded threshold)
    pub fn should_take_action(&self, peer_id: &PeerId) -> bool {
        self.failure_reports
            .get(peer_id)
            .map(|reports| reports.len() as u32 >= self.accountability_threshold)
            .unwrap_or(false)
    }

    /// Get failure reports for a peer
    pub fn get_reports(&self, peer_id: &PeerId) -> Option<&Vec<FailureReport>> {
        self.failure_reports.get(peer_id)
    }

    /// Clear reports for a peer (e.g., after resolution)
    pub fn clear_reports(&mut self, peer_id: &PeerId) {
        self.failure_reports.remove(peer_id);
    }

    /// Get failure count for a peer
    pub fn failure_count(&self, peer_id: &PeerId) -> u32 {
        self.failure_reports
            .get(peer_id)
            .map(|reports| reports.len() as u32)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::social_storage::TrustLevel;

    #[test]
    fn test_trust_score_initialization() {
        let score = TrustScore::new(1000);
        assert_eq!(score.successful_operations, 0);
        assert_eq!(score.failed_operations, 0);
        assert_eq!(score.reliability_score, 0.5);
        assert_eq!(score.uptime_ratio, 1.0);
    }

    #[test]
    fn test_trust_score_success_updates() {
        let mut score = TrustScore::new(1000);

        score.record_success(100, 1100);
        assert_eq!(score.successful_operations, 1);
        assert_eq!(score.avg_response_time_ms, 100);
        assert!(score.reliability_score > 0.5);

        score.record_success(200, 1200);
        assert_eq!(score.successful_operations, 2);
        assert!(score.avg_response_time_ms > 100 && score.avg_response_time_ms < 200);
    }

    #[test]
    fn test_trust_score_failure_impact() {
        let mut score = TrustScore::new(1000);

        // Build up good history
        for i in 0..10 {
            score.record_success(100, 1000 + i * 100);
        }

        let good_score = score.overall_score();

        // Record a recent failure
        score.record_failure(2000);

        let bad_score = score.overall_score();
        assert!(bad_score < good_score);
    }

    #[test]
    fn test_social_replica_placement_selection() {
        let mut placement = SocialReplicaPlacement::new(3);

        // Add peers with different trust scores
        let peer1 = vec![1];
        let peer2 = vec![2];
        let peer3 = vec![3];

        placement.add_peer(peer1.clone(), 1000);
        placement.add_peer(peer2.clone(), 1000);
        placement.add_peer(peer3.clone(), 1000);

        // Build reputation for peer1
        for i in 0..20 {
            placement.record_success(&peer1, 50, 1000 + i * 100);
        }

        // Mixed history for peer2
        for i in 0..10 {
            placement.record_success(&peer2, 150, 1000 + i * 100);
        }
        placement.record_failure(&peer2, 2000);

        // Limited history for peer3
        placement.record_success(&peer3, 100, 1000);

        let peers = vec![
            StoragePeer {
                peer_id: peer1.clone(),
                device_id: peer1.clone(),
                account_id: vec![100],
                announcement: crate::StorageCapabilityAnnouncement::new(
                    1_000_000_000,
                    TrustLevel::High,
                    4 * 1024 * 1024,
                ),
                relationship_established_at: 1000,
                trust_score: 0.9,
                storage_metrics: crate::StorageMetrics::new(),
            },
            StoragePeer {
                peer_id: peer2.clone(),
                device_id: peer2.clone(),
                account_id: vec![100],
                announcement: crate::StorageCapabilityAnnouncement::new(
                    1_000_000_000,
                    TrustLevel::High,
                    4 * 1024 * 1024,
                ),
                relationship_established_at: 1000,
                trust_score: 0.8,
                storage_metrics: crate::StorageMetrics::new(),
            },
            StoragePeer {
                peer_id: peer3.clone(),
                device_id: peer3.clone(),
                account_id: vec![100],
                announcement: crate::StorageCapabilityAnnouncement::new(
                    1_000_000_000,
                    TrustLevel::High,
                    4 * 1024 * 1024,
                ),
                relationship_established_at: 1000,
                trust_score: 0.7,
                storage_metrics: crate::StorageMetrics::new(),
            },
        ];

        let requirements =
            StorageRequirements::basic(500_000_000).with_trust_level(TrustLevel::High);
        // Use timestamp > 3600 after peer2's failure (2000 + 3600 = 5600)
        let selected = placement.select_replicas(&peers, &requirements, 6000);

        assert_eq!(selected.len(), 3);
        // Peer1 should be first (best history)
        assert_eq!(selected[0], peer1);
    }

    #[test]
    fn test_accountability_tracker() {
        let mut tracker = AccountabilityTracker::new(3);

        let peer_id = vec![1];
        let reporter = vec![2];

        // Report failures
        tracker.report_failure(
            peer_id.clone(),
            FailureType::ChunkNotFound,
            reporter.clone(),
            "chunk_123".to_string(),
            1000,
        );

        assert!(!tracker.should_take_action(&peer_id));
        assert_eq!(tracker.failure_count(&peer_id), 1);

        tracker.report_failure(
            peer_id.clone(),
            FailureType::ChunkCorrupted,
            reporter.clone(),
            "chunk_456".to_string(),
            1100,
        );

        tracker.report_failure(
            peer_id.clone(),
            FailureType::PeerUnresponsive,
            reporter.clone(),
            "timeout".to_string(),
            1200,
        );

        assert!(tracker.should_take_action(&peer_id));
        assert_eq!(tracker.failure_count(&peer_id), 3);

        // Clear reports
        tracker.clear_reports(&peer_id);
        assert!(!tracker.should_take_action(&peer_id));
        assert_eq!(tracker.failure_count(&peer_id), 0);
    }

    #[test]
    fn test_dynamic_replica_adjustment() {
        let mut placement = SocialReplicaPlacement::new(3);

        // High failure rate increases replica count
        placement.adjust_replica_count(0.25);
        assert_eq!(placement.target_replicas(), 4);

        // Low failure rate decreases replica count
        let mut placement2 = SocialReplicaPlacement::new(5);
        placement2.adjust_replica_count(0.02);
        assert_eq!(placement2.target_replicas(), 4);
    }

    #[test]
    fn test_trust_score_is_healthy() {
        let mut score = TrustScore::new(1000);

        // New peer with no operations is healthy (optimistic)
        assert!(score.is_healthy(1000));

        // Build successful history
        for i in 0..10 {
            score.record_success(100, 1000 + i * 100);
        }
        assert!(score.is_healthy(2000));

        // Recent failure makes unhealthy
        score.record_failure(2000);
        assert!(!score.is_healthy(2100)); // Within 1 hour

        // After timeout, becomes healthy again if reliability good
        assert!(score.is_healthy(6000)); // > 1 hour later
    }
}
