//! Sync Service Manager
//!
//! Wraps `aura_sync::SyncService` for integration with the agent runtime.
//! Provides lifecycle management and configuration for automatic background sync.

use aura_core::effects::indexed::{IndexedFact, IndexedJournalEffects};
use aura_core::effects::PhysicalTimeEffects;
use aura_core::DeviceId;
use aura_sync::services::{Service, SyncService, SyncServiceConfig};
use aura_sync::verification::{MerkleVerifier, VerificationResult};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Configuration for the sync service manager
#[derive(Debug, Clone)]
pub struct SyncManagerConfig {
    /// Enable automatic periodic sync
    pub auto_sync_enabled: bool,

    /// Interval between automatic sync rounds (default: 60s)
    pub auto_sync_interval: Duration,

    /// Maximum concurrent sync sessions
    pub max_concurrent_syncs: usize,

    /// Initial peers to sync with (can be empty if using discovery)
    pub initial_peers: Vec<DeviceId>,

    /// Enable periodic maintenance cleanup
    pub maintenance_enabled: bool,

    /// Interval between maintenance runs
    pub maintenance_interval: Duration,

    /// TTL for stale peer states
    pub peer_state_ttl: Duration,

    /// Maximum tracked peer states before pruning
    pub max_peer_states: usize,
}

impl Default for SyncManagerConfig {
    fn default() -> Self {
        Self {
            auto_sync_enabled: true,
            auto_sync_interval: Duration::from_secs(60),
            max_concurrent_syncs: 5,
            initial_peers: Vec::new(),
            maintenance_enabled: true,
            maintenance_interval: Duration::from_secs(60),
            peer_state_ttl: Duration::from_secs(6 * 60 * 60),
            max_peer_states: 1024,
        }
    }
}

impl SyncManagerConfig {
    /// Create config for testing (shorter intervals)
    pub fn for_testing() -> Self {
        Self {
            auto_sync_enabled: true,
            auto_sync_interval: Duration::from_secs(5),
            max_concurrent_syncs: 3,
            initial_peers: Vec::new(),
            maintenance_enabled: true,
            maintenance_interval: Duration::from_secs(5),
            peer_state_ttl: Duration::from_secs(60),
            max_peer_states: 128,
        }
    }

    /// Create config with auto-sync disabled
    pub fn manual_only() -> Self {
        Self {
            auto_sync_enabled: false,
            auto_sync_interval: Duration::from_secs(60),
            max_concurrent_syncs: 5,
            initial_peers: Vec::new(),
            maintenance_enabled: true,
            maintenance_interval: Duration::from_secs(60),
            peer_state_ttl: Duration::from_secs(6 * 60 * 60),
            max_peer_states: 1024,
        }
    }
}

/// State of the sync service manager
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncManagerState {
    /// Service not yet started
    Stopped,
    /// Service starting up
    Starting,
    /// Service running and actively syncing
    Running,
    /// Service shutting down
    Stopping,
}

/// Manager for background journal synchronization
///
/// Integrates `aura_sync::SyncService` into the agent runtime lifecycle.
/// Handles startup, shutdown, and coordination with other agent services.
#[derive(Clone)]
pub struct SyncServiceManager {
    /// Inner sync service from aura-sync
    service: Arc<RwLock<Option<SyncService>>>,

    /// Configuration
    config: SyncManagerConfig,

    /// Current state
    state: Arc<RwLock<SyncManagerState>>,

    /// Known peers for sync (populated via discovery or configuration)
    peers: Arc<RwLock<Vec<DeviceId>>>,

    /// Handle to background sync task (if auto-sync enabled)
    background_task: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,

    /// Optional Merkle verifier for fact sync (requires indexed journal)
    merkle_verifier: Option<Arc<MerkleVerifier>>,
}

