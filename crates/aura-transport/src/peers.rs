//! Peer Discovery and Network Types
//!
//! Core data structures for peer discovery, capability management, and network coordination.
//! These types are shared across storage, communication, and coordination subsystems.

use aura_types::identifiers::{AccountId, DeviceId};
use aura_types::relationships::TrustLevel;
use serde::{Deserialize, Serialize};

/// Unique identifier for a peer in the network
pub type PeerId = DeviceId;

/// Information about a peer in the network
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerInfo {
    /// Unique peer identifier
    pub peer_id: PeerId,
    /// Account ID associated with this peer
    pub account_id: AccountId,
    /// Timestamp when peer was last seen
    pub last_seen: u64,
    /// Peer's capabilities
    pub capabilities: PeerCapabilities,
    /// Peer's performance metrics
    pub metrics: PeerMetrics,
}

impl PeerInfo {
    /// Create new peer info
    pub fn new(
        peer_id: PeerId,
        account_id: AccountId,
        last_seen: u64,
        capabilities: PeerCapabilities,
        metrics: PeerMetrics,
    ) -> Self {
        Self {
            peer_id,
            account_id,
            last_seen,
            capabilities,
            metrics,
        }
    }

    /// Check if peer was seen recently (within last 5 minutes)
    pub fn is_recently_seen(&self, current_time: u64) -> bool {
        current_time.saturating_sub(self.last_seen) < 300 // 5 minutes
    }

    /// Update last seen timestamp
    pub fn update_last_seen(&mut self, timestamp: u64) {
        self.last_seen = timestamp;
    }
}

/// Capabilities advertised by a peer
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerCapabilities {
    /// Whether peer offers storage capabilities
    pub storage_available: bool,
    /// Storage capacity in bytes
    pub storage_capacity_bytes: u64,
    /// Whether peer can act as a relay
    pub relay_available: bool,
    /// Whether peer is available for communication
    pub communication_available: bool,
}

impl PeerCapabilities {
    /// Create capabilities for a storage peer
    pub fn storage_peer(capacity_bytes: u64) -> Self {
        Self {
            storage_available: true,
            storage_capacity_bytes: capacity_bytes,
            relay_available: false,
            communication_available: true,
        }
    }

    /// Create capabilities for a relay peer
    pub fn relay_peer() -> Self {
        Self {
            storage_available: false,
            storage_capacity_bytes: 0,
            relay_available: true,
            communication_available: true,
        }
    }

    /// Create capabilities for a communication-only peer
    pub fn communication_peer() -> Self {
        Self {
            storage_available: false,
            storage_capacity_bytes: 0,
            relay_available: false,
            communication_available: true,
        }
    }

    /// Create capabilities for a full-service peer
    pub fn full_service_peer(capacity_bytes: u64) -> Self {
        Self {
            storage_available: true,
            storage_capacity_bytes: capacity_bytes,
            relay_available: true,
            communication_available: true,
        }
    }
}

impl Default for PeerCapabilities {
    fn default() -> Self {
        Self::communication_peer()
    }
}

/// Performance metrics for a peer
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct PeerMetrics {
    /// Reliability score (0-100)
    pub reliability_score: u32,
    /// Average latency in milliseconds
    pub average_latency_ms: u32,
    /// Trust level assigned to this peer
    pub trust_level: TrustLevel,
}

impl PeerMetrics {
    /// Create new peer metrics
    pub fn new(reliability_score: u32, average_latency_ms: u32, trust_level: TrustLevel) -> Self {
        Self {
            reliability_score: reliability_score.min(100), // Cap at 100
            average_latency_ms,
            trust_level,
        }
    }

    /// Update reliability score (0-100)
    pub fn update_reliability(&mut self, new_score: u32) {
        self.reliability_score = new_score.min(100);
    }

    /// Update average latency
    pub fn update_latency(&mut self, latency_ms: u32) {
        self.average_latency_ms = latency_ms;
    }

    /// Update trust level
    pub fn update_trust(&mut self, trust_level: TrustLevel) {
        self.trust_level = trust_level;
    }

    /// Check if peer meets minimum quality thresholds
    pub fn meets_quality_threshold(&self, min_reliability: u32, max_latency_ms: u32) -> bool {
        self.reliability_score >= min_reliability && self.average_latency_ms <= max_latency_ms
    }
}

impl Default for PeerMetrics {
    fn default() -> Self {
        Self::new(50, 100, TrustLevel::None)
    }
}

/// Criteria for selecting peers based on use case
#[derive(Debug, Clone)]
pub enum SelectionCriteria {
    /// Select peers suitable for storage
    Storage {
        /// Minimum storage capacity in bytes
        min_capacity_bytes: u64,
        /// Minimum reliability score
        min_reliability: u32,
        /// Minimum trust level
        min_trust: TrustLevel,
    },
    /// Select peers suitable for communication
    Communication {
        /// Maximum acceptable latency in milliseconds
        max_latency_ms: u32,
        /// Whether peer must be currently online
        require_online: bool,
    },
    /// Select peers suitable as relays
    Relay {
        /// Minimum trust level
        min_trust: TrustLevel,
        /// Whether high capacity is required
        require_high_capacity: bool,
    },
}

impl SelectionCriteria {
    /// Create storage selection criteria with defaults
    pub fn storage_default() -> Self {
        SelectionCriteria::Storage {
            min_capacity_bytes: 1_000_000, // 1MB minimum
            min_reliability: 70,
            min_trust: TrustLevel::Medium,
        }
    }

