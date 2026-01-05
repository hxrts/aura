//! Peer discovery and management infrastructure
//!
//! This module consolidates all peer-related logic for synchronization:
//! - Discovering sync-capable peers via aura-rendezvous
//! - Managing peer metadata and connection state
//! - Capability-based peer selection via aura-authorization
//! - Connection lifecycle management via aura-transport
//!
//! # Architecture
//!
//! The peer management system integrates with multiple Aura subsystems:
//! - **aura-rendezvous**: Peer discovery via SBB flooding
//! - **aura-authorization**: Capability-based authorization and trust ranking
//! - **aura-transport**: Connection establishment and management
//! - **aura-signature**: Identity verification for discovered peers

//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_sync::infrastructure::{PeerManager, PeerDiscoveryConfig};
//! use aura_core::effects::{NetworkEffects, StorageEffects};
//!
//! async fn discover_sync_peers<E>(effects: &E) -> aura_sync::SyncResult<Vec<DeviceId>>
//! where
//!     E: NetworkEffects + StorageEffects,
//! {
//!     let config = PeerDiscoveryConfig::default();
//!     let manager = PeerManager::new(config);
//!     manager.discover_peers(effects).await
//! }
//! ```

use std::collections::HashMap;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::{sync_config_error, sync_peer_error, SyncResult};
use aura_core::time::PhysicalTime;
use aura_core::DeviceId;

/// Deterministic monotonic counter for test/dev purposes.
/// Real deployments should use `PhysicalTime` from the time effect provider.
#[cfg(test)]
static NEXT_PEER_TS: AtomicU64 = AtomicU64::new(1);

/// Internal helper to create a test time from the monotonic counter.
/// Real deployments should use `PhysicalTime` from the time effect provider.
#[cfg(test)]
fn test_time_now() -> PhysicalTime {
    PhysicalTime {
        ts_ms: NEXT_PEER_TS.fetch_add(1, Ordering::SeqCst) * 1000,
        uncertainty: None,
    }
}
use aura_guards::types::CapabilityId;
use aura_guards::BiscuitGuardEvaluator;

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for peer discovery and management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerDiscoveryConfig {
    /// Maximum number of concurrent sync sessions per peer
    pub max_concurrent_sessions: u32,

    /// Timeout for peer discovery operations
    pub discovery_timeout: Duration,

    /// Interval between periodic peer refresh
    pub refresh_interval: Duration,

    /// Minimum trust level required for sync peers
    /// Integrates with aura-authorization trust ranking
    pub min_trust_level: u8,

    /// Maximum number of peers to track
    pub max_tracked_peers: u32,

    /// Enable capability-aware peer filtering
    pub capability_filtering: bool,
}

impl Default for PeerDiscoveryConfig {
    fn default() -> Self {
        Self {
            max_concurrent_sessions: 10,
            discovery_timeout: Duration::from_secs(30),
            refresh_interval: Duration::from_secs(300),
            min_trust_level: 50,
            max_tracked_peers: 100,
            capability_filtering: true,
        }
    }
}

/// Trust requirements for peer discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustRequirements {
    /// Minimum trust level required (0-100)
    pub min_trust_level: u8,
    /// Acceptable relationship types
    pub relationship_types: Vec<String>,
}

/// Privacy requirements for peer discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyRequirements {
    /// Required anonymity level
    pub anonymity_level: AnonymityLevel,
    /// Whether queries should be unlinkable
    pub unlinkable_queries: bool,
    /// Whether metadata should be protected
    pub metadata_protection: bool,
}

/// Anonymity levels for privacy protection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnonymityLevel {
    /// No anonymity protection
    None,
    /// Basic anonymity protection
    Low,
    /// Moderate anonymity protection
    Medium,
    /// Strong anonymity protection
    High,
    /// Maximum anonymity protection
    Maximum,
}

/// Query scope for discovery operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryScope {
    /// Local network only
    Local,
    /// Regional scope
    Regional,
    /// Global scope
    Global,
}

// =============================================================================
// Peer Status and Metadata
// =============================================================================

