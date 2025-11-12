//! Manifest Manager for Privacy-Preserving Discovery
//!
//! Coordinates the creation and management of relationship-scoped capability
//! manifests, integrating blinded manifests with scoped views.

use super::{
    blinded_manifest::{BlindedManifest, CapabilityBucket},
    relationship_scope::{CapabilityViewManager, RelationshipScope, ScopedCapabilityView},
};
use crate::peers::PeerCapabilities;
use aura_core::identifiers::{AccountId, DeviceId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

/// Manages manifest generation for different relationship contexts
pub struct ManifestManager {
    /// Local device identity
    device_id: DeviceId,

    /// Account identity
    account_id: AccountId,

    /// Local device capabilities
    local_capabilities: PeerCapabilities,

    /// Capability view manager for relationship scoping
    view_manager: Arc<RwLock<CapabilityViewManager>>,

    /// Cached manifests for different relationship scopes
    manifest_cache: Arc<RwLock<BTreeMap<ManifestCacheKey, CachedManifest>>>,
}

/// Cache key for manifest storage
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct ManifestCacheKey {
    scope: RelationshipScope,
    capabilities_hash: [u8; 32],
}

/// Cached manifest with metadata
#[derive(Debug, Clone)]
struct CachedManifest {
    manifest: DeviceManifest,
    created_at: u64,
    expires_at: u64,
}

/// Complete device manifest for a specific relationship context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceManifest {
    /// Device identity
    pub device_id: DeviceId,

    /// Account identity (always revealed)
    pub account_id: AccountId,

    /// Blinded capability information
    pub blinded_manifest: BlindedManifest,

    /// Relationship-scoped detailed view (if appropriate)
    pub scoped_view: Option<ScopedCapabilityView>,

    /// Manifest generation timestamp
    pub created_at: u64,

    /// Manifest expiration
    pub expires_at: u64,

    /// Signature over manifest content
    pub signature: Vec<u8>, // TODO: Use proper signature type
}

