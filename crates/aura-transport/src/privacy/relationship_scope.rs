//! Relationship-Scoped Views for Privacy-Preserving Discovery
//!
//! Implements DKD-derived relationship contexts that determine what
//! capability information can be revealed to different peers.

use aura_core::hash::hasher;
use aura_core::identifiers::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::peers::PeerCapabilities;

/// Relationship context derived from DKD shared secrets
///
/// This determines what level of detail about capabilities can be
/// revealed to a specific peer based on the relationship strength.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RelationshipScope {
    /// No established relationship - minimal disclosure
    Public,
    /// Weak relationship - basic capability categories
    Acquaintance,
    /// Medium relationship - detailed capabilities without sensitive data
    Trusted,
    /// Strong relationship - full capability disclosure
    Intimate,
}

impl RelationshipScope {
    /// Derive relationship scope from DKD shared secret strength
    ///
    /// In practice, this would analyze the DKD derivation path length,
    /// shared session history, and trust metrics to determine scope.
    pub fn from_dkd_strength(
        _local_device: DeviceId,
        _remote_device: DeviceId,
        _shared_sessions: u32,
        _trust_score: f32,
    ) -> Self {
        // TODO fix - Simplified implementation - in production this would use:
        // - DKD derivation path analysis
        // - Session history evaluation
        // - Trust metric computation
        // - Relationship graph analysis

        // TODO fix - For now, use trust score as proxy
        if _trust_score >= 0.8 {
            Self::Intimate
        } else if _trust_score >= 0.6 {
            Self::Trusted
        } else if _trust_score >= 0.3 {
            Self::Acquaintance
        } else {
            Self::Public
        }
    }

    /// Check if this scope allows revealing specific capability details
    pub fn allows_capability_detail(&self, detail_type: CapabilityDetailType) -> bool {
        match (self, detail_type) {
            // Public scope: only basic existence
            (RelationshipScope::Public, CapabilityDetailType::Existence) => true,
            (RelationshipScope::Public, _) => false,

            // Acquaintance: existence + rough categories
            (RelationshipScope::Acquaintance, CapabilityDetailType::Existence) => true,
            (RelationshipScope::Acquaintance, CapabilityDetailType::Categories) => true,
            (RelationshipScope::Acquaintance, _) => false,

            // Trusted: + approximate capacity/performance
            (RelationshipScope::Trusted, CapabilityDetailType::Existence) => true,
            (RelationshipScope::Trusted, CapabilityDetailType::Categories) => true,
            (RelationshipScope::Trusted, CapabilityDetailType::Approximate) => true,
            (RelationshipScope::Trusted, _) => false,

            // Intimate: full details
            (RelationshipScope::Intimate, _) => true,
        }
    }
}

/// Types of capability detail that can be revealed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityDetailType {
    /// Basic capability existence (storage/relay/compute)
    Existence,
    /// Capability categories without specifics
    Categories,
    /// Approximate performance/capacity buckets
    Approximate,
    /// Exact performance metrics and versions
    Exact,
    /// Internal implementation details
    Internal,
}

/// Scoped view of peer capabilities for a specific relationship context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopedCapabilityView {
    /// The relationship scope this view was created for
    pub scope: RelationshipScope,

    /// Scoped capability information
    pub capabilities: ScopedCapabilities,

    /// Hash of full capabilities (for verification after relationship upgrade)
    pub full_capabilities_hash: [u8; 32],

    /// View creation timestamp
    pub created_at: u64,

    /// View expiration (relationship scopes can change)
    pub expires_at: u64,
}

/// Capability information scoped to a relationship level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopedCapabilities {
    /// Storage capability available for this relationship
    pub has_storage: bool,
    /// Relay capability available for this relationship
    pub has_relay: bool,
    /// Compute capability available for this relationship
    pub has_compute: bool,
    /// Communication capability available for this relationship
    pub has_communication: bool,

    /// Category-level details (Acquaintance+)
    pub storage_category: Option<StorageCategory>,
    /// Relay service category details for this relationship
    pub relay_category: Option<RelayCategory>,
    /// Compute service category details for this relationship
    pub compute_category: Option<ComputeCategory>,

    /// Approximate metrics (Trusted+)
    pub performance_bucket: Option<PerformanceBucket>,
    /// Capacity bucket indicating storage capacity level
    pub capacity_bucket: Option<CapacityBucket>,

    /// Exact metrics (Intimate only)
    pub exact_metrics: Option<ExactMetrics>,
}

/// Storage capability categories
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum StorageCategory {
    /// Minimal storage (< 100MB)
    Minimal,
    /// Standard storage (100MB - 10GB)
    Standard,
    /// Large storage (10GB - 1TB)
    Large,
    /// Massive storage (> 1TB)
    Massive,
}

/// Relay capability categories
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RelayCategory {
    /// Basic message relay
    Basic,
    /// High-throughput relay
    HighThroughput,
    /// Anonymous relay (onion routing)
    Anonymous,
    /// Multi-protocol relay
    MultiProtocol,
}