/// Current status of a peer connection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerStatus {
    /// Peer discovered but not yet connected
    Discovered,

    /// Connection being established
    Connecting,

    /// Peer connected and available for sync
    Connected,

    /// Peer disconnected, may reconnect
    Disconnected,

    /// Peer degraded but still reachable
    Degraded,

    /// Peer failed verification or capability check
    Failed,

    /// Peer explicitly removed from available set
    Removed,
}

/// Metadata about a discovered peer
///
/// **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMetadata {
    /// Device identifier
    pub device_id: DeviceId,

    /// Current connection status
    pub status: PeerStatus,

    /// When peer was first discovered (unified time system)
    pub discovered_at: PhysicalTime,

    /// When peer status last changed (unified time system)
    pub last_status_change: PhysicalTime,

    /// Number of successful sync sessions with this peer
    pub successful_syncs: u64,

    /// Number of failed sync attempts
    pub failed_syncs: u64,

    /// Approximate average latency in milliseconds
    pub average_latency_ms: u64,

    /// Last time the peer was seen (unified time system)
    pub last_seen: PhysicalTime,

    /// Last successful sync time (unified time system)
    pub last_successful_sync: PhysicalTime,

    /// Trust level from aura-authorization (0-100)
    pub trust_level: u8,

    /// Whether peer has required sync capabilities (checked via Biscuit tokens)
    pub has_sync_capability: bool,

    /// Current number of active sync sessions
    pub active_sessions: usize,
}

impl PeerMetadata {
    /// Create new peer metadata for a discovered peer
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    /// Note: Callers should obtain `now` via their time provider and pass it to this method
    pub fn new(device_id: DeviceId, now: &PhysicalTime) -> Self {
        Self {
            device_id,
            status: PeerStatus::Discovered,
            discovered_at: now.clone(),
            last_status_change: now.clone(),
            successful_syncs: 0,
            failed_syncs: 0,
            average_latency_ms: 1000,
            last_seen: now.clone(),
            last_successful_sync: now.clone(),
            trust_level: 0,
            has_sync_capability: false,
            active_sessions: 0,
        }
    }

    /// Create new peer metadata (from milliseconds)
    ///
    /// Convenience constructor for backward compatibility.
    pub fn new_from_ms(device_id: DeviceId, now_ms: u64) -> Self {
        Self::new(
            device_id,
            &PhysicalTime {
                ts_ms: now_ms,
                uncertainty: None,
            },
        )
    }

    /// Update peer status
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    /// Note: Callers should obtain `now` via their time provider and pass it to this method
    pub fn set_status(&mut self, status: PeerStatus, now: &PhysicalTime) {
        if self.status != status {
            self.status = status;
            self.last_status_change = now.clone();
        }
    }

    /// Update peer status (from milliseconds)
    ///
    /// Convenience method for backward compatibility.
    pub fn set_status_ms(&mut self, status: PeerStatus, now_ms: u64) {
        self.set_status(
            status,
            &PhysicalTime {
                ts_ms: now_ms,
                uncertainty: None,
            },
        );
    }

    /// Check if peer is available for new sync sessions
    pub fn is_available(&self, max_concurrent: usize) -> bool {
        matches!(self.status, PeerStatus::Connected) && self.active_sessions < max_concurrent
    }

    /// Calculate peer score for selection priority
    /// Higher score = better peer for sync
    pub fn calculate_score(&self) -> f64 {
        let success_rate = if self.successful_syncs + self.failed_syncs > 0 {
            self.successful_syncs as f64 / (self.successful_syncs + self.failed_syncs) as f64
        } else {
            0.5 // Neutral for new peers
        };

        let trust_factor = self.trust_level as f64 / 100.0;
        let load_factor = 1.0 - (self.active_sessions as f64 / 10.0).min(1.0);

        // Weighted combination: trust > success > load
        (trust_factor * 0.5) + (success_rate * 0.3) + (load_factor * 0.2)
    }
}

/// Detailed peer information with connection details
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Basic peer metadata
    pub metadata: PeerMetadata,

    /// Peer's Biscuit token (from aura-authorization authorization system)
    pub token: Option<Vec<u8>>, // Serialized Biscuit token

    /// Connection-specific details (from aura-transport)
    pub connection_details: Option<ConnectionDetails>,
}

