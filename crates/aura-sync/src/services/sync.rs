//! Synchronization service
//!
//! Provides high-level journal synchronization service that orchestrates
//! anti-entropy protocols, peer management, and session coordination.
//!
//! # Architecture
//!
//! The sync service:
//! - Uses `JournalSyncProtocol` and `AntiEntropyProtocol` from protocols/
//! - Manages peers via `PeerManager` from infrastructure/
//! - Enforces rate limits via `RateLimiter` from infrastructure/
//! - Tracks sessions via `SessionManager` from core/
//!
//! # Usage
//!
//! ```rust,no_run
//! use aura_sync::services::{SyncService, SyncServiceConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = SyncServiceConfig::default();
//! let service = SyncService::new(config)?;
//!
//! // Sync with specific peers
//! let peers = vec![peer1, peer2, peer3];
//! service.sync_with_peers(peers).await?;
//! # Ok(())
//! # }
//! ```

// TODO: Refactor to use TimeEffects for timing. Current Instant::now() usage is for
// sync timing metrics and should be replaced with effect system integration.
#![allow(clippy::disallowed_methods)]

use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use serde::{Deserialize, Serialize};

use super::{HealthCheck, HealthStatus, Service, ServiceMetrics, ServiceState};
use crate::core::{sync_session_error, MetricsCollector, SessionManager, SyncResult};
use crate::infrastructure::{PeerDiscoveryConfig, PeerManager, RateLimitConfig, RateLimiter};
use crate::protocols::{JournalSyncConfig, JournalSyncProtocol};
use aura_core::DeviceId;

// =============================================================================
// Configuration
// =============================================================================

/// Sync service configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncServiceConfig {
    /// Enable automatic periodic sync
    pub auto_sync_enabled: bool,

    /// Interval between automatic sync rounds
    pub auto_sync_interval: Duration,

    /// Peer discovery configuration
    pub peer_discovery: PeerDiscoveryConfig,

    /// Rate limiting configuration
    pub rate_limit: RateLimitConfig,

    /// Journal sync configuration
    pub journal_sync: JournalSyncConfig,

    /// Maximum concurrent sync sessions
    pub max_concurrent_syncs: usize,
}

impl Default for SyncServiceConfig {
    fn default() -> Self {
        Self {
            auto_sync_enabled: true,
            auto_sync_interval: Duration::from_secs(60),
            peer_discovery: PeerDiscoveryConfig::default(),
            rate_limit: RateLimitConfig::default(),
            journal_sync: JournalSyncConfig::default(),
            max_concurrent_syncs: 5,
        }
    }
}

// =============================================================================
// Service Health
// =============================================================================

/// Sync service health information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncServiceHealth {
    /// Overall health status
    pub status: HealthStatus,

    /// Number of active sync sessions
    pub active_sessions: usize,

    /// Number of tracked peers
    pub tracked_peers: usize,

    /// Number of available peers
    pub available_peers: usize,

    /// Last sync timestamp
    pub last_sync: Option<u64>,

    /// Service uptime
    pub uptime: Duration,
}

// =============================================================================
// Sync Service
// =============================================================================

/// High-level synchronization service
///
/// Orchestrates journal synchronization across multiple peers using
/// unified protocols and infrastructure from Phases 2 and 3.
pub struct SyncService {
    /// Configuration
    config: SyncServiceConfig,

    /// Service state
    state: Arc<RwLock<ServiceState>>,

    /// Peer manager
    peer_manager: Arc<RwLock<PeerManager>>,

    /// Rate limiter
    rate_limiter: Arc<RwLock<RateLimiter>>,

    /// Session manager
    session_manager: Arc<RwLock<SessionManager<serde_json::Value>>>,

    /// Journal sync protocol
    journal_sync: Arc<RwLock<JournalSyncProtocol>>,

    /// Metrics collector
    metrics: Arc<RwLock<MetricsCollector>>,

    /// Service start time
    started_at: Arc<RwLock<Option<Instant>>>,
    /// Background task shutdown signal
    shutdown_tx: Arc<RwLock<Option<watch::Sender<bool>>>>,
    /// Background task handles
    task_handles: Arc<RwLock<Vec<JoinHandle<()>>>>,
}