impl SyncServiceManager {
    /// Create a new sync service manager
    pub fn new(config: SyncManagerConfig) -> Self {
        Self {
            service: Arc::new(RwLock::new(None)),
            config: config.clone(),
            state: Arc::new(RwLock::new(SyncManagerState::Stopped)),
            peers: Arc::new(RwLock::new(config.initial_peers)),
            background_task: Arc::new(RwLock::new(None)),
            merkle_verifier: None,
        }
    }

    /// Create a new sync service manager with indexed journal for Merkle verification
    ///
    /// This enables fact sync with cryptographic verification of facts using
    /// Merkle trees and Bloom filters from the indexed journal.
    pub fn with_indexed_journal(
        config: SyncManagerConfig,
        indexed_journal: Arc<dyn IndexedJournalEffects + Send + Sync>,
        time: Arc<dyn PhysicalTimeEffects>,
    ) -> Self {
        Self {
            service: Arc::new(RwLock::new(None)),
            config: config.clone(),
            state: Arc::new(RwLock::new(SyncManagerState::Stopped)),
            peers: Arc::new(RwLock::new(config.initial_peers)),
            background_task: Arc::new(RwLock::new(None)),
            merkle_verifier: Some(Arc::new(MerkleVerifier::new(indexed_journal, time))),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(SyncManagerConfig::default())
    }

    /// Get the current state
    pub async fn state(&self) -> SyncManagerState {
        *self.state.read().await
    }

    /// Check if the service is running
    pub async fn is_running(&self) -> bool {
        *self.state.read().await == SyncManagerState::Running
    }

    /// Start the sync service
    ///
    /// # Arguments
    /// - `time_effects`: Time effects for service initialization
    pub async fn start(
        &self,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
    ) -> Result<(), String> {
        let current_state = *self.state.read().await;
        if current_state == SyncManagerState::Running {
            return Ok(()); // Already running
        }

        *self.state.write().await = SyncManagerState::Starting;

        // Build aura-sync service config from our config
        let sync_config = SyncServiceConfig {
            auto_sync_enabled: self.config.auto_sync_enabled,
            auto_sync_interval: self.config.auto_sync_interval,
            max_concurrent_syncs: self.config.max_concurrent_syncs as u32,
            ..Default::default()
        };

        // Create the underlying sync service
        let now_instant = SyncService::monotonic_now();
        let service = SyncService::new(sync_config, time_effects, now_instant)
            .await
            .map_err(|e| format!("Failed to create sync service: {}", e))?;

        // Start the service
        service
            .start(now_instant)
            .await
            .map_err(|e| format!("Failed to start sync service: {}", e))?;

        *self.service.write().await = Some(service);
        *self.state.write().await = SyncManagerState::Running;

        tracing::info!("Sync service manager started");
        Ok(())
    }

    /// Stop the sync service
    pub async fn stop(&self) -> Result<(), String> {
        let current_state = *self.state.read().await;
        if current_state == SyncManagerState::Stopped {
            return Ok(()); // Already stopped
        }

        *self.state.write().await = SyncManagerState::Stopping;

        // Cancel background task if running
        if let Some(handle) = self.background_task.write().await.take() {
            handle.abort();
        }

        // Stop the underlying service
        if let Some(service) = self.service.read().await.as_ref() {
            let now_instant = SyncService::monotonic_now();
            service
                .stop(now_instant)
                .await
                .map_err(|e| format!("Failed to stop sync service: {}", e))?;
        }

        *self.service.write().await = None;
        *self.state.write().await = SyncManagerState::Stopped;

        tracing::info!("Sync service manager stopped");
        Ok(())
    }

    /// Start background maintenance task for pruning long-lived state.
    pub fn start_maintenance_task(
        &self,
        tasks: Arc<crate::runtime::services::RuntimeTaskRegistry>,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
    ) {
        if !self.config.maintenance_enabled {
            tracing::debug!("Sync maintenance task disabled by configuration");
            return;
        }

        let interval = self.config.maintenance_interval;
        let peer_state_ttl = self.config.peer_state_ttl;
        let max_peer_states = self.config.max_peer_states;
        let manager = self.clone();

        tasks.spawn_interval_until(interval, move || {
            let manager = manager.clone();
            let time_effects = time_effects.clone();
            async move {
                let state = manager.state.read().await;
                if matches!(
                    *state,
                    SyncManagerState::Stopped | SyncManagerState::Stopping
                ) {
                    return true;
                }

                let now_ms = match time_effects.physical_time().await {
                    Ok(t) => t.ts_ms,
                    Err(e) => {
                        tracing::warn!("Sync maintenance: failed to get time: {}", e);
                        return true;
                    }
                };

                if let Some(service) = manager.service.read().await.as_ref() {
                    if let Err(e) = service
                        .maintenance_cleanup(
                            now_ms,
                            peer_state_ttl.as_millis() as u64,
                            max_peer_states,
                        )
                        .await
                    {
                        tracing::warn!("Sync maintenance failed: {}", e);
                    }
                }

                true
            }
        });
    }

    /// Perform a manual sync with specific peers
    ///
    /// # Arguments
    /// - `effects`: Effect system providing journal, network, and time capabilities
    /// - `peers`: List of peers to sync with
    pub async fn sync_with_peers<E>(&self, effects: &E, peers: Vec<DeviceId>) -> Result<(), String>
    where
        E: aura_core::effects::JournalEffects
            + aura_core::effects::NetworkEffects
            + aura_core::effects::PhysicalTimeEffects
            + Send
            + Sync,
    {
        let service = self.service.read().await;
        let service = service.as_ref().ok_or("Sync service not started")?;

        let now_instant = SyncService::monotonic_now();
        service
            .sync_with_peers(effects, peers, now_instant)
            .await
            .map_err(|e| format!("Sync failed: {}", e))
    }

    /// Add a peer to the known peers list
    pub async fn add_peer(&self, peer: DeviceId) {
        let mut peers = self.peers.write().await;
        if !peers.contains(&peer) {
            peers.push(peer);
            tracing::debug!("Added peer {} to sync manager", peer);
        }
    }

    /// Remove a peer from the known peers list
    pub async fn remove_peer(&self, peer: &DeviceId) {
        let mut peers = self.peers.write().await;
        peers.retain(|p| p != peer);
        tracing::debug!("Removed peer {} from sync manager", peer);
    }

    /// Get the list of known peers
    pub async fn peers(&self) -> Vec<DeviceId> {
        self.peers.read().await.clone()
    }

    /// Get service health information
    pub async fn health(&self) -> Option<aura_sync::services::SyncServiceHealth> {
        self.service.read().await.as_ref().map(|s| s.get_health())
    }

    /// Get service metrics
    pub async fn metrics(&self) -> Option<aura_sync::services::ServiceMetrics> {
        self.service.read().await.as_ref().map(|s| s.get_metrics())
    }

    /// Get the configuration
    pub fn config(&self) -> &SyncManagerConfig {
        &self.config
    }

    // =========================================================================
    // Merkle Verification Methods
    // =========================================================================

    /// Check if Merkle verification is available
    ///
    /// Returns `true` if the manager was created with an indexed journal,
    /// enabling cryptographic fact verification.
    pub fn has_merkle_verification(&self) -> bool {
        self.merkle_verifier.is_some()
    }

    /// Get the local Merkle root for exchange with peers
    ///
    /// Returns `None` if Merkle verification is not enabled (no indexed journal).
    /// The root represents the current state of the local fact journal and can
    /// be compared with remote roots to determine if synchronization is needed.
    pub async fn local_merkle_root(&self) -> Option<[u8; 32]> {
        if let Some(ref verifier) = self.merkle_verifier {
            verifier.local_merkle_root().await.ok()
        } else {
            None
        }
    }

    /// Verify incoming facts against the local Merkle tree
    ///
    /// Returns `None` if Merkle verification is not enabled.
    /// Otherwise returns the verification result containing:
    /// - `verified`: Facts that passed verification
    /// - `rejected`: Facts that failed verification with reasons
    /// - `merkle_root`: Current local Merkle root after verification
    pub async fn verify_facts(
        &self,
        facts: Vec<IndexedFact>,
        claimed_root: [u8; 32],
    ) -> Option<VerificationResult> {
        if let Some(ref verifier) = self.merkle_verifier {
            verifier
                .verify_incoming_facts(facts, claimed_root)
                .await
                .ok()
        } else {
            None
        }
    }

    /// Get the internal Merkle verifier reference
    ///
    /// Returns `None` if Merkle verification is not enabled.
    /// Use this for direct access to verification operations like
    /// `compare_roots()` or `local_bloom_filter()`.
    pub fn merkle_verifier(&self) -> Option<&Arc<MerkleVerifier>> {
        self.merkle_verifier.as_ref()
    }
}

impl Default for SyncServiceManager {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::domain::journal::FactValue;
    use aura_core::effects::indexed::{FactId, FactStreamReceiver, IndexStats};
    use aura_core::effects::{BloomConfig, BloomFilter};
    use aura_core::AuthorityId;
    use aura_effects::time::PhysicalTimeHandler;
    use std::sync::Mutex;