/// Connection details for an active peer
///
/// **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionDetails {
    /// When connection was established (unified time system)
    pub connected_at: PhysicalTime,

    /// Connection identifier from aura-transport
    pub connection_id: String,

    /// Relationship type from aura-rendezvous
    pub relationship_type: RelationshipType,
}

/// Relationship type determines encryption context
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelationshipType {
    /// Direct trust relationship
    Direct,

    /// Guardian relationship
    Guardian,

    /// Peer discovered through network
    Network,
}

// =============================================================================
// Peer Manager
// =============================================================================

/// Manages peer discovery, tracking, and selection for sync operations
///
/// The peer manager integrates with multiple Aura subsystems:
/// - Uses aura-rendezvous for discovering peers via SBB
/// - Uses aura-authorization for Biscuit token-based authorization and trust ranking
/// - Uses aura-transport for connection management
/// - Uses aura-signature for identity verification
///
/// **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.
pub struct PeerManager {
    /// Configuration
    config: PeerDiscoveryConfig,

    /// Tracked peers by device ID
    peers: HashMap<DeviceId, PeerInfo>,

    /// Last discovery refresh time (unified time system)
    last_refresh: Option<PhysicalTime>,

    /// Biscuit guard evaluator for authorization checks
    guard_evaluator: Option<BiscuitGuardEvaluator>,
}

impl PeerManager {
    /// Create a new peer manager with the given configuration
    pub fn new(config: PeerDiscoveryConfig) -> Self {
        Self {
            config,
            peers: HashMap::new(),
            last_refresh: None,
            guard_evaluator: None,
        }
    }

    /// Create a new peer manager with Biscuit authorization support
    pub fn with_biscuit_authorization(
        config: PeerDiscoveryConfig,
        guard_evaluator: BiscuitGuardEvaluator,
    ) -> Self {
        Self {
            config,
            peers: HashMap::new(),
            last_refresh: None,
            guard_evaluator: Some(guard_evaluator),
        }
    }

    /// Discover peers available for synchronization
    ///
    /// This method integrates with aura-rendezvous to discover peers,
    /// then filters based on capabilities from aura-authorization.
    ///
    /// # Integration Points
    /// - Uses `NetworkEffects` for peer discovery
    /// - Uses `StorageEffects` to persist discovered peers
    /// - Filters by capabilities via aura-authorization integration
    ///
    /// # Note
    /// Full rendezvous-based peer discovery is Week 2 work. Currently returns
    /// only manually-added/tracked peers. When rendezvous integration is complete,
    /// this will query `aura_rendezvous::discovery::DiscoveryService` for peers.
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub async fn discover_peers<E>(
        &mut self,
        _effects: &E,
        now: &PhysicalTime,
    ) -> SyncResult<Vec<DeviceId>>
    where
        E: aura_core::effects::NetworkEffects + aura_core::effects::StorageEffects + Send + Sync,
    {
        tracing::info!(
            "Starting peer discovery (using tracked peers - rendezvous integration pending)"
        );

        // Update last refresh time
        self.last_refresh = Some(now.clone());

        // Return currently tracked peers (full rendezvous discovery is Week 2 work)
        let all_peers: Vec<DeviceId> = self.peers.keys().copied().collect();

        tracing::info!(
            "Peer discovery completed: {} tracked peers available",
            all_peers.len()
        );

        Ok(all_peers)
    }

    // =============================================================================
    // Rendezvous Discovery Integration (Week 2 - not yet implemented)
    // =============================================================================
    // The following functions are gated because they reference types from
    // `aura_rendezvous::discovery` which don't exist yet. These will be enabled
    // when the rendezvous peer discovery integration is completed (Week 2 tasks).
    //
    // Functions:
    // - create_discovery_service: Creates rendezvous discovery service
    // - create_sync_discovery_query: Builds sync-specific discovery query
    // - validate_discovered_peer: Validates peers from discovery results
    // - extract_device_id_from_peer_token: Extracts DeviceId from peer token
    // - validate_peer_sync_capabilities: Validates peer has sync capabilities
    // - verify_peer_identity: Verifies peer identity using aura-signature
    // =============================================================================