impl SyncService {
    /// Create a new sync service
    pub fn new(config: SyncServiceConfig) -> SyncResult<Self> {
        let peer_manager = PeerManager::new(config.peer_discovery.clone());
        // TODO: Should obtain Instant via TimeEffects
        #[allow(clippy::disallowed_methods)]
        let now_instant = std::time::Instant::now();
        let rate_limiter = RateLimiter::new(config.rate_limit.clone(), now_instant);
        // TODO: Obtain actual timestamp via TimeEffects
        let now = 0u64;
        let session_manager = SessionManager::new(Default::default(), now);
        let journal_sync = JournalSyncProtocol::new(config.journal_sync.clone());
        let metrics = MetricsCollector::new();

        Ok(Self {
            config,
            state: Arc::new(RwLock::new(ServiceState::Stopped)),
            peer_manager: Arc::new(RwLock::new(peer_manager)),
            rate_limiter: Arc::new(RwLock::new(rate_limiter)),
            session_manager: Arc::new(RwLock::new(session_manager)),
            journal_sync: Arc::new(RwLock::new(journal_sync)),
            metrics: Arc::new(RwLock::new(metrics)),
            started_at: Arc::new(RwLock::new(None)),
            shutdown_tx: Arc::new(RwLock::new(None)),
            task_handles: Arc::new(RwLock::new(Vec::new())),
        })
    }

    /// Create a new sync service with builder
    pub fn builder() -> SyncServiceBuilder {
        SyncServiceBuilder::default()
    }

    /// Synchronize with specific peers
    pub async fn sync_with_peers<E>(&self, _effects: &E, peers: Vec<DeviceId>) -> SyncResult<()>
    where
        E: Send + Sync,
    {
        if peers.is_empty() {
            return Ok(());
        }

        // TODO: Implement using journal_sync protocol and infrastructure
        // This would:
        // 1. Check rate limits for each peer
        // 2. Create sessions for each peer
        // 3. Execute journal sync protocol
        // 4. Update metrics
        // 5. Clean up sessions

        Ok(())
    }

    /// Discover and sync with available peers
    pub async fn discover_and_sync<E>(&self, _effects: &E) -> SyncResult<()>
    where
        E: Send + Sync,
    {
        // TODO: Implement using peer_manager and journal_sync
        // This would:
        // 1. Discover peers via peer_manager
        // 2. Select best peers based on scoring
        // 3. Sync with selected peers
        // 4. Update peer states

        Ok(())
    }

    /// Get service health
    pub fn get_health(&self) -> SyncServiceHealth {
        let state = *self.state.read();
        let status = match state {
            ServiceState::Running => HealthStatus::Healthy,
            ServiceState::Starting => HealthStatus::Starting,
            ServiceState::Stopping => HealthStatus::Stopping,
            ServiceState::Stopped | ServiceState::Failed => HealthStatus::Unhealthy,
        };

        let session_stats = self.session_manager.read().get_statistics();
        let peer_stats = self.peer_manager.read().statistics();

        let uptime = self
            .started_at
            .read()
            .map(|t| t.elapsed())
            .unwrap_or(Duration::ZERO);

        SyncServiceHealth {
            status,
            active_sessions: session_stats.active_sessions,
            tracked_peers: peer_stats.total_tracked,
            available_peers: peer_stats.available_peers,
            last_sync: None, // TODO: Track from metrics
            uptime,
        }
    }

