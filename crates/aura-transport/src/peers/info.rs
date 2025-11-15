//! Privacy-Aware Peer Information Types
//!
//! Essential peer management with built-in capability blinding and relationship scoping.
//! Target: <180 lines (concise implementation).

use aura_core::{DeviceId, RelationshipId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

use crate::PrivacyLevel;

/// Essential peer information with built-in capability blinding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    /// Peer device identifier
    pub device_id: DeviceId,

    /// Blinded capability advertisement
    pub capabilities: BlindedPeerCapabilities,

    /// Relationship-scoped metrics
    pub metrics: ScopedPeerMetrics,

    /// Last seen time
    pub last_seen: SystemTime,

    /// Available relationship contexts
    pub relationship_contexts: HashSet<RelationshipId>,

    /// Privacy-preserving status information
    pub status: PeerStatus,
}

/// Minimal privacy-preserving capability advertisement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlindedPeerCapabilities {
    /// Blinded capability identifiers
    blinded_capabilities: HashSet<String>,

    /// Privacy level for capability disclosure
    privacy_level: PrivacyLevel,

    /// Capability metadata (blinded)
    metadata: HashMap<String, String>,

    /// Time when capabilities were last updated
    last_updated: SystemTime,
}

/// Core metrics with relationship scoping
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopedPeerMetrics {
    /// Per-relationship connection counts
    connection_counts: HashMap<RelationshipId, u32>,

    /// Per-relationship message counts
    message_counts: HashMap<RelationshipId, u64>,

    /// Per-relationship latency measurements (blinded)
    latency_data: HashMap<RelationshipId, LatencyData>,

    /// Overall reliability score (privacy-preserving)
    pub reliability_score: f64,
}

/// Privacy-preserving latency measurements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyData {
    /// Blinded average latency (rounded to preserve privacy)
    avg_latency_ms: u32,

    /// Reliability indicator (high/medium/low)
    reliability: ReliabilityLevel,
}

/// Privacy-preserving peer status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PeerStatus {
    /// Peer is online and available
    Online {
        /// Capabilities available
        available_capabilities: BlindedPeerCapabilities,
    },
    /// Peer is offline or unreachable
    Offline {
        /// Last seen time
        last_seen: SystemTime,
    },
    /// Peer status unknown (privacy-preserving)
    Unknown,
}

/// Simple reliability levels (privacy-preserving)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReliabilityLevel {
    /// High reliability peer (>= 80% success rate)
    High,
    /// Medium reliability peer (50-80% success rate)
    Medium,
    /// Low reliability peer (0-50% success rate)
    Low,
    /// Unknown reliability (insufficient data)
    Unknown,
}

impl PeerInfo {
    /// Create new peer info with minimal capabilities
    pub fn new(device_id: DeviceId) -> Self {
        Self::new_at_time(device_id, SystemTime::UNIX_EPOCH)
    }

    /// Create new peer info at specific time
    pub fn new_at_time(device_id: DeviceId, current_time: SystemTime) -> Self {
        Self {
            device_id,
            capabilities: BlindedPeerCapabilities::new_at_time(current_time),
            metrics: ScopedPeerMetrics::new(),
            last_seen: current_time,
            relationship_contexts: HashSet::new(),
            status: PeerStatus::Unknown,
        }
    }

    /// Add relationship context
    pub fn add_relationship(&mut self, relationship_id: RelationshipId) {
        self.relationship_contexts.insert(relationship_id.clone());
        self.metrics.add_relationship_context(relationship_id);
    }

    /// Check if peer is available in relationship context
    pub fn is_available_in_relationship(&self, relationship_id: &RelationshipId) -> bool {
        self.relationship_contexts.contains(relationship_id)
            && matches!(self.status, PeerStatus::Online { .. })
    }