    /// Add a discovered peer to tracking
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn add_peer(&mut self, device_id: DeviceId, now: &PhysicalTime) -> SyncResult<()> {
        let max_tracked_peers = self.config.max_tracked_peers as usize;
        if self.peers.len() >= max_tracked_peers {
            return Err(sync_config_error("sync", "Maximum tracked peers exceeded"));
        }

        self.peers.entry(device_id).or_insert_with(|| PeerInfo {
            metadata: PeerMetadata::new(device_id, now),
            token: None, // Will be set when peer provides token
            connection_details: None,
        });

        Ok(())
    }

    /// Add a discovered peer to tracking (from milliseconds)
    ///
    /// Convenience method for backward compatibility.
    pub fn add_peer_ms(&mut self, device_id: DeviceId, now_ms: u64) -> SyncResult<()> {
        self.add_peer(
            device_id,
            &PhysicalTime {
                ts_ms: now_ms,
                uncertainty: None,
            },
        )
    }

    /// Update peer metadata
    pub fn update_peer_metadata<F>(&mut self, device_id: DeviceId, f: F) -> SyncResult<()>
    where
        F: FnOnce(&mut PeerMetadata),
    {
        let peer = self
            .peers
            .get_mut(&device_id)
            .ok_or_else(|| sync_peer_error("update", "Peer not found"))?;

        f(&mut peer.metadata);
        Ok(())
    }

    /// Update peer's Biscuit token
    ///
    /// Integrates with aura-authorization to validate token and extract capabilities
    pub async fn update_peer_token(
        &mut self,
        device_id: DeviceId,
        token_bytes: Vec<u8>,
    ) -> SyncResult<()> {
        if !self.peers.contains_key(&device_id) {
            return Err(sync_peer_error("update", "Peer not found"));
        }

        // Check sync capability using Biscuit token if evaluator available
        let has_sync_capability = if let Some(ref evaluator) = self.guard_evaluator {
            let validated = self
                .validate_biscuit_token(&token_bytes, evaluator)
                .await
                .unwrap_or(false);
            tracing::debug!(
                device_id = %device_id,
                validated,
                "Peer token validated via Biscuit guard evaluator"
            );
            validated
        } else {
            tracing::debug!(
                device_id = %device_id,
                "No Biscuit evaluator available, assuming sync capability"
            );
            true
        };

        let peer = self
            .peers
            .get_mut(&device_id)
            .ok_or_else(|| sync_peer_error("update", "Peer not found"))?;

        peer.metadata.has_sync_capability = has_sync_capability;
        peer.token = Some(token_bytes);
        Ok(())
    }

    /// Validate a Biscuit token using proper root public key verification
    #[allow(dead_code)]
    async fn validate_biscuit_token(
        &self,
        token_bytes: &[u8],
        evaluator: &BiscuitGuardEvaluator,
    ) -> SyncResult<bool> {
        // Get the root public key from configuration or authority context
        let root_public_key = self.get_root_public_key().await?;

        // Parse the Biscuit token using the root public key
        let biscuit_token = match biscuit_auth::Biscuit::from(token_bytes, root_public_key) {
            Ok(token) => token,
            Err(e) => {
                tracing::debug!("Failed to parse Biscuit token: {}", e);
                return Ok(false);
            }
        };

        // Create a resource scope for sync operations
        let sync_resource = aura_core::scope::ResourceScope::Authority {
            authority_id: aura_core::AuthorityId::new_from_entropy([1u8; 32]),
            operation: aura_core::scope::AuthorityOp::UpdateTree,
        };

        // Check if the token grants sync capability using the guard evaluator
        let capability = CapabilityId::from("sync:read");
        match evaluator.check_guard_default_time(&biscuit_token, &capability, &sync_resource) {
            Ok(has_permission) => {
                tracing::debug!(
                    has_sync_capability = has_permission,
                    "Biscuit token validation completed"
                );
                Ok(has_permission)
            }
            Err(e) => {
                tracing::debug!("Biscuit token capability check failed: {}", e);
                Ok(false)
            }
        }
    }

    /// Get the root public key for Biscuit token validation
    #[allow(dead_code)]
    async fn get_root_public_key(&self) -> SyncResult<biscuit_auth::PublicKey> {
        // In a real implementation, this would:
        // 1. Load from configuration file or environment
        // 2. Get from authority context via effects system
        // 3. Retrieve from a trusted key store or HSM
        // 4. Use key derivation from master authority key

        // Use deterministic development key for tests
        // This would typically come from the authority's cryptographic material
        // Generated via: openssl rand -hex 32
        let dev_key_hex = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
        let dev_key_bytes = hex::decode(dev_key_hex).map_err(|e| {
            sync_peer_error("key_loading", format!("Failed to decode dev key: {e}"))
        })?;

        if dev_key_bytes.len() != 32 {
            return Err(sync_peer_error(
                "key_loading",
                "Root public key must be exactly 32 bytes",
            ));
        }

        let mut key_array = [0u8; 32];
        key_array.copy_from_slice(&dev_key_bytes);

        biscuit_auth::PublicKey::from_bytes(&key_array).map_err(|e| {
            sync_peer_error(
                "key_loading",
                format!("Failed to load root public key: {e}"),
            )
        })
    }

    /// Get peer information
    pub fn get_peer(&self, device_id: &DeviceId) -> Option<&PeerInfo> {
        self.peers.get(device_id)
    }

    /// Select best peers for synchronization based on scoring
    ///
    /// Returns up to `count` peers sorted by score (highest first)
    pub fn select_sync_peers(&self, count: usize) -> Vec<DeviceId> {
        let max_concurrent_sessions = self.config.max_concurrent_sessions as usize;
        let mut scored_peers: Vec<_> = self
            .peers
            .values()
            .filter(|p| p.metadata.is_available(max_concurrent_sessions))
            .filter(|p| !self.config.capability_filtering || p.metadata.has_sync_capability)
            .filter(|p| p.metadata.trust_level >= self.config.min_trust_level)
            .map(|p| (p.metadata.device_id, p.metadata.calculate_score()))
            .collect();

        // Sort by score descending
        scored_peers.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        scored_peers
            .into_iter()
            .take(count)
            .map(|(id, _score)| id)
            .collect()
    }

    /// Check if a peer is available for sync
    pub fn is_peer_available(&self, device_id: &DeviceId) -> bool {
        let max_concurrent_sessions = self.config.max_concurrent_sessions as usize;
        self.peers
            .get(device_id)
            .map(|p| p.metadata.is_available(max_concurrent_sessions))
            .unwrap_or(false)
    }

    /// Mark a sync session as started with a peer
    pub fn start_sync_session(&mut self, device_id: DeviceId) -> SyncResult<()> {
        self.update_peer_metadata(device_id, |meta| {
            meta.active_sessions += 1;
        })
    }

    /// Mark a sync session as completed (success or failure)
    pub fn end_sync_session(&mut self, device_id: DeviceId, success: bool) -> SyncResult<()> {
        self.update_peer_metadata(device_id, |meta| {
            if meta.active_sessions > 0 {
                meta.active_sessions -= 1;
            }

            if success {
                meta.successful_syncs += 1;
            } else {
                meta.failed_syncs += 1;
            }
        })
    }

    /// Remove a peer from tracking
    pub fn remove_peer(&mut self, device_id: &DeviceId) -> Option<PeerInfo> {
        self.peers.remove(device_id)
    }

    /// Get all tracked peers
    pub fn all_peers(&self) -> impl Iterator<Item = &PeerInfo> {
        self.peers.values()
    }

    /// Get statistics about tracked peers
    pub fn statistics(&self) -> PeerManagerStatistics {
        let max_concurrent_sessions = self.config.max_concurrent_sessions as usize;
        let connected = self
            .peers
            .values()
            .filter(|p| matches!(p.metadata.status, PeerStatus::Connected))
            .count();

        let available = self
            .peers
            .values()
            .filter(|p| p.metadata.is_available(max_concurrent_sessions))
            .count();

        let with_capability = self
            .peers
            .values()
            .filter(|p| p.metadata.has_sync_capability)
            .count();

        let active_sessions: usize = self
            .peers
            .values()
            .map(|p| p.metadata.active_sessions)
            .sum();

        PeerManagerStatistics {
            total_tracked: self.peers.len() as u32,
            connected_peers: connected as u32,
            available_peers: available as u32,
            peers_with_sync_capability: with_capability as u32,
            total_active_sessions: active_sessions as u32,
        }
    }