    /// Get service metrics
    pub fn get_metrics(&self) -> ServiceMetrics {
        let metrics = self.metrics.read();

        ServiceMetrics {
            uptime_seconds: self
                .started_at
                .read()
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0),
            requests_processed: 0, // TODO: From metrics
            errors_encountered: 0, // TODO: From metrics
            avg_latency_ms: 0.0,   // TODO: From metrics
            last_operation_at: None,
        }
    }

    // =============================================================================
    // Background Task Management
    // =============================================================================

    /// Start the automatic synchronization background task
    async fn start_auto_sync_task(&self) -> SyncResult<()> {
        // Create shutdown signal channel
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        *self.shutdown_tx.write() = Some(shutdown_tx);

        // Clone necessary data for the task
        let interval = self.config.auto_sync_interval;
        let peer_manager = Arc::clone(&self.peer_manager);
        let session_manager = Arc::clone(&self.session_manager);
        let journal_sync = Arc::clone(&self.journal_sync);
        let rate_limiter = Arc::clone(&self.rate_limiter);
        let max_concurrent = self.config.max_concurrent_syncs;

        // Spawn the background auto-sync task
        let handle = tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            interval_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    // Check for shutdown signal
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            break;
                        }
                    }

                    // Handle interval tick for auto-sync
                    _ = interval_timer.tick() => {
                        if let Err(e) = Self::perform_auto_sync(
                            &peer_manager,
                            &session_manager,
                            &journal_sync,
                            &rate_limiter,
                            max_concurrent,
                        ).await {
                            eprintln!("Auto-sync failed: {}", e);
                        }
                    }
                }
            }
        });

        // Store the task handle
        self.task_handles.write().push(handle);

        Ok(())
    }

    /// Perform one round of automatic synchronization
    async fn perform_auto_sync(
        peer_manager: &Arc<RwLock<PeerManager>>,
        session_manager: &Arc<RwLock<SessionManager<serde_json::Value>>>,
        journal_sync: &Arc<RwLock<JournalSyncProtocol>>,
        rate_limiter: &Arc<RwLock<RateLimiter>>,
        max_concurrent: usize,
    ) -> SyncResult<()> {
        // Get available peers for synchronization
        let peers = {
            let manager = peer_manager.read();
            manager.select_sync_peers(max_concurrent)
        };

        if peers.is_empty() {
            return Ok(());
        }

        // Check rate limits before proceeding
        {
            let mut limiter = rate_limiter.write();
            #[allow(clippy::disallowed_methods)]
            let now = std::time::Instant::now(); // TODO: Should use TimeEffects
            for peer in &peers {
                match limiter.check_rate_limit(*peer, 100, now) {
                    aura_core::RateLimitResult::Allowed { .. } => continue,
                    aura_core::RateLimitResult::Denied { .. } => {
                        return Ok(()); // Rate limited, skip this round
                    }
                }
            }
        }

        // Get current active session count
        let active_sessions = {
            let manager = session_manager.read();
            manager.get_statistics().active_sessions
        };

        // Respect max concurrent limit
        if active_sessions >= max_concurrent {
            return Ok(());
        }

        // TODO: Implement actual peer synchronization using journal_sync
        // This would involve:
        // 1. Select best peers based on priority and health scores
        // 2. Initiate sync sessions with selected peers
        // 3. Execute anti-entropy and journal sync protocols
        // 4. Update peer scores based on sync results
        // 5. Track metrics and update session states

        // For now, just log that auto-sync is working
        println!("Auto-sync: checking {} available peers", peers.len());

        Ok(())
    }

    /// Stop all background tasks
    async fn stop_background_tasks(&self) -> SyncResult<()> {
        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.write().take() {
            let _ = tx.send(true);
        }

        // Wait for all tasks to complete
        let handles = {
            let mut task_handles = self.task_handles.write();
            std::mem::take(&mut *task_handles)
        };

        for handle in handles {
            let _ = handle.await;
        }

        Ok(())
    }

    /// Wait for active sessions to complete with timeout
    async fn wait_for_sessions_to_complete(&self) -> SyncResult<()> {
        let timeout = Duration::from_secs(30); // 30 second timeout
        let check_interval = Duration::from_millis(100);
        let start = Instant::now();

        while start.elapsed() < timeout {
            let active_sessions = {
                let manager = self.session_manager.read();
                manager.get_statistics().active_sessions
            };

            if active_sessions == 0 {
                break;
            }

            tokio::time::sleep(check_interval).await;
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl Service for SyncService {
    async fn start(&self, now: Instant) -> SyncResult<()> {
        {
            let mut state = self.state.write();
            if *state == ServiceState::Running {
                return Err(sync_session_error("Service already running"));
            }

            *state = ServiceState::Starting;
            *self.started_at.write() = Some(now);
        } // Drop state lock before await

        // Start background auto-sync task if enabled
        if self.config.auto_sync_enabled {
            self.start_auto_sync_task().await?;
        }

        {
            let mut state = self.state.write();
            *state = ServiceState::Running;
        }
        Ok(())
    }

    async fn stop(&self) -> SyncResult<()> {
        {
            let mut state = self.state.write();
            if *state == ServiceState::Stopped {
                return Ok(());
            }

            *state = ServiceState::Stopping;
        } // Drop state lock before await

        // Stop background tasks
        self.stop_background_tasks().await?;

        // Wait for active sessions to complete with timeout
        self.wait_for_sessions_to_complete().await?;

        {
            let mut state = self.state.write();
            *state = ServiceState::Stopped;
        }
        Ok(())
    }

    async fn health_check(&self) -> SyncResult<HealthCheck> {
        let health = self.get_health();
        let mut details = std::collections::HashMap::new();

        details.insert(
            "active_sessions".to_string(),
            health.active_sessions.to_string(),
        );
        details.insert(
            "tracked_peers".to_string(),
            health.tracked_peers.to_string(),
        );
        details.insert(
            "available_peers".to_string(),
            health.available_peers.to_string(),
        );
        details.insert(
            "uptime".to_string(),
            format!("{}s", health.uptime.as_secs()),
        );

        Ok(HealthCheck {
            status: health.status,
            message: Some("Sync service operational".to_string()),
            checked_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            details,
        })
    }

    fn name(&self) -> &str {
        "SyncService"
    }

    fn is_running(&self) -> bool {
        *self.state.read() == ServiceState::Running
    }
}

// =============================================================================
// Builder
// =============================================================================

/// Builder for SyncService
#[derive(Default)]
pub struct SyncServiceBuilder {
    config: Option<SyncServiceConfig>,
}

impl SyncServiceBuilder {
    /// Set configuration
    pub fn with_config(mut self, config: SyncServiceConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Set auto-sync enabled
    pub fn with_auto_sync(mut self, enabled: bool) -> Self {
        self.config
            .get_or_insert_with(Default::default)
            .auto_sync_enabled = enabled;
        self
    }

    /// Set auto-sync interval
    pub fn with_sync_interval(mut self, interval: Duration) -> Self {
        self.config
            .get_or_insert_with(Default::default)
            .auto_sync_interval = interval;
        self
    }

    /// Build the service
    pub fn build(self) -> SyncResult<SyncService> {
        let config = self.config.unwrap_or_default();
        SyncService::new(config)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_service_creation() {
        let config = SyncServiceConfig::default();
        let service = SyncService::new(config).unwrap();

        assert_eq!(service.name(), "SyncService");
        assert!(!service.is_running());
    }

    #[test]
    fn test_sync_service_builder() {
        let service = SyncService::builder()
            .with_auto_sync(true)
            .with_sync_interval(Duration::from_secs(30))
            .build()
            .unwrap();

        assert!(service.config.auto_sync_enabled);
        assert_eq!(service.config.auto_sync_interval, Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_sync_service_lifecycle() {
        let service = SyncService::builder().build().unwrap();

        assert!(!service.is_running());

        #[allow(clippy::disallowed_methods)]
        let now = std::time::Instant::now();
        service.start(now).await.unwrap();
        assert!(service.is_running());

        service.stop().await.unwrap();
        assert!(!service.is_running());
    }

    #[tokio::test]
    async fn test_sync_service_health_check() {
        let service = SyncService::builder().build().unwrap();
        #[allow(clippy::disallowed_methods)]
        let now = std::time::Instant::now();
        service.start(now).await.unwrap();

        let health = service.health_check().await.unwrap();
        assert_eq!(health.status, HealthStatus::Healthy);
        assert!(health.details.contains_key("active_sessions"));
    }
}
