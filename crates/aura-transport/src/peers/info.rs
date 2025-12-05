//! Privacy-Aware Peer Information Types
//!
//! Essential peer management with built-in capability blinding and context scoping.
//! Target: <180 lines (concise implementation).

use aura_core::{
    identifiers::{AuthorityId, ContextId},
    time::{PhysicalTime, TimeStamp},
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::PrivacyLevel;

/// Essential peer information with built-in capability blinding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    /// Peer authority identifier (cross-authority communication)
    pub authority_id: AuthorityId,

    /// Blinded capability advertisement
    pub capabilities: BlindedPeerCapabilities,

    /// Context-scoped metrics
    pub metrics: ScopedPeerMetrics,

    /// Last seen time (using Aura unified time system)
    pub last_seen: TimeStamp,

    /// Available context scopes
    pub context_ids: HashSet<ContextId>,

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

    /// Time when capabilities were last updated (using Aura unified time system)
    last_updated: TimeStamp,
}

/// Core metrics with context scoping
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopedPeerMetrics {
    /// Per-context connection counts
    connection_counts: HashMap<ContextId, u32>,

    /// Per-context message counts
    message_counts: HashMap<ContextId, u64>,

    /// Per-context latency measurements (blinded)
    latency_data: HashMap<ContextId, LatencyData>,

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
        /// Last seen time (using Aura unified time system)
        last_seen: TimeStamp,
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
    pub fn new(authority_id: AuthorityId) -> Self {
        Self::new_with_timestamp(
            authority_id,
            TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            }),
        )
    }

    /// Create new peer info with specific timestamp
    pub fn new_with_timestamp(authority_id: AuthorityId, current_time: TimeStamp) -> Self {
        Self {
            authority_id,
            capabilities: BlindedPeerCapabilities::new_with_timestamp(current_time.clone()),
            metrics: ScopedPeerMetrics::new(),
            last_seen: current_time,
            context_ids: HashSet::new(),
            status: PeerStatus::Unknown,
        }
    }

    /// Add context scope
    pub fn add_context(&mut self, context_id: ContextId) {
        self.context_ids.insert(context_id);
        self.metrics.add_context(context_id);
    }

    /// Check if peer is available in context
    pub fn is_available_in_context(&self, context_id: &ContextId) -> bool {
        self.context_ids.contains(context_id) && matches!(self.status, PeerStatus::Online { .. })
    }

    /// Get capabilities for context (privacy-preserving)
    pub fn capabilities_for_context(
        &self,
        context_id: &ContextId,
    ) -> Option<&BlindedPeerCapabilities> {
        if self.context_ids.contains(context_id) {
            Some(&self.capabilities)
        } else {
            None
        }
    }

    /// Update peer status
    pub fn update_status(&mut self, status: PeerStatus) {
        self.update_status_with_timestamp(
            status,
            TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            }),
        )
    }

    /// Update peer status with specific timestamp
    pub fn update_status_with_timestamp(&mut self, status: PeerStatus, current_time: TimeStamp) {
        self.status = status;
        self.last_seen = current_time;
    }
}

impl BlindedPeerCapabilities {
    /// Create new blinded capabilities
    pub fn new() -> Self {
        Self::new_with_timestamp(TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 0,
            uncertainty: None,
        }))
    }

    /// Create new blinded capabilities with specific timestamp
    pub fn new_with_timestamp(current_time: TimeStamp) -> Self {
        Self {
            blinded_capabilities: HashSet::new(),
            privacy_level: PrivacyLevel::Blinded,
            metadata: HashMap::new(),
            last_updated: current_time,
        }
    }

    /// Add blinded capability
    pub fn add_capability(&mut self, capability: String) {
        self.add_capability_with_timestamp(
            capability,
            TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            }),
        )
    }

    /// Add blinded capability with specific timestamp
    pub fn add_capability_with_timestamp(&mut self, capability: String, current_time: TimeStamp) {
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

    /// Add context
    pub fn add_context(&mut self, context_id: ContextId) {
        self.connection_counts.insert(context_id, 0);
        self.message_counts.insert(context_id, 0);
        self.latency_data.insert(
            context_id,
            LatencyData {
                avg_latency_ms: 0,
                reliability: ReliabilityLevel::Unknown,
            },
        );
    }

    /// Record connection for context
    pub fn record_connection(&mut self, context_id: &ContextId) {
        if let Some(count) = self.connection_counts.get_mut(context_id) {
            *count += 1;
        }
    }

    /// Get connection count for context
    pub fn connection_count(&self, context_id: &ContextId) -> u32 {
        self.connection_counts.get(context_id).copied().unwrap_or(0)
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