impl ManifestManager {
    /// Create a new manifest manager
    pub fn new(
        device_id: DeviceId,
        account_id: AccountId,
        local_capabilities: PeerCapabilities,
    ) -> Self {
        let view_manager = Arc::new(RwLock::new(CapabilityViewManager::new(device_id)));

        Self {
            device_id,
            account_id,
            local_capabilities,
            view_manager,
            manifest_cache: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    /// Generate a manifest for discovery by a specific peer
    pub fn generate_manifest_for_peer(
        &self,
        requesting_peer: DeviceId,
        current_time: u64,
        trust_score: f32,
        shared_sessions: u32,
    ) -> Result<DeviceManifest, ManifestError> {
        // Determine relationship scope with the requesting peer
        let scope = {
            let _view_manager = self
                .view_manager
                .write()
                .map_err(|_| ManifestError::LockError)?;

            RelationshipScope::from_dkd_strength(
                self.device_id,
                requesting_peer,
                shared_sessions,
                trust_score,
            )
        };

        // Check cache first
        let capabilities_hash = self.hash_local_capabilities();
        let cache_key = ManifestCacheKey {
            scope,
            capabilities_hash,
        };

        if let Some(cached) = self.get_cached_manifest(&cache_key, current_time)? {
            return Ok(cached.manifest);
        }

        // Generate new manifest
        let manifest = self.generate_manifest_for_scope(
            scope,
            requesting_peer,
            current_time,
            trust_score,
            shared_sessions,
        )?;

        // Cache the result
        self.cache_manifest(cache_key, manifest.clone(), current_time)?;

        Ok(manifest)
    }

    /// Generate a public manifest for broadcast discovery
    pub fn generate_public_manifest(
        &self,
        current_time: u64,
    ) -> Result<DeviceManifest, ManifestError> {
        // Use a constant zero UUID as dummy peer ID for public context
        // (prevents random UUID generation for testability)
        let dummy_peer = DeviceId(uuid::Uuid::nil());
        self.generate_manifest_for_scope(
            RelationshipScope::Public,
            dummy_peer,
            current_time,
            0.0, // No trust for public
            0,   // No shared sessions for public
        )
    }

    /// Generate manifest for a specific relationship scope
    fn generate_manifest_for_scope(
        &self,
        scope: RelationshipScope,
        peer: DeviceId,
        current_time: u64,
        trust_score: f32,
        shared_sessions: u32,
    ) -> Result<DeviceManifest, ManifestError> {
        // Create blinded manifest (always included)
        let blinded_manifest = self.create_blinded_manifest()?;

        // Create scoped view if relationship allows detail
        let scoped_view = if scope != RelationshipScope::Public {
            let mut view_manager = self
                .view_manager
                .write()
                .map_err(|_| ManifestError::LockError)?;

            Some(view_manager.create_scoped_view(
                peer,
                &self.local_capabilities,
                current_time,
                trust_score,
                shared_sessions,
            ))
        } else {
            None
        };

        // Calculate expiration based on relationship scope
        let expires_at = current_time + self.get_manifest_ttl(scope);

        // Create manifest
        let mut manifest = DeviceManifest {
            device_id: self.device_id,
            account_id: self.account_id,
            blinded_manifest,
            scoped_view,
            created_at: current_time,
            expires_at,
            signature: Vec::new(), // Placeholder
        };

        // Sign the manifest
        manifest.signature = self.sign_manifest(&manifest)?;

        Ok(manifest)
    }

    /// Create blinded manifest from local capabilities
    fn create_blinded_manifest(&self) -> Result<BlindedManifest, ManifestError> {
        // Determine capability buckets
        let mut buckets = BTreeSet::new();
        buckets.insert(CapabilityBucket::Communication); // Always present

        if self.local_capabilities.storage_available {
            buckets.insert(CapabilityBucket::Storage);
        }
        if self.local_capabilities.relay_available {
            buckets.insert(CapabilityBucket::Relay);
        }

        // TODO: Add other capability types based on local configuration
        buckets.insert(CapabilityBucket::Protocol); // TODO fix - Simplified

        // Serialize detailed capabilities
        let detailed_capabilities = bincode::serialize(&self.local_capabilities)
            .map_err(|_| ManifestError::SerializationError)?;

        // Create feature categories
        let feature_categories = self.extract_feature_categories();

        Ok(BlindedManifest::from_capabilities(
            buckets,
            &detailed_capabilities,
            feature_categories,
        ))
    }

    /// Extract feature categories from local capabilities
    fn extract_feature_categories(&self) -> Vec<(String, Vec<String>)> {
        let mut categories = Vec::new();

        // Protocol support
        let mut protocols = Vec::new();
        protocols.push("aura-core".to_string());
        protocols.push("frost".to_string());
        protocols.push("dkd".to_string());
        categories.push(("protocols".to_string(), protocols));

        // Transport support
        let mut transports = Vec::new();
        transports.push("tcp".to_string());
        // TODO: Add other transport types based on actual configuration
        categories.push(("transports".to_string(), transports));

        // Capability-specific features
        if self.local_capabilities.storage_available {
            let storage_features = vec![
                "encrypted_chunks".to_string(),
                "capability_based_access".to_string(),
            ];
            categories.push(("storage".to_string(), storage_features));
        }

        if self.local_capabilities.relay_available {
            let relay_features = vec!["message_relay".to_string(), "session_routing".to_string()];
            categories.push(("relay".to_string(), relay_features));
        }

        categories
    }

    /// Get manifest TTL based on relationship scope
    fn get_manifest_ttl(&self, scope: RelationshipScope) -> u64 {
        match scope {
            RelationshipScope::Public => 3600, // 1 hour for public manifests
            RelationshipScope::Acquaintance => 1800, // 30 minutes for acquaintances
            RelationshipScope::Trusted => 900, // 15 minutes for trusted
            RelationshipScope::Intimate => 300, // 5 minutes for intimate (more dynamic)
        }
    }

    /// Hash local capabilities for cache keying
    fn hash_local_capabilities(&self) -> [u8; 32] {
        let serialized = bincode::serialize(&self.local_capabilities).unwrap_or_default();
        *blake3::hash(&serialized).as_bytes()
    }

    /// Get cached manifest if valid
    fn get_cached_manifest(
        &self,
        key: &ManifestCacheKey,
        current_time: u64,
    ) -> Result<Option<CachedManifest>, ManifestError> {
        let cache = self
            .manifest_cache
            .read()
            .map_err(|_| ManifestError::LockError)?;

        if let Some(cached) = cache.get(key) {
            if current_time < cached.expires_at {
                return Ok(Some(cached.clone()));
            }
        }

        Ok(None)
    }

    /// Cache manifest
    fn cache_manifest(
        &self,
        key: ManifestCacheKey,
        manifest: DeviceManifest,
        current_time: u64,
    ) -> Result<(), ManifestError> {
        let mut cache = self
            .manifest_cache
            .write()
            .map_err(|_| ManifestError::LockError)?;

        let cached = CachedManifest {
            manifest,
            created_at: current_time,
            expires_at: current_time + 300, // 5 minute cache TTL
        };

        cache.insert(key, cached);

        // Clean expired entries
        let expired_keys: Vec<_> = cache
            .iter()
            .filter(|(_, cached)| current_time >= cached.expires_at)
            .map(|(key, _)| key.clone())
            .collect();

        for key in expired_keys {
            cache.remove(&key);
        }

        Ok(())
    }

    /// Sign manifest (placeholder implementation)
    fn sign_manifest(&self, _manifest: &DeviceManifest) -> Result<Vec<u8>, ManifestError> {
        // TODO: Implement proper signing using device key
        // TODO fix - For now, return placeholder signature
        Ok(vec![0u8; 64])
    }

    /// Update local capabilities and invalidate cache
    pub fn update_local_capabilities(&mut self, new_capabilities: PeerCapabilities) {
        self.local_capabilities = new_capabilities;

        // Clear manifest cache since capabilities changed
        if let Ok(mut cache) = self.manifest_cache.write() {
            cache.clear();
        }
    }

    /// Get manifest statistics
    pub fn get_stats(&self) -> Result<ManifestStats, ManifestError> {
        let cache = self
            .manifest_cache
            .read()
            .map_err(|_| ManifestError::LockError)?;

        Ok(ManifestStats {
            cached_manifests: cache.len(),
            local_capabilities_hash: self.hash_local_capabilities(),
            supported_buckets: self.get_supported_buckets(),
        })
    }

    /// Get supported capability buckets
    fn get_supported_buckets(&self) -> BTreeSet<CapabilityBucket> {
        let mut buckets = BTreeSet::new();
        buckets.insert(CapabilityBucket::Communication);

        if self.local_capabilities.storage_available {
            buckets.insert(CapabilityBucket::Storage);
        }
        if self.local_capabilities.relay_available {
            buckets.insert(CapabilityBucket::Relay);
        }

        buckets.insert(CapabilityBucket::Protocol);
        buckets
    }
}

/// Manifest generation errors
#[derive(Debug)]
pub enum ManifestError {
    LockError,
    SerializationError,
    SignatureError,
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::LockError => write!(f, "Lock acquisition failed"),
            ManifestError::SerializationError => write!(f, "Serialization failed"),
            ManifestError::SignatureError => write!(f, "Signature generation failed"),
        }
    }
}