    /// Create communication selection criteria with defaults
    pub fn communication_default() -> Self {
        SelectionCriteria::Communication {
            max_latency_ms: 200,
            require_online: true,
        }
    }

    /// Create relay selection criteria with defaults
    pub fn relay_default() -> Self {
        SelectionCriteria::Relay {
            min_trust: TrustLevel::Medium,
            require_high_capacity: false,
        }
    }

    /// Check if a peer meets these criteria
    pub fn matches(&self, peer: &PeerInfo, current_time: u64) -> bool {
        match self {
            SelectionCriteria::Storage {
                min_capacity_bytes,
                min_reliability,
                min_trust,
            } => {
                peer.capabilities.storage_available
                    && peer.capabilities.storage_capacity_bytes >= *min_capacity_bytes
                    && peer.metrics.reliability_score >= *min_reliability
                    && peer.metrics.trust_level >= *min_trust
            }
            SelectionCriteria::Communication {
                max_latency_ms,
                require_online,
            } => {
                peer.capabilities.communication_available
                    && peer.metrics.average_latency_ms <= *max_latency_ms
                    && (!require_online || peer.is_recently_seen(current_time))
            }
            SelectionCriteria::Relay {
                min_trust,
                require_high_capacity,
            } => {
                peer.capabilities.relay_available
                    && peer.metrics.trust_level >= *min_trust
                    && (!require_high_capacity
                        || peer.capabilities.storage_capacity_bytes > 1_000_000_000)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::identifiers::{AccountId, DeviceId};

    fn create_test_peer_info(
        reliability: u32,
        latency: u32,
        trust: TrustLevel,
        storage_capacity: u64,
    ) -> PeerInfo {
        let peer_id = DeviceId::new();
        let account_id = AccountId::new();

        let capabilities = if storage_capacity > 0 {
            PeerCapabilities::full_service_peer(storage_capacity)
        } else {
            PeerCapabilities::communication_peer()
        };

        let metrics = PeerMetrics::new(reliability, latency, trust);

        PeerInfo::new(peer_id, account_id, 1000, capabilities, metrics)
    }

    #[test]
    fn test_peer_info_creation() {
        let peer = create_test_peer_info(80, 50, TrustLevel::High, 1_000_000);

        assert_eq!(peer.metrics.reliability_score, 80);
        assert_eq!(peer.metrics.average_latency_ms, 50);
        assert_eq!(peer.metrics.trust_level, TrustLevel::High);
        assert!(peer.capabilities.storage_available);
        assert_eq!(peer.capabilities.storage_capacity_bytes, 1_000_000);
    }

    #[test]
    fn test_peer_capabilities() {
        let storage_peer = PeerCapabilities::storage_peer(5_000_000);
        assert!(storage_peer.storage_available);
        assert!(!storage_peer.relay_available);
        assert!(storage_peer.communication_available);

        let relay_peer = PeerCapabilities::relay_peer();
        assert!(!relay_peer.storage_available);
        assert!(relay_peer.relay_available);
        assert!(relay_peer.communication_available);

        let full_service = PeerCapabilities::full_service_peer(10_000_000);
        assert!(full_service.storage_available);
        assert!(full_service.relay_available);
        assert!(full_service.communication_available);
    }

    #[test]
    fn test_trust_level_ordering() {
        assert!(TrustLevel::High > TrustLevel::Medium);
        assert!(TrustLevel::Medium > TrustLevel::Low);
        assert!(TrustLevel::Low > TrustLevel::None);

        assert!(TrustLevel::High.meets_requirement(TrustLevel::Medium));
        assert!(!TrustLevel::Low.meets_requirement(TrustLevel::Medium));
    }

    #[test]
    fn test_peer_metrics_quality_threshold() {
        let metrics = PeerMetrics::new(80, 50, TrustLevel::High);

        assert!(metrics.meets_quality_threshold(70, 100));
        assert!(!metrics.meets_quality_threshold(90, 100));
        assert!(!metrics.meets_quality_threshold(70, 30));
    }

    #[test]
    fn test_selection_criteria_storage() {
        let peer = create_test_peer_info(80, 50, TrustLevel::High, 5_000_000);
        let current_time = 1000;

        let criteria = SelectionCriteria::Storage {
            min_capacity_bytes: 1_000_000,
            min_reliability: 70,
            min_trust: TrustLevel::Medium,
        };

        assert!(criteria.matches(&peer, current_time));

        let strict_criteria = SelectionCriteria::Storage {
            min_capacity_bytes: 10_000_000,
            min_reliability: 90,
            min_trust: TrustLevel::High,
        };

        assert!(!strict_criteria.matches(&peer, current_time));
    }

    #[test]
    fn test_selection_criteria_communication() {
        let peer = create_test_peer_info(80, 50, TrustLevel::High, 0);
        let current_time = 1200; // 200 seconds after last_seen

        let criteria = SelectionCriteria::Communication {
            max_latency_ms: 100,
            require_online: true,
        };

        assert!(criteria.matches(&peer, current_time));

        let strict_criteria = SelectionCriteria::Communication {
            max_latency_ms: 30,
            require_online: true,
        };

        assert!(!strict_criteria.matches(&peer, current_time));

        // Test offline requirement
        let old_time = 2000; // 1000 seconds after last_seen (too old)
        assert!(!criteria.matches(&peer, old_time));
    }

    #[test]
    fn test_peer_recently_seen() {
        let peer = create_test_peer_info(80, 50, TrustLevel::High, 1_000_000);

        assert!(peer.is_recently_seen(1200)); // 200 seconds later
        assert!(!peer.is_recently_seen(1400)); // 400 seconds later (too old)
    }
}