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

use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;

use serde::{Deserialize, Serialize};

use aura_core::DeviceId;
use crate::core::{SyncError, SyncResult, SessionManager, MetricsCollector};
use crate::infrastructure::{PeerManager, PeerDiscoveryConfig, RateLimiter, RateLimitConfig};
use crate::protocols::{JournalSyncProtocol, JournalSyncConfig, AntiEntropyProtocol};
use super::{Service, HealthStatus, HealthCheck, ServiceState, ServiceMetrics};

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
    session_manager: Arc<RwLock<SessionManager>>,

    /// Journal sync protocol
    journal_sync: Arc<RwLock<JournalSyncProtocol>>,

    /// Metrics collector
    metrics: Arc<RwLock<MetricsCollector>>,

    /// Service start time
    started_at: Arc<RwLock<Option<Instant>>>,
}

impl SyncService {
    /// Create a new sync service
    pub fn new(config: SyncServiceConfig) -> SyncResult<Self> {
        let peer_manager = PeerManager::new(config.peer_discovery.clone());
        let rate_limiter = RateLimiter::new(config.rate_limit.clone());
        let session_manager = SessionManager::new(Default::default());
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

        let session_stats = self.session_manager.read().statistics();
        let peer_stats = self.peer_manager.read().statistics();

        let uptime = self.started_at.read()
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
            uptime_seconds: self.started_at.read()
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0),
            requests_processed: 0, // TODO: From metrics
            errors_encountered: 0, // TODO: From metrics
            avg_latency_ms: 0.0,   // TODO: From metrics
            last_operation_at: None,
        }
    }
}

#[async_trait::async_trait]
impl Service for SyncService {
    async fn start(&self) -> SyncResult<()> {
        let mut state = self.state.write();
        if *state == ServiceState::Running {
            return Err(SyncError::Service("Service already running".to_string()));
        }

        *state = ServiceState::Starting;
        *self.started_at.write() = Some(Instant::now());

        // TODO: Start background tasks for auto-sync

        *state = ServiceState::Running;
        Ok(())
    }

    async fn stop(&self) -> SyncResult<()> {
        let mut state = self.state.write();
        if *state == ServiceState::Stopped {
            return Ok(());
        }

        *state = ServiceState::Stopping;

        // TODO: Stop background tasks
        // TODO: Wait for active sessions to complete

        *state = ServiceState::Stopped;
        Ok(())
    }

    async fn health_check(&self) -> SyncResult<HealthCheck> {
        let health = self.get_health();
        let mut details = std::collections::HashMap::new();

        details.insert("active_sessions".to_string(), health.active_sessions.to_string());
        details.insert("tracked_peers".to_string(), health.tracked_peers.to_string());
        details.insert("available_peers".to_string(), health.available_peers.to_string());
        details.insert("uptime".to_string(), format!("{}s", health.uptime.as_secs()));

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
        self.config.get_or_insert_with(Default::default).auto_sync_enabled = enabled;
        self
    }

    /// Set auto-sync interval
    pub fn with_sync_interval(mut self, interval: Duration) -> Self {
        self.config.get_or_insert_with(Default::default).auto_sync_interval = interval;
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

        service.start().await.unwrap();
        assert!(service.is_running());

        service.stop().await.unwrap();
        assert!(!service.is_running());
    }

    #[tokio::test]
    async fn test_sync_service_health_check() {
        let service = SyncService::builder().build().unwrap();
        service.start().await.unwrap();

        let health = service.health_check().await.unwrap();
        assert_eq!(health.status, HealthStatus::Healthy);
        assert!(health.details.contains_key("active_sessions"));
    }
}