/// Compute capability categories
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ComputeCategory {
    /// Basic computation
    Basic,
    /// Cryptographic operations
    Crypto,
    /// Machine learning inference
    ML,
    /// General purpose computation
    GeneralPurpose,
}

/// Performance bucket for approximate disclosure
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PerformanceBucket {
    /// Low performance
    Low,
    /// Medium performance
    Medium,
    /// High performance
    High,
    /// Very high performance
    VeryHigh,
}

/// Capacity bucket for approximate disclosure
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CapacityBucket {
    /// Small capacity
    Small,
    /// Medium capacity
    Medium,
    /// Large capacity
    Large,
    /// Very large capacity
    VeryLarge,
}

/// Exact metrics for intimate relationships
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExactMetrics {
    /// Exact storage capacity in bytes
    pub storage_bytes: u64,
    /// Exact reliability score (0-100)
    pub reliability_score: u32,
    /// Exact average latency in milliseconds
    pub average_latency_ms: u32,
    /// Supported protocol versions
    pub protocol_versions: BTreeMap<String, String>,
}

/// Manager for creating scoped capability views
pub struct CapabilityViewManager {
    /// Local device ID
    device_id: DeviceId,
    /// Cache of relationship scopes
    scope_cache: BTreeMap<DeviceId, (RelationshipScope, u64)>, // (scope, computed_at)
}