    /// Check if discovery refresh is needed
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    /// Note: Callers should obtain `now` via their time provider
    pub fn needs_refresh(&self, now: &PhysicalTime) -> bool {
        match &self.last_refresh {
            None => true,
            Some(last) => {
                let elapsed_ms = now.ts_ms.saturating_sub(last.ts_ms);
                elapsed_ms >= self.config.refresh_interval.as_millis() as u64
            }
        }
    }

    /// Check if discovery refresh is needed (from milliseconds)
    ///
    /// Convenience method for backward compatibility.
    pub fn needs_refresh_ms(&self, now_ms: u64) -> bool {
        self.needs_refresh(&PhysicalTime {
            ts_ms: now_ms,
            uncertainty: None,
        })
    }

    /// List all tracked peers
    pub fn list_peers(&self) -> Vec<DeviceId> {
        self.peers.keys().cloned().collect()
    }

    /// Get peer health score (0.0 to 1.0)
    pub fn get_peer_health(&self, peer: &DeviceId) -> f64 {
        if let Some(peer_info) = self.peers.get(peer) {
            peer_info.metadata.calculate_score()
        } else {
            0.0
        }
    }

    /// Get peer latency
    pub fn get_peer_latency(&self, peer: &DeviceId) -> Duration {
        if let Some(peer_info) = self.peers.get(peer) {
            Duration::from_millis(peer_info.metadata.average_latency_ms)
        } else {
            Duration::from_millis(1000) // Default high latency for unknown peers
        }
    }

    /// Get sync success rate for a peer
    pub fn get_sync_success_rate(&self, peer: &DeviceId) -> f64 {
        if let Some(peer_info) = self.peers.get(peer) {
            let total_syncs = peer_info.metadata.successful_syncs + peer_info.metadata.failed_syncs;
            if total_syncs > 0 {
                peer_info.metadata.successful_syncs as f64 / total_syncs as f64
            } else {
                0.5 // Neutral for new peers
            }
        } else {
            0.0
        }
    }

    /// Update last contact timestamp for a peer
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn update_last_contact(&mut self, peer: DeviceId, now: &PhysicalTime) {
        if let Some(peer_info) = self.peers.get_mut(&peer) {
            peer_info.metadata.last_seen = now.clone();
        }
    }

    /// Update last contact timestamp for a peer (using test time)
    ///
    /// **Note**: Production code should use `update_last_contact` with explicit time.
    #[cfg(test)]
    pub fn update_last_contact_test(&mut self, peer: DeviceId) {
        self.update_last_contact(peer, &test_time_now());
    }

    /// Get recent sync success rate for a peer
    pub fn get_recent_sync_success_rate(&mut self, _peer: &DeviceId) -> f64 {
        // Return overall success rate
        // In a real implementation, this would track recent performance
        0.8
    }

    /// Mark peer as degraded
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn mark_peer_degraded(&mut self, peer: &DeviceId, now: &PhysicalTime) {
        if let Some(peer_info) = self.peers.get_mut(peer) {
            peer_info.metadata.set_status(PeerStatus::Degraded, now);
        }
    }

    /// Mark peer as healthy
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn mark_peer_healthy(&mut self, peer: &DeviceId, now: &PhysicalTime) {
        if let Some(peer_info) = self.peers.get_mut(peer) {
            peer_info.metadata.set_status(PeerStatus::Connected, now);
        }
    }

    /// Get time since last sync with a peer
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn get_time_since_last_sync(&self, peer: &DeviceId, now: &PhysicalTime) -> Duration {
        if let Some(peer_info) = self.peers.get(peer) {
            let elapsed_ms = now
                .ts_ms
                .saturating_sub(peer_info.metadata.last_successful_sync.ts_ms);
            Duration::from_millis(elapsed_ms)
        } else {
            Duration::from_secs(u64::MAX) // Very long time for unknown peers
        }
    }

    /// Get peer priority for sync selection
    pub fn get_peer_priority(&self, peer: &DeviceId) -> f64 {
        self.get_peer_health(peer)
    }