impl std::error::Error for ManifestError {}

/// Statistics about manifest manager state
#[derive(Debug)]
pub struct ManifestStats {
    /// Number of cached manifests
    pub cached_manifests: usize,
    /// Hash of current local capabilities
    pub local_capabilities_hash: [u8; 32],
    /// Supported capability buckets
    pub supported_buckets: BTreeSet<CapabilityBucket>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::{AccountId, DeviceId};
    use uuid::Uuid;
    fn create_test_capabilities() -> PeerCapabilities {
        PeerCapabilities {
            storage_available: true,
            storage_capacity_bytes: 1_000_000_000, // 1GB
            relay_available: true,
            communication_available: true,
        }
    }

    #[test]
    fn test_manifest_generation() {
        let account_id = AccountId(Uuid::new_v4());
        let device_id = DeviceId(Uuid::new_v4());
        let peer_id = DeviceId(Uuid::new_v4());

        let capabilities = create_test_capabilities();
        let manager = ManifestManager::new(device_id, account_id, capabilities);

        // Test public manifest
        let public_manifest = manager.generate_public_manifest(1000).unwrap();
        assert_eq!(public_manifest.device_id, device_id);
        assert_eq!(public_manifest.account_id, account_id);
        assert!(public_manifest.scoped_view.is_none());
        assert!(public_manifest
            .blinded_manifest
            .supports_bucket(CapabilityBucket::Storage));

        // Test trusted manifest
        let trusted_manifest = manager
            .generate_manifest_for_peer(peer_id, 1000, 0.7, 5)
            .unwrap();
        assert!(trusted_manifest.scoped_view.is_some());

        let scoped_view = trusted_manifest.scoped_view.unwrap();
        assert_eq!(scoped_view.scope, RelationshipScope::Trusted);
        assert!(scoped_view.capabilities.has_storage);
    }

    #[test]
    fn test_manifest_caching() {
        let account_id = AccountId(Uuid::new_v4());
        let device_id = DeviceId(Uuid::new_v4());
        let peer_id = DeviceId(Uuid::new_v4());

        let capabilities = create_test_capabilities();
        let manager = ManifestManager::new(device_id, account_id, capabilities);

        // Generate manifest twice
        let manifest1 = manager
            .generate_manifest_for_peer(peer_id, 1000, 0.7, 5)
            .unwrap();

        let manifest2 = manager
            .generate_manifest_for_peer(peer_id, 1050, 0.7, 5)
            .unwrap();

        // Should be identical due to caching
        assert_eq!(manifest1.created_at, manifest2.created_at);
        assert_eq!(manifest1.signature, manifest2.signature);
    }

    #[test]
    fn test_capability_buckets() {
        let account_id = AccountId(Uuid::new_v4());
        let device_id = DeviceId(Uuid::new_v4());

        let capabilities = create_test_capabilities();
        let manager = ManifestManager::new(device_id, account_id, capabilities);

        let buckets = manager.get_supported_buckets();
        assert!(buckets.contains(&CapabilityBucket::Communication));
        assert!(buckets.contains(&CapabilityBucket::Storage));
        assert!(buckets.contains(&CapabilityBucket::Relay));
        assert!(buckets.contains(&CapabilityBucket::Protocol));
    }

    #[test]
    fn test_manifest_expiration() {
        let account_id = AccountId(Uuid::new_v4());
        let device_id = DeviceId(Uuid::new_v4());

        let capabilities = create_test_capabilities();
        let manager = ManifestManager::new(device_id, account_id, capabilities);

        let public_manifest = manager.generate_public_manifest(1000).unwrap();
        let intimate_manifest = manager
            .generate_manifest_for_peer(DeviceId(Uuid::new_v4()), 1000, 0.9, 10)
            .unwrap();

        // Public manifests should have longer TTL than intimate ones
        assert!(public_manifest.expires_at > intimate_manifest.expires_at);
    }
}