impl CapabilityViewManager {
    /// Create a new capability view manager
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            scope_cache: BTreeMap::new(),
        }
    }

    /// Create a scoped view of capabilities for a specific peer relationship
    pub fn create_scoped_view(
        &mut self,
        for_peer: DeviceId,
        full_capabilities: &PeerCapabilities,
        current_time: u64,
        trust_score: f32,
        shared_sessions: u32,
    ) -> ScopedCapabilityView {
        // Determine relationship scope
        let scope = self.get_or_compute_scope(for_peer, current_time, trust_score, shared_sessions);

        // Hash full capabilities for verification
        let full_capabilities_hash = self.hash_capabilities(full_capabilities);

        // Create scoped capabilities based on relationship strength
        let capabilities = self.scope_capabilities(full_capabilities, scope);

        ScopedCapabilityView {
            scope,
            capabilities,
            full_capabilities_hash,
            created_at: current_time,
            expires_at: current_time + 3600, // 1 hour expiration
        }
    }

    /// Get or compute relationship scope with caching
    fn get_or_compute_scope(
        &mut self,
        peer: DeviceId,
        current_time: u64,
        trust_score: f32,
        shared_sessions: u32,
    ) -> RelationshipScope {
        // Check cache first (with 5 minute TTL)
        if let Some((cached_scope, computed_at)) = self.scope_cache.get(&peer) {
            if current_time.saturating_sub(*computed_at) < 300 {
                return *cached_scope;
            }
        }

        // Compute new scope
        let scope = RelationshipScope::from_dkd_strength(
            self.device_id,
            peer,
            shared_sessions,
            trust_score,
        );

        // Cache the result
        self.scope_cache.insert(peer, (scope, current_time));

        scope
    }

    /// Create scoped capabilities based on relationship strength
    fn scope_capabilities(
        &self,
        full_caps: &PeerCapabilities,
        scope: RelationshipScope,
    ) -> ScopedCapabilities {
        let mut scoped = ScopedCapabilities {
            has_storage: full_caps.storage_available,
            has_relay: full_caps.relay_available,
            has_compute: false, // Not in current PeerCapabilities
            has_communication: full_caps.communication_available,
            storage_category: None,
            relay_category: None,
            compute_category: None,
            performance_bucket: None,
            capacity_bucket: None,
            exact_metrics: None,
        };

        // Add category details for Acquaintance+
        if scope.allows_capability_detail(CapabilityDetailType::Categories) {
            if full_caps.storage_available {
                scoped.storage_category =
                    Some(self.categorize_storage(full_caps.storage_capacity_bytes));
            }
            if full_caps.relay_available {
                scoped.relay_category = Some(RelayCategory::Basic); // TODO fix - Simplified
            }
        }

        // Add approximate metrics for Trusted+
        if scope.allows_capability_detail(CapabilityDetailType::Approximate) {
            scoped.performance_bucket = Some(PerformanceBucket::Medium); // TODO fix - Simplified
            scoped.capacity_bucket = Some(self.bucket_capacity(full_caps.storage_capacity_bytes));
        }

        // Add exact metrics for Intimate only
        if scope.allows_capability_detail(CapabilityDetailType::Exact) {
            scoped.exact_metrics = Some(ExactMetrics {
                storage_bytes: full_caps.storage_capacity_bytes,
                reliability_score: 80,  // Would come from actual metrics
                average_latency_ms: 50, // Would come from actual metrics
                protocol_versions: BTreeMap::new(), // Would be populated from actual data
            });
        }

        scoped
    }

    /// Categorize storage capacity for scoped disclosure
    fn categorize_storage(&self, bytes: u64) -> StorageCategory {
        match bytes {
            0..=104_857_600 => StorageCategory::Minimal, // < 100MB
            104_857_601..=10_737_418_240 => StorageCategory::Standard, // 100MB - 10GB
            10_737_418_241..=1_099_511_627_776 => StorageCategory::Large, // 10GB - 1TB
            _ => StorageCategory::Massive,               // > 1TB
        }
    }

    /// Bucket capacity for approximate disclosure
    fn bucket_capacity(&self, bytes: u64) -> CapacityBucket {
        match bytes {
            0..=1_048_576 => CapacityBucket::Small,              // < 1MB
            1_048_577..=1_073_741_824 => CapacityBucket::Medium, // 1MB - 1GB
            1_073_741_825..=107_374_182_400 => CapacityBucket::Large, // 1GB - 100GB
            _ => CapacityBucket::VeryLarge,                      // > 100GB
        }
    }

    /// Hash capabilities for verification
    fn hash_capabilities(&self, capabilities: &PeerCapabilities) -> [u8; 32] {
        let mut h = hasher();
        h.update(capabilities.storage_available.to_string().as_bytes());
        h.update(&capabilities.storage_capacity_bytes.to_le_bytes());
        h.update(capabilities.relay_available.to_string().as_bytes());
        h.update(capabilities.communication_available.to_string().as_bytes());
        h.finalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::DeviceId;

    #[test]
    fn test_relationship_scope_derivation() {
        let local = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let remote = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));

        assert_eq!(
            RelationshipScope::from_dkd_strength(local, remote, 10, 0.9),
            RelationshipScope::Intimate
        );

        assert_eq!(
            RelationshipScope::from_dkd_strength(local, remote, 5, 0.7),
            RelationshipScope::Trusted
        );

        assert_eq!(
            RelationshipScope::from_dkd_strength(local, remote, 1, 0.4),
            RelationshipScope::Acquaintance
        );

        assert_eq!(
            RelationshipScope::from_dkd_strength(local, remote, 0, 0.1),
            RelationshipScope::Public
        );
    }

    #[test]
    fn test_capability_detail_permissions() {
        assert!(RelationshipScope::Public.allows_capability_detail(CapabilityDetailType::Existence));
        assert!(
            !RelationshipScope::Public.allows_capability_detail(CapabilityDetailType::Categories)
        );

        assert!(RelationshipScope::Acquaintance
            .allows_capability_detail(CapabilityDetailType::Categories));
        assert!(!RelationshipScope::Acquaintance
            .allows_capability_detail(CapabilityDetailType::Approximate));

        assert!(
            RelationshipScope::Trusted.allows_capability_detail(CapabilityDetailType::Approximate)
        );
        assert!(!RelationshipScope::Trusted.allows_capability_detail(CapabilityDetailType::Exact));

        assert!(RelationshipScope::Intimate.allows_capability_detail(CapabilityDetailType::Exact));
        assert!(
            RelationshipScope::Intimate.allows_capability_detail(CapabilityDetailType::Internal)
        );
    }

    #[test]
    fn test_scoped_capability_view_creation() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let mut manager = CapabilityViewManager::new(device_id);
        let peer_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));

        let capabilities = PeerCapabilities::full_service_peer(5_000_000_000); // 5GB

        // Test different relationship scopes
        let public_view = manager.create_scoped_view(peer_id, &capabilities, 1000, 0.1, 0);
        assert_eq!(public_view.scope, RelationshipScope::Public);
        assert!(public_view.capabilities.has_storage);
        assert!(public_view.capabilities.storage_category.is_none());

        let trusted_view = manager.create_scoped_view(peer_id, &capabilities, 1400, 0.7, 5);
        assert_eq!(trusted_view.scope, RelationshipScope::Trusted);
        assert!(trusted_view.capabilities.storage_category.is_some());
        assert!(trusted_view.capabilities.capacity_bucket.is_some());
        assert!(trusted_view.capabilities.exact_metrics.is_none());

        let intimate_view = manager.create_scoped_view(peer_id, &capabilities, 1800, 0.9, 10);
        assert_eq!(intimate_view.scope, RelationshipScope::Intimate);
        assert!(intimate_view.capabilities.exact_metrics.is_some());
    }

    #[test]
    fn test_storage_categorization() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let manager = CapabilityViewManager::new(device_id);

        assert!(matches!(
            manager.categorize_storage(50_000_000),
            StorageCategory::Minimal
        ));
        assert!(matches!(
            manager.categorize_storage(1_000_000_000),
            StorageCategory::Standard
        ));
        assert!(matches!(
            manager.categorize_storage(100_000_000_000),
            StorageCategory::Large
        ));
        assert!(matches!(
            manager.categorize_storage(2_000_000_000_000),
            StorageCategory::Massive
        ));
    }
}