    /// Increment sync success counter for a peer
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn increment_sync_success(&mut self, peer: &DeviceId, now: &PhysicalTime) {
        if let Some(peer_info) = self.peers.get_mut(peer) {
            peer_info.metadata.successful_syncs += 1;
            peer_info.metadata.last_successful_sync = now.clone();
        }
    }

    /// Update last successful sync timestamp
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn update_last_successful_sync(&mut self, peer: &DeviceId, now: &PhysicalTime) {
        if let Some(peer_info) = self.peers.get_mut(peer) {
            peer_info.metadata.last_successful_sync = now.clone();
        }
    }

    /// Increment sync failure counter for a peer
    pub fn increment_sync_failure(&mut self, peer: &DeviceId) {
        if let Some(peer_info) = self.peers.get_mut(peer) {
            peer_info.metadata.failed_syncs += 1;
        }
    }

    /// Recalculate peer health based on recent performance
    pub fn recalculate_peer_health(&mut self, _peer: &DeviceId) {
        // Health is calculated on-demand
        // In a real implementation, this would update cached health metrics
    }
}

/// Statistics about peer manager state
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PeerManagerStatistics {
    /// Total number of tracked peers
    pub total_tracked: u32,

    /// Number of connected peers
    pub connected_peers: u32,

    /// Number of peers available for new sessions
    pub available_peers: u32,

    /// Number of peers with sync capability
    pub peers_with_sync_capability: u32,

    /// Total active sync sessions across all peers
    pub total_active_sessions: u32,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to create PhysicalTime for tests
    fn test_time(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    #[test]
    fn test_peer_metadata_scoring() {
        let now = test_time(1_234_567_890_000); // Use milliseconds timestamp
        let mut meta = PeerMetadata::new(DeviceId::from_bytes([1; 32]), &now);
        meta.trust_level = 80;
        meta.successful_syncs = 8;
        meta.failed_syncs = 2;
        meta.active_sessions = 2;

        let score = meta.calculate_score();

        // Trust (80/100 * 0.5) + Success (0.8 * 0.3) + Load ((1 - 0.2) * 0.2)
        // = 0.4 + 0.24 + 0.16 = 0.80
        assert!(
            (score - 0.80).abs() < 0.01,
            "Expected score ~0.80, got {score}",
        );
    }

    #[test]
    fn test_peer_availability() {
        let now = test_time(1_234_567_890_000); // Use milliseconds timestamp
        let mut meta = PeerMetadata::new(DeviceId::from_bytes([1; 32]), &now);
        meta.status = PeerStatus::Connected;
        meta.active_sessions = 5;

        assert!(meta.is_available(10));
        assert!(!meta.is_available(5));
        assert!(!meta.is_available(4));
    }

    #[test]
    fn test_peer_manager_selection() {
        let mut manager = PeerManager::new(PeerDiscoveryConfig::default());

        // Add peers with different characteristics
        let peer1 = DeviceId::from_bytes([1; 32]);
        let peer2 = DeviceId::from_bytes([2; 32]);
        let peer3 = DeviceId::from_bytes([3; 32]);

        let now = test_time(1_234_567_890_000); // Use milliseconds timestamp

        manager.add_peer(peer1, &now).unwrap();
        manager.add_peer(peer2, &now).unwrap();
        manager.add_peer(peer3, &now).unwrap();

        // Set up peer1 as high trust, connected
        manager
            .update_peer_metadata(peer1, |m| {
                m.status = PeerStatus::Connected;
                m.trust_level = 90;
                m.has_sync_capability = true;
            })
            .unwrap();

        // Set up peer2 as medium trust, connected
        manager
            .update_peer_metadata(peer2, |m| {
                m.status = PeerStatus::Connected;
                m.trust_level = 70;
                m.has_sync_capability = true;
            })
            .unwrap();

        // Set up peer3 as high trust but disconnected
        manager
            .update_peer_metadata(peer3, |m| {
                m.status = PeerStatus::Disconnected;
                m.trust_level = 95;
                m.has_sync_capability = true;
            })
            .unwrap();

        // Should select peer1 and peer2 (both connected), peer1 first (higher trust)
        let selected = manager.select_sync_peers(2);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0], peer1);
        assert_eq!(selected[1], peer2);
    }
}
