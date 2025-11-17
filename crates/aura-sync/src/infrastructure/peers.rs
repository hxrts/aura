//! Peer discovery and management infrastructure
//!
//! This module consolidates all peer-related logic for synchronization:
//! - Discovering sync-capable peers via aura-rendezvous
//! - Managing peer metadata and connection state
//! - Capability-based peer selection via aura-wot
//! - Connection lifecycle management via aura-transport
//!
//! # Architecture
//!
//! The peer management system integrates with multiple Aura subsystems:
//! - **aura-rendezvous**: Peer discovery via SBB flooding
//! - **aura-wot**: Capability-based authorization and trust ranking
//! - **aura-transport**: Connection establishment and management
//! - **aura-verify**: Identity verification for discovered peers

//!
//! # Usage
//!
//! ```rust,no_run
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

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use aura_core::{DeviceId, AuraResult, AuraError};
use crate::core::{SyncError, SyncResult};

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for peer discovery and management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerDiscoveryConfig {
    /// Maximum number of concurrent sync sessions per peer
    pub max_concurrent_sessions: usize,

    /// Timeout for peer discovery operations
    pub discovery_timeout: Duration,

    /// Interval between periodic peer refresh
    pub refresh_interval: Duration,

    /// Minimum trust level required for sync peers
    /// Integrates with aura-wot trust ranking
    pub min_trust_level: u8,

    /// Maximum number of peers to track
    pub max_tracked_peers: usize,

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

    /// Peer failed verification or capability check
    Failed,

    /// Peer explicitly removed from available set
    Removed,
}

/// Metadata about a discovered peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMetadata {
    /// Device identifier
    pub device_id: DeviceId,

    /// Current connection status
    pub status: PeerStatus,

    /// When peer was first discovered
    pub discovered_at: Instant,

    /// When peer status last changed
    pub last_status_change: Instant,

    /// Number of successful sync sessions with this peer
    pub successful_syncs: u64,

    /// Number of failed sync attempts
    pub failed_syncs: u64,

    /// Trust level from aura-wot (0-100)
    pub trust_level: u8,

    /// Whether peer has required sync capabilities
    pub has_sync_capability: bool,

    /// Current number of active sync sessions
    pub active_sessions: usize,
}

impl PeerMetadata {
    /// Create new peer metadata for a discovered peer
    ///
    /// Note: Callers should obtain `now` via `TimeEffects::now_instant()` and pass it to this method
    pub fn new(device_id: DeviceId, now: Instant) -> Self {
        Self {
            device_id,
            status: PeerStatus::Discovered,
            discovered_at: now,
            last_status_change: now,
            successful_syncs: 0,
            failed_syncs: 0,
            trust_level: 0,
            has_sync_capability: false,
            active_sessions: 0,
        }
    }

    /// Update peer status
    ///
    /// Note: Callers should obtain `now` via `TimeEffects::now_instant()` and pass it to this method
    pub fn set_status(&mut self, status: PeerStatus, now: Instant) {
        if self.status != status {
            self.status = status;
            self.last_status_change = now;
        }
    }

    /// Check if peer is available for new sync sessions
    pub fn is_available(&self, max_concurrent: usize) -> bool {
        matches!(self.status, PeerStatus::Connected)
            && self.active_sessions < max_concurrent
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

    /// Peer's advertised capabilities (from aura-wot)
    pub capabilities: HashSet<String>,

    /// Connection-specific details (from aura-transport)
    pub connection_details: Option<ConnectionDetails>,
}

/// Connection details for an active peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionDetails {
    /// When connection was established
    pub connected_at: Instant,

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
/// - Uses aura-wot for capability verification and trust ranking
/// - Uses aura-transport for connection management
/// - Uses aura-verify for identity verification
pub struct PeerManager {
    /// Configuration
    config: PeerDiscoveryConfig,

    /// Tracked peers by device ID
    peers: HashMap<DeviceId, PeerInfo>,

    /// Last discovery refresh time
    last_refresh: Option<Instant>,
}

impl PeerManager {
    /// Create a new peer manager with the given configuration
    pub fn new(config: PeerDiscoveryConfig) -> Self {
        Self {
            config,
            peers: HashMap::new(),
            last_refresh: None,
        }
    }

    /// Discover peers available for synchronization
    ///
    /// This method integrates with aura-rendezvous to discover peers,
    /// then filters based on capabilities from aura-wot.
    ///
    /// # Integration Points
    /// - Uses `NetworkEffects` for peer discovery
    /// - Uses `StorageEffects` to persist discovered peers
    /// - Filters by capabilities via aura-wot integration
    pub async fn discover_peers<E>(&mut self, _effects: &E) -> SyncResult<Vec<DeviceId>>
    where
        E: Send + Sync,
    {
        // TODO: Integrate with aura-rendezvous DiscoveryService
        // let discovery_service = DiscoveryService::new();
        // let discovered = discovery_service.discover_peers(effects).await?;

        // For now, return tracked peers
        // TODO: ARCHITECTURAL VIOLATION - This should accept `now: Instant` from TimeEffects.
        // Infrastructure timing is NOT exempt from the effect system - it affects protocol decisions
        // and must be testable. Refactor to accept time parameter from caller.
        #[allow(clippy::disallowed_methods)]
        let now = Instant::now();
        self.last_refresh = Some(now);

        Ok(self.peers.keys().copied().collect())
    }

