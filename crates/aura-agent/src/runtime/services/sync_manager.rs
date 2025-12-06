//! Sync Service Manager
//!
//! Wraps `aura_sync::SyncService` for integration with the agent runtime.
//! Provides lifecycle management and configuration for automatic background sync.

use aura_core::effects::{PhysicalTimeEffects, TimeEffects};
use aura_core::DeviceId;
use aura_sync::services::{Service, SyncService, SyncServiceConfig};
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
}

impl Default for SyncManagerConfig {
    fn default() -> Self {
        Self {
            auto_sync_enabled: true,
            auto_sync_interval: Duration::from_secs(60),
            max_concurrent_syncs: 5,
            initial_peers: Vec::new(),
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
        }
    }

    /// Create config with auto-sync disabled
    pub fn manual_only() -> Self {
        Self {
            auto_sync_enabled: false,
            auto_sync_interval: Duration::from_secs(60),
            max_concurrent_syncs: 5,
            initial_peers: Vec::new(),
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
    pub async fn start<T: PhysicalTimeEffects + TimeEffects + Send + Sync>(
        &self,
        time_effects: &T,
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
            max_concurrent_syncs: self.config.max_concurrent_syncs,
            ..Default::default()
        };

        // Create the underlying sync service
        let now_instant = SyncService::monotonic_now();
        let service = SyncService::new_with_time_effects(sync_config, time_effects, now_instant)
            .await
            .map_err(|e| format!("Failed to create sync service: {}", e))?;

        // Start the service
        service
            .start_with_time_effects(time_effects, now_instant)
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
}

impl Default for SyncServiceManager {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_effects::time::PhysicalTimeHandler;

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
        let time_effects = PhysicalTimeHandler::new();

        // Start
        manager.start(&time_effects).await.unwrap();
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
        let time_effects = PhysicalTimeHandler::new();

        manager.start(&time_effects).await.unwrap();

        // Health should be available when running
        let health = manager.health().await;
        assert!(health.is_some());

        manager.stop().await.unwrap();
    }
}