    /// Get capabilities for relationship (privacy-preserving)
    pub fn capabilities_for_relationship(
        &self,
        relationship_id: &RelationshipId,
    ) -> Option<&BlindedPeerCapabilities> {
        if self.relationship_contexts.contains(relationship_id) {
            Some(&self.capabilities)
        } else {
            None
        }
    }

    /// Update peer status
    pub fn update_status(&mut self, status: PeerStatus) {
        self.update_status_at_time(status, SystemTime::UNIX_EPOCH)
    }

    /// Update peer status at specific time
    pub fn update_status_at_time(&mut self, status: PeerStatus, current_time: SystemTime) {
        self.status = status;
        self.last_seen = current_time;
    }
}

impl BlindedPeerCapabilities {
    /// Create new blinded capabilities
    pub fn new() -> Self {
        Self::new_at_time(SystemTime::UNIX_EPOCH)
    }

    /// Create new blinded capabilities at specific time
    pub fn new_at_time(current_time: SystemTime) -> Self {
        Self {
            blinded_capabilities: HashSet::new(),
            privacy_level: PrivacyLevel::Blinded,
            metadata: HashMap::new(),
            last_updated: current_time,
        }
    }

    /// Add blinded capability
    pub fn add_capability(&mut self, capability: String) {
        self.add_capability_at_time(capability, SystemTime::UNIX_EPOCH)
    }

    /// Add blinded capability at specific time
    pub fn add_capability_at_time(&mut self, capability: String, current_time: SystemTime) {
        // Blind the capability using a simple hash-based approach
        let blinded_cap = format!(
            "cap_{}",
            capability
                .chars()
                .map(|c| (c as u8).wrapping_add(42))
                .fold(0u64, |acc, x| acc.wrapping_mul(31).wrapping_add(x as u64))
        );

        self.blinded_capabilities.insert(blinded_cap);
        self.last_updated = current_time;
    }

    /// Check if has blinded capability (privacy-preserving lookup)
    pub fn has_capability_like(&self, pattern: &str) -> bool {
        // Simple privacy-preserving capability matching
        let blinded_pattern = format!(
            "cap_{}",
            pattern
                .chars()
                .map(|c| (c as u8).wrapping_add(42))
                .fold(0u64, |acc, x| acc.wrapping_mul(31).wrapping_add(x as u64))
        );

        self.blinded_capabilities.contains(&blinded_pattern)
    }

    /// Get capability count (privacy-preserving)
    pub fn capability_count(&self) -> usize {
        self.blinded_capabilities.len()
    }
}

impl ScopedPeerMetrics {
    /// Create new scoped metrics
    pub fn new() -> Self {
        Self {
            connection_counts: HashMap::new(),
            message_counts: HashMap::new(),
            latency_data: HashMap::new(),
            reliability_score: 0.5, // Neutral starting score
        }
    }

    /// Add relationship context
    pub fn add_relationship_context(&mut self, relationship_id: RelationshipId) {
        self.connection_counts.insert(relationship_id.clone(), 0);
        self.message_counts.insert(relationship_id.clone(), 0);
        self.latency_data.insert(
            relationship_id,
            LatencyData {
                avg_latency_ms: 0,
                reliability: ReliabilityLevel::Unknown,
            },
        );
    }

    /// Record connection for relationship
    pub fn record_connection(&mut self, relationship_id: &RelationshipId) {
        if let Some(count) = self.connection_counts.get_mut(relationship_id) {
            *count += 1;
        }
    }

    /// Get connection count for relationship
    pub fn connection_count(&self, relationship_id: &RelationshipId) -> u32 {
        self.connection_counts
            .get(relationship_id)
            .copied()
            .unwrap_or(0)
    }

    /// Update reliability score
    pub fn update_reliability(&mut self, score: f64) {
        self.reliability_score = score.clamp(0.0, 1.0);
    }
}

impl Default for BlindedPeerCapabilities {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for ScopedPeerMetrics {
    fn default() -> Self {
        Self::new()
    }
}