    /// Add a discovered peer to tracking
    pub fn add_peer(&mut self, device_id: DeviceId) -> SyncResult<()> {
        if self.peers.len() >= self.config.max_tracked_peers {
            return Err(SyncError::Configuration(
                "Maximum tracked peers exceeded".to_string()
            ));
        }

        // TODO: ARCHITECTURAL VIOLATION - Should accept `now: Instant` parameter from TimeEffects.
        // Peer discovery timing affects protocol behavior and must be testable.
        #[allow(clippy::disallowed_methods)]
        self.peers.entry(device_id).or_insert_with(|| {
            PeerInfo {
                metadata: PeerMetadata::new(device_id, Instant::now()),
                capabilities: HashSet::new(),
                connection_details: None,
            }
        });

        Ok(())
    }

    /// Update peer metadata
    pub fn update_peer_metadata<F>(&mut self, device_id: DeviceId, f: F) -> SyncResult<()>
    where
        F: FnOnce(&mut PeerMetadata),
    {
        let peer = self.peers.get_mut(&device_id)
            .ok_or_else(|| SyncError::PeerNotFound(device_id))?;

        f(&mut peer.metadata);
        Ok(())
    }

    /// Update peer capabilities
    ///
    /// Integrates with aura-wot to validate capabilities
    pub fn update_peer_capabilities(
        &mut self,
        device_id: DeviceId,
        capabilities: HashSet<String>,
    ) -> SyncResult<()> {
        let peer = self.peers.get_mut(&device_id)
            .ok_or_else(|| SyncError::PeerNotFound(device_id))?;

        // Check for sync capability
        peer.metadata.has_sync_capability = capabilities.contains("sync_journal")
            || capabilities.contains("sync_state");

        peer.capabilities = capabilities;
        Ok(())
    }

    /// Get peer information
    pub fn get_peer(&self, device_id: &DeviceId) -> Option<&PeerInfo> {
        self.peers.get(device_id)
    }

    /// Select best peers for synchronization based on scoring
    ///
    /// Returns up to `count` peers sorted by score (highest first)
    pub fn select_sync_peers(&self, count: usize) -> Vec<DeviceId> {
        let mut scored_peers: Vec<_> = self.peers
            .values()
            .filter(|p| p.metadata.is_available(self.config.max_concurrent_sessions))
            .filter(|p| {
                !self.config.capability_filtering
                    || p.metadata.has_sync_capability
            })
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
        self.peers
            .get(device_id)
            .map(|p| p.metadata.is_available(self.config.max_concurrent_sessions))
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
        let connected = self.peers.values()
            .filter(|p| matches!(p.metadata.status, PeerStatus::Connected))
            .count();

        let available = self.peers.values()
            .filter(|p| p.metadata.is_available(self.config.max_concurrent_sessions))
            .count();

        let with_capability = self.peers.values()
            .filter(|p| p.metadata.has_sync_capability)
            .count();

        PeerManagerStatistics {
            total_tracked: self.peers.len(),
            connected_peers: connected,
            available_peers: available,
            peers_with_sync_capability: with_capability,
            total_active_sessions: self.peers.values()
                .map(|p| p.metadata.active_sessions)
                .sum(),
        }
    }

    /// Check if discovery refresh is needed
    pub fn needs_refresh(&self) -> bool {
        match self.last_refresh {
            None => true,
            Some(last) => last.elapsed() >= self.config.refresh_interval,
        }
    }
}

/// Statistics about peer manager state
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PeerManagerStatistics {
    /// Total number of tracked peers
    pub total_tracked: usize,

    /// Number of connected peers
    pub connected_peers: usize,

    /// Number of peers available for new sessions
    pub available_peers: usize,

    /// Number of peers with sync capability
    pub peers_with_sync_capability: usize,

    /// Total active sync sessions across all peers
    pub total_active_sessions: usize,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_metadata_scoring() {
        #[allow(clippy::disallowed_methods)]
        let now = Instant::now();
        let mut meta = PeerMetadata::new(DeviceId::from_bytes([1; 32]), now);
        meta.trust_level = 80;
        meta.successful_syncs = 8;
        meta.failed_syncs = 2;
        meta.active_sessions = 2;

        let score = meta.calculate_score();

        // Trust (80/100 * 0.5) + Success (0.8 * 0.3) + Load ((1 - 0.2) * 0.2)
        // = 0.4 + 0.24 + 0.16 = 0.80
        assert!((score - 0.80).abs() < 0.01, "Expected score ~0.80, got {}", score);
    }

    #[test]
    fn test_peer_availability() {
        #[allow(clippy::disallowed_methods)]
        let now = Instant::now();
        let mut meta = PeerMetadata::new(DeviceId::from_bytes([1; 32]), now);
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

        manager.add_peer(peer1).unwrap();
        manager.add_peer(peer2).unwrap();
        manager.add_peer(peer3).unwrap();

        #[allow(clippy::disallowed_methods)]
        let now = Instant::now();

        // Set up peer1 as high trust, connected
        manager.update_peer_metadata(peer1, |m| {
            m.status = PeerStatus::Connected;
            m.trust_level = 90;
            m.has_sync_capability = true;
        }).unwrap();

        // Set up peer2 as medium trust, connected
        manager.update_peer_metadata(peer2, |m| {
            m.status = PeerStatus::Connected;
            m.trust_level = 70;
            m.has_sync_capability = true;
        }).unwrap();

        // Set up peer3 as high trust but disconnected
        manager.update_peer_metadata(peer3, |m| {
            m.status = PeerStatus::Disconnected;
            m.trust_level = 95;
            m.has_sync_capability = true;
        }).unwrap();

        // Should select peer1 and peer2 (both connected), peer1 first (higher trust)
        let selected = manager.select_sync_peers(2);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0], peer1);
        assert_eq!(selected[1], peer2);
    }
}