    /// Mock indexed journal for testing
    struct MockIndexedJournal {
        root: Mutex<[u8; 32]>,
        facts: Mutex<Vec<IndexedFact>>,
    }

    impl MockIndexedJournal {
        fn new(root: [u8; 32]) -> Self {
            Self {
                root: Mutex::new(root),
                facts: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl IndexedJournalEffects for MockIndexedJournal {
        fn watch_facts(&self) -> Box<dyn FactStreamReceiver> {
            panic!("Not implemented for mock")
        }

        async fn facts_by_predicate(
            &self,
            _predicate: &str,
        ) -> Result<Vec<IndexedFact>, aura_core::AuraError> {
            Ok(Vec::new())
        }

        async fn facts_by_authority(
            &self,
            _authority: &AuthorityId,
        ) -> Result<Vec<IndexedFact>, aura_core::AuraError> {
            Ok(Vec::new())
        }

        async fn facts_in_range(
            &self,
            _start: aura_core::time::TimeStamp,
            _end: aura_core::time::TimeStamp,
        ) -> Result<Vec<IndexedFact>, aura_core::AuraError> {
            Ok(Vec::new())
        }

        async fn all_facts(&self) -> Result<Vec<IndexedFact>, aura_core::AuraError> {
            Ok(self.facts.lock().unwrap().clone())
        }

        fn might_contain(&self, _predicate: &str, _value: &FactValue) -> bool {
            false
        }

        async fn merkle_root(&self) -> Result<[u8; 32], aura_core::AuraError> {
            Ok(*self.root.lock().unwrap())
        }

        async fn verify_fact_inclusion(
            &self,
            fact: &IndexedFact,
        ) -> Result<bool, aura_core::AuraError> {
            let facts = self.facts.lock().unwrap();
            Ok(facts.iter().any(|f| f.id == fact.id))
        }

        async fn get_bloom_filter(&self) -> Result<BloomFilter, aura_core::AuraError> {
            BloomFilter::new(BloomConfig::for_sync(100))
        }

        async fn index_stats(&self) -> Result<IndexStats, aura_core::AuraError> {
            let facts = self.facts.lock().unwrap();
            Ok(IndexStats {
                fact_count: facts.len() as u64,
                predicate_count: 1,
                authority_count: 1,
                bloom_fp_rate: 0.01,
                merkle_depth: 10,
            })
        }
    }

    #[tokio::test]
    async fn test_sync_manager_creation() {
        let config = SyncManagerConfig::for_testing();
        let manager = SyncServiceManager::new(config);

        assert_eq!(manager.state().await, SyncManagerState::Stopped);
        assert!(!manager.is_running().await);
    }

    #[tokio::test]
    async fn test_sync_manager_lifecycle() {
        let config = SyncManagerConfig::for_testing();
        let manager = SyncServiceManager::new(config);
        let time_effects = Arc::new(PhysicalTimeHandler::new());

        // Start
        manager.start(time_effects).await.unwrap();
        assert!(manager.is_running().await);

        // Stop
        manager.stop().await.unwrap();
        assert!(!manager.is_running().await);
    }

    #[tokio::test]
    async fn test_sync_manager_peer_management() {
        let manager = SyncServiceManager::with_defaults();

        let peer1 = DeviceId::new_from_entropy([1u8; 32]);
        let peer2 = DeviceId::new_from_entropy([2u8; 32]);

        // Add peers
        manager.add_peer(peer1).await;
        manager.add_peer(peer2).await;

        let peers = manager.peers().await;
        assert_eq!(peers.len(), 2);
        assert!(peers.contains(&peer1));
        assert!(peers.contains(&peer2));

        // Remove peer
        manager.remove_peer(&peer1).await;
        let peers = manager.peers().await;
        assert_eq!(peers.len(), 1);
        assert!(!peers.contains(&peer1));
        assert!(peers.contains(&peer2));
    }

    #[tokio::test]
    async fn test_sync_manager_health_when_not_running() {
        let manager = SyncServiceManager::with_defaults();

        // Health should be None when not running
        assert!(manager.health().await.is_none());
    }

    #[tokio::test]
    async fn test_sync_manager_health_when_running() {
        let manager = SyncServiceManager::new(SyncManagerConfig::for_testing());
        let time_effects = Arc::new(PhysicalTimeHandler::new());

        manager.start(time_effects).await.unwrap();

        // Health should be available when running
        let health = manager.health().await;
        assert!(health.is_some());

        manager.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_sync_manager_without_merkle_verification() {
        let manager = SyncServiceManager::new(SyncManagerConfig::for_testing());

        // Manager without indexed journal should not have Merkle verification
        assert!(!manager.has_merkle_verification());
        assert!(manager.local_merkle_root().await.is_none());
        assert!(manager.verify_facts(vec![], [0u8; 32]).await.is_none());
        assert!(manager.merkle_verifier().is_none());
    }

    #[tokio::test]
    async fn test_sync_manager_with_merkle_verification() {
        let root = [42u8; 32];
        let journal = Arc::new(MockIndexedJournal::new(root));
        let time = Arc::new(PhysicalTimeHandler::new());
        let manager = SyncServiceManager::with_indexed_journal(
            SyncManagerConfig::for_testing(),
            journal,
            time,
        );

        // Manager with indexed journal should have Merkle verification
        assert!(manager.has_merkle_verification());
        assert!(manager.merkle_verifier().is_some());

        // Should return local Merkle root
        let local_root = manager.local_merkle_root().await;
        assert_eq!(local_root, Some(root));
    }

    #[tokio::test]
    async fn test_sync_manager_verify_facts() {
        let root = [42u8; 32];
        let journal = Arc::new(MockIndexedJournal::new(root));
        let time = Arc::new(PhysicalTimeHandler::new());
        let manager = SyncServiceManager::with_indexed_journal(
            SyncManagerConfig::for_testing(),
            journal,
            time,
        );

        // Create test fact
        let fact = IndexedFact {
            id: FactId(1),
            predicate: "test".to_string(),
            value: FactValue::String("test_value".to_string()),
            authority: None,
            timestamp: None,
        };

        // Verify facts returns a result
        let result = manager.verify_facts(vec![fact], root).await;
        assert!(result.is_some());

        let result = result.unwrap();
        // New fact should be accepted for merge
        assert_eq!(result.verified.len(), 1);
        assert!(result.rejected.is_empty());
    }
}
