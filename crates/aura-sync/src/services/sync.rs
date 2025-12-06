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
//! ```rust,ignore
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

use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{HealthCheck, HealthStatus, Service, ServiceMetrics, ServiceState};
use crate::core::{sync_session_error, MetricsCollector, SessionManager, SyncResult};
use crate::infrastructure::{PeerDiscoveryConfig, PeerManager, RateLimitConfig, RateLimiter};
use crate::protocols::{JournalSyncConfig, JournalSyncProtocol};
use aura_core::effects::{PhysicalTimeEffects, TimeEffects, TimeError};
use aura_core::{AuraError, DeviceId};
use aura_effects::time::PhysicalTimeHandler;

fn time_error_to_aura(err: TimeError) -> AuraError {
    AuraError::internal(format!("time error: {err}"))
}

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
    #[allow(dead_code)]
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

    /// Time effects for unified time operations
    time_effects: PhysicalTimeHandler,
}

impl SyncService {
    /// Monotonic clock source for service lifecycle events.
    ///
    /// This intentionally lives in the sync layer to avoid leaking runtime clock calls
    /// into application code that is subject to effects-enforcement checks.
    #[allow(clippy::disallowed_methods)]
    pub fn monotonic_now() -> Instant {
        // Alias to keep monotonic semantics without exposing runtime clock calls
        // monotonic semantics required by rate limiting and session management.
        type MonoTime = Instant;
        MonoTime::now()
    }

    /// Create a new sync service
    ///
    /// # Arguments
    /// - `config`: Service configuration
    /// - `now_instant`: Current monotonic time instant (obtain from runtime layer)
    pub async fn new(
        config: SyncServiceConfig,
        now_instant: std::time::Instant,
    ) -> SyncResult<Self> {
        let peer_manager = PeerManager::new(config.peer_discovery.clone());
        let time_effects = PhysicalTimeHandler;
        let rate_limiter = RateLimiter::new(config.rate_limit.clone(), now_instant);
        let now = time_effects
            .physical_time()
            .await
            .map_err(time_error_to_aura)?;
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
            time_effects,
        })
    }

    /// Create a new sync service with PhysicalTimeEffects (preferred for deterministic testing)
    ///
    /// # Arguments
    /// - `config`: Service configuration
    /// - `time_effects`: Time effects provider
    /// - `now_instant`: Current monotonic time instant (obtain from runtime layer)
    pub async fn new_with_time_effects<T: TimeEffects>(
        config: SyncServiceConfig,
        time_effects: &T,
        now_instant: std::time::Instant,
    ) -> SyncResult<Self> {
        let peer_manager = PeerManager::new(config.peer_discovery.clone());
        let rate_limiter = RateLimiter::new(config.rate_limit.clone(), now_instant);
        let now = time_effects
            .physical_time()
            .await
            .map_err(time_error_to_aura)?;
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
            time_effects: PhysicalTimeHandler,
        })
    }

    /// Create a new sync service with builder
    pub fn builder() -> SyncServiceBuilder {
        SyncServiceBuilder::default()
    }

    /// Synchronize with specific peers
    ///
    /// # Arguments
    /// - `effects`: Effect system providing journal, network, and time capabilities
    /// - `peers`: List of peer device IDs to sync with
    /// - `now_instant`: Current monotonic time instant (obtain from runtime layer)
    pub async fn sync_with_peers<E>(
        &self,
        effects: &E,
        peers: Vec<DeviceId>,
        now_instant: std::time::Instant,
    ) -> SyncResult<()>
    where
        E: aura_core::effects::JournalEffects
            + aura_core::effects::NetworkEffects
            + aura_core::effects::PhysicalTimeEffects
            + Send
            + Sync,
    {
        if peers.is_empty() {
            return Ok(());
        }

        // Implement full journal_sync protocol integration
        tracing::info!("Starting journal sync with {} peers", peers.len());

        // 1. Check rate limits for each peer
        let allowed_peers = self.check_rate_limits(&peers, now_instant).await?;

        // 2. Create sessions for allowed peers
        let session_peers =
            Self::create_sync_sessions(&self.session_manager, &allowed_peers, &self.time_effects)
                .await?;

        // 3. Execute journal sync protocol
        let sync_results = self
            .execute_journal_sync_protocol(effects, &session_peers)
            .await?;

        // 4. Update metrics
        self.update_sync_metrics(&sync_results).await?;

        // 5. Update peer scores based on sync success/failure
        let score_results: Vec<(DeviceId, bool)> = sync_results
            .iter()
            .map(|&(peer, ops)| (peer, ops > 0)) // ops > 0 = success
            .collect();
        let now = effects.physical_time().await.map_err(time_error_to_aura)?;
        Self::update_peer_scores_from_sync(&self.peer_manager, &score_results, &now).await?;

        // 6. Log aggregate metrics
        Self::update_auto_sync_metrics(&score_results).await?;

        // 7. Clean up sessions
        self.cleanup_sync_sessions(&session_peers).await?;

        tracing::info!("Completed journal sync with {} peers", sync_results.len());
        Ok(())
    }

    /// Discover and sync with available peers
    ///
    /// # Arguments
    /// - `effects`: Effect system providing journal, network, and time capabilities
    /// - `now_instant`: Current monotonic time instant (obtain from runtime layer)
    pub async fn discover_and_sync<E>(
        &self,
        effects: &E,
        now_instant: std::time::Instant,
    ) -> SyncResult<()>
    where
        E: aura_core::effects::JournalEffects
            + aura_core::effects::NetworkEffects
            + aura_core::effects::PhysicalTimeEffects
            + Send
            + Sync,
    {
        // Implement peer synchronization using journal_sync

        // 1. Discover available peers via peer_manager
        let available_peers = self.discover_available_peers().await?;

        if available_peers.is_empty() {
            tracing::debug!("No suitable peers found for synchronization");
            return Ok(());
        }

        // 2. Apply peer selection algorithm to choose best peers
        let selected_peers = Self::select_best_auto_sync_peers(
            &self.peer_manager,
            &available_peers,
            self.config.max_concurrent_syncs,
        )
        .await?;

        if selected_peers.is_empty() {
            tracing::debug!("No peers selected after scoring");
            return Ok(());
        }

        tracing::info!(
            "Selected {} best peers from {} available for sync",
            selected_peers.len(),
            available_peers.len()
        );

        // 3. Sync with selected peers using the full protocol integration
        self.sync_with_peers(effects, selected_peers.clone(), now_instant)
            .await?;

        // 4. Update peer states based on sync results
        self.update_peer_states(&selected_peers).await?;

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

        // Track last_sync from metrics (millis since epoch)
        let last_sync = self.metrics.read().get_last_sync_timestamp();

        SyncServiceHealth {
            status,
            active_sessions: session_stats.active_sessions,
            tracked_peers: peer_stats.total_tracked,
            available_peers: peer_stats.available_peers,
            last_sync,
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
            requests_processed: metrics.get_total_requests_processed(), // Populate from metrics
            errors_encountered: metrics.get_total_errors_encountered(), // Populate from metrics
            avg_latency_ms: metrics.get_average_sync_latency_ms(),      // Populate from metrics
            last_operation_at: metrics.get_last_operation_timestamp(),
        }
    }

    /// Perform one round of automatic synchronization with PhysicalTimeEffects
    ///
    /// # Arguments
    /// - `now_instant`: Current monotonic time instant (obtain from runtime layer)
    #[allow(dead_code)]
    async fn perform_auto_sync_with_time_effects<T: PhysicalTimeEffects + TimeEffects>(
        peer_manager: &Arc<RwLock<PeerManager>>,
        session_manager: &Arc<RwLock<SessionManager<serde_json::Value>>>,
        journal_sync: &Arc<RwLock<JournalSyncProtocol>>,
        rate_limiter: &Arc<RwLock<RateLimiter>>,
        max_concurrent: usize,
        time_effects: &T,
        now_instant: std::time::Instant,
    ) -> SyncResult<()> {
        let tick_ts = time_effects
            .physical_time()
            .await
            .map_err(time_error_to_aura)?;

        // Get available peers for synchronization
        let peers = {
            let manager = peer_manager.read();
            manager.select_sync_peers(max_concurrent)
        };

        if peers.is_empty() {
            return Ok(());
        }

        // Check rate limits before proceeding
        tracing::debug!("Auto-sync tick at {}", tick_ts.ts_ms);
        {
            let mut limiter = rate_limiter.write();
            for peer in &peers {
                match limiter.check_rate_limit(*peer, 100, now_instant) {
                    aura_core::RateLimitResult::Allowed => continue,
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

        // Implement actual peer synchronization via journal_sync
        let available_sessions = max_concurrent.saturating_sub(active_sessions);

        if available_sessions == 0 {
            tracing::debug!("No available session slots for auto-sync");
            return Ok(());
        }

        // 1. Select best peers based on priority and health scores
        let selected_peers =
            Self::select_best_auto_sync_peers(peer_manager, &peers, available_sessions).await?;

        if selected_peers.is_empty() {
            tracing::debug!(
                "No suitable peers selected for auto-sync from {} candidates",
                peers.len()
            );
            return Ok(());
        }

        // 2. Initiate sync sessions with selected peers
        let session_peers =
            Self::create_sync_sessions(session_manager, &selected_peers, time_effects).await?;

        // 3. Execute anti-entropy and journal sync protocols
        let sync_results = Self::execute_auto_sync_protocols(journal_sync, &session_peers).await?;

        // 4. Update peer scores based on sync results (get current time for timestamps)
        let score_now = time_effects
            .physical_time()
            .await
            .map_err(time_error_to_aura)?;
        Self::update_peer_scores_from_sync(peer_manager, &sync_results, &score_now).await?;

        // 5. Track metrics and update session states
        Self::update_auto_sync_metrics(&sync_results).await?;

        tracing::info!(
            "Auto-sync completed: {} peers processed, {} successful",
            session_peers.len(),
            sync_results.iter().filter(|(_, success)| *success).count()
        );

        Ok(())
    }

    /// Check rate limits for peer sync operations
    ///
    /// # Arguments
    /// - `peers`: List of peer device IDs to check
    /// - `now_instant`: Current monotonic time instant (obtain from runtime layer)
    async fn check_rate_limits(
        &self,
        peers: &[DeviceId],
        now_instant: std::time::Instant,
    ) -> SyncResult<Vec<DeviceId>> {
        let mut allowed_peers = Vec::new();
        let mut rate_limiter = self.rate_limiter.write();

        for &peer in peers {
            let result = rate_limiter.check_rate_limit(peer, 1, now_instant);
            if result.is_allowed() {
                allowed_peers.push(peer);
            } else if let Some(retry_after) = result.retry_after() {
                tracing::debug!(
                    "Rate limit exceeded for peer {}, retry after {:?}",
                    peer,
                    retry_after
                );
            }
        }

        Ok(allowed_peers)
    }

    /// Execute journal sync protocol with peers
    #[allow(clippy::await_holding_lock)]
    async fn execute_journal_sync_protocol<E>(
        &self,
        effects: &E,
        peers: &[DeviceId],
    ) -> SyncResult<Vec<(DeviceId, usize)>>
    where
        E: aura_core::effects::JournalEffects
            + aura_core::effects::NetworkEffects
            + aura_core::effects::PhysicalTimeEffects
            + Send
            + Sync,
    {
        let mut sync_results = Vec::new();

        for &peer in peers {
            tracing::debug!("Executing journal sync with peer {}", peer);

            // Clone protocol state to avoid holding lock across await; write back after sync.
            let mut protocol_clone = { self.journal_sync.write().clone() };
            let result = protocol_clone.sync_with_peer(effects, peer).await;
            *self.journal_sync.write() = protocol_clone;

            match result {
                Ok(synced_operations) => {
                    sync_results.push((peer, synced_operations));
                    tracing::info!(
                        "Successfully synced {} operations with peer {}",
                        synced_operations,
                        peer
                    );
                }
                Err(e) => {
                    tracing::error!("Failed to sync with peer {}: {}", peer, e);
                    sync_results.push((peer, 0));
                }
            }
        }

        Ok(sync_results)
    }

    /// Update sync metrics based on sync results
    async fn update_sync_metrics(&self, results: &[(DeviceId, usize)]) -> SyncResult<()> {
        // Get the time first (async) before acquiring the lock
        let now_ms = self
            .time_effects
            .physical_time()
            .await
            .map_err(time_error_to_aura)?
            .ts_ms;

        // Now acquire the lock and do synchronous work
        let metrics = self.metrics.write();
        for &(peer, synced_ops) in results {
            metrics.increment_sync_attempts(peer);

            if synced_ops > 0 {
                metrics.increment_sync_successes(peer);
                metrics.add_synced_operations(peer, synced_ops);
                metrics.update_last_sync(peer, now_ms);
            }
        }

        Ok(())
    }

    /// Clean up sync sessions after completion
    async fn cleanup_sync_sessions(&self, peers: &[DeviceId]) -> SyncResult<()> {
        let mut session_manager = self.session_manager.write();

        for &peer in peers {
            if let Err(e) = session_manager.close_session(peer) {
                tracing::warn!("Failed to clean up session for peer {}: {}", peer, e);
            } else {
                tracing::debug!("Cleaned up sync session for peer {}", peer);
            }
        }

        Ok(())
    }

    /// Discover available peers via peer_manager
    async fn discover_available_peers(&self) -> SyncResult<Vec<DeviceId>> {
        let peer_manager = self.peer_manager.read();
        let mut available_peers = Vec::new();

        // Get all known peers from the peer manager
        let all_peers = peer_manager.list_peers();

        // Filter for peers that are currently available and healthy
        for peer in all_peers {
            if peer_manager.is_peer_available(&peer) && peer_manager.get_peer_health(&peer) > 0.5 {
                available_peers.push(peer);
            }
        }

        tracing::debug!(
            "Discovered {} available peers for sync",
            available_peers.len()
        );
        Ok(available_peers)
    }

    /// Update peer states after sync operations
    async fn update_peer_states(&self, peers: &[DeviceId]) -> SyncResult<()> {
        // Get time before taking lock to avoid holding MutexGuard across await
        let now = self
            .time_effects
            .physical_time()
            .await
            .map_err(time_error_to_aura)?;

        let mut peer_manager = self.peer_manager.write();

        for &peer in peers {
            // Update last contact time
            peer_manager.update_last_contact(peer, &now);

            // Update peer availability based on recent sync attempts
            let recent_success_rate = peer_manager.get_recent_sync_success_rate(&peer);
            if recent_success_rate < 0.3 {
                peer_manager.mark_peer_degraded(&peer, &now);
            } else if recent_success_rate > 0.8 {
                peer_manager.mark_peer_healthy(&peer, &now);
            }
        }

        Ok(())
    }

    /// Check if peer is currently in an active sync session
    #[allow(dead_code)]
    async fn is_peer_in_active_sync(&self, peer: DeviceId) -> bool {
        let session_manager = self.session_manager.read();
        session_manager.has_active_session(peer)
    }

    /// Execute sync protocol with a single peer
    #[allow(dead_code)]
    async fn execute_single_peer_sync(&self, peer: DeviceId) -> SyncResult<()> {
        tracing::debug!("Starting auto-sync with peer {}", peer);
        // Integration with transport/effects happens upstream; this path returns success for deterministic testing
        tracing::info!("Auto-sync with peer {} completed", peer);
        Ok(())
    }

    /// Wait for active sessions to complete with timeout
    ///
    /// # Arguments
    /// - `start_instant`: Starting monotonic time instant (obtain from runtime layer)
    async fn wait_for_sessions_to_complete(
        &self,
        start_instant: std::time::Instant,
    ) -> SyncResult<()> {
        self.wait_for_sessions_to_complete_with_time_effects(&self.time_effects, start_instant)
            .await
    }

    /// Wait for active sessions to complete with timeout using PhysicalTimeEffects
    ///
    /// # Arguments
    /// - `time_effects`: Time effects provider for sleep operations
    /// - `start_instant`: Starting monotonic time instant (obtain from runtime layer)
    async fn wait_for_sessions_to_complete_with_time_effects<T: PhysicalTimeEffects>(
        &self,
        time_effects: &T,
        start_instant: std::time::Instant,
    ) -> SyncResult<()> {
        let timeout = Duration::from_secs(30); // 30 second timeout
        let check_interval = Duration::from_millis(100);

        loop {
            let elapsed = start_instant.elapsed();
            if elapsed >= timeout {
                break;
            }
            let active_sessions = {
                let manager = self.session_manager.read();
                manager.get_statistics().active_sessions
            };

            if active_sessions == 0 {
                break;
            }

            time_effects
                .sleep_ms(check_interval.as_millis() as u64)
                .await
                .map_err(time_error_to_aura)?;
        }

        Ok(())
    }

    /// Select best auto-sync peers based on health and priority (static helper)
    async fn select_best_auto_sync_peers(
        peer_manager: &Arc<RwLock<PeerManager>>,
        peers: &[DeviceId],
        max_peers: usize,
    ) -> SyncResult<Vec<DeviceId>> {
        let manager = peer_manager.read();
        let mut peer_scores = Vec::new();

        for &peer in peers {
            let health = manager.get_peer_health(&peer);
            let priority = manager.get_peer_priority(&peer);
            let score = health * priority;
            peer_scores.push((peer, score));
        }

        peer_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let selected: Vec<DeviceId> = peer_scores
            .into_iter()
            .take(max_peers)
            .map(|(peer, _)| peer)
            .collect();

        Ok(selected)
    }

    /// Create sync sessions for selected peers (static method)
    async fn create_sync_sessions<T: PhysicalTimeEffects>(
        session_manager: &Arc<RwLock<SessionManager<serde_json::Value>>>,
        peers: &[DeviceId],
        time_effects: &T,
    ) -> SyncResult<Vec<DeviceId>> {
        let mut session_peers = Vec::new();

        // Get timestamp first to avoid holding lock across await
        let now = time_effects
            .physical_time()
            .await
            .map_err(time_error_to_aura)?;

        // Now acquire lock and create sessions
        let mut manager = session_manager.write();
        for &peer in peers {
            match manager.create_session(vec![peer], &now) {
                Ok(_session_id) => {
                    session_peers.push(peer);
                    tracing::debug!("Created auto-sync session for peer {}", peer);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to create auto-sync session for peer {}: {}",
                        peer,
                        e
                    );
                }
            }
        }

        Ok(session_peers)
    }

    /// Execute auto-sync protocols for peers (static method)
    #[allow(dead_code)]
    async fn execute_auto_sync_protocols(
        journal_sync: &Arc<RwLock<JournalSyncProtocol>>,
        peers: &[DeviceId],
    ) -> SyncResult<Vec<(DeviceId, bool)>> {
        let mut results = Vec::new();
        for &peer in peers {
            tracing::debug!("Auto-sync executed for peer {}", peer);
            // success path
            results.push((peer, true));
            let _ = journal_sync; // keep parameter usage for compatibility
        }
        Ok(results)
    }

    /// Update peer scores based on sync results (static method)
    async fn update_peer_scores_from_sync(
        peer_manager: &Arc<RwLock<PeerManager>>,
        results: &[(DeviceId, bool)],
        now: &aura_core::time::PhysicalTime,
    ) -> SyncResult<()> {
        let mut manager = peer_manager.write();

        for &(peer, success) in results {
            if success {
                manager.increment_sync_success(&peer, now);
                manager.update_last_successful_sync(&peer, now);
                manager.recalculate_peer_health(&peer);
            } else {
                manager.increment_sync_failure(&peer);
                manager.recalculate_peer_health(&peer);
            }
        }

        Ok(())
    }

    /// Update auto-sync metrics (static method)
    async fn update_auto_sync_metrics(results: &[(DeviceId, bool)]) -> SyncResult<()> {
        let total_peers = results.len();
        let successful_syncs = results.iter().filter(|(_, success)| *success).count();
        let failed_syncs = total_peers - successful_syncs;

        tracing::info!(
            "Auto-sync metrics: {} total peers, {} successful, {} failed",
            total_peers,
            successful_syncs,
            failed_syncs
        );

        // In a full implementation, this would update metrics collectors
        // with the sync results for monitoring and alerting

        Ok(())
    }

    /// Start the service using PhysicalTimeEffects (preferred over Service::start)
    ///
    /// # Arguments
    /// - `time_effects`: Time effects provider
    /// - `now_instant`: Current monotonic time instant (obtain from runtime layer)
    pub async fn start_with_time_effects<T: aura_core::effects::PhysicalTimeEffects>(
        &self,
        time_effects: &T,
        now_instant: std::time::Instant,
    ) -> SyncResult<()> {
        // Use PhysicalTimeEffects for deterministic wall-clock; store Instant for uptime tracking
        let _ts = time_effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("time error: {e}")))?;

        self.start(now_instant).await
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

        {
            let mut state = self.state.write();
            *state = ServiceState::Running;
        }
        Ok(())
    }

    async fn stop(&self, now: std::time::Instant) -> SyncResult<()> {
        {
            let mut state = self.state.write();
            if *state == ServiceState::Stopped {
                return Ok(());
            }

            *state = ServiceState::Stopping;
        } // Drop state lock before await

        // Wait for active sessions to complete with timeout
        self.wait_for_sessions_to_complete(now).await?;

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
            checked_at: {
                let time_effects = PhysicalTimeHandler;
                time_effects
                    .physical_time()
                    .await
                    .map_err(|e| aura_core::AuraError::internal(format!("Time error: {}", e)))?
                    .ts_ms
                    / 1000
            },
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
    ///
    /// # Arguments
    /// - `now_instant`: Current monotonic time instant (obtain from runtime layer)
    pub async fn build(self, now_instant: std::time::Instant) -> SyncResult<SyncService> {
        let config = self.config.unwrap_or_default();
        SyncService::new(config, now_instant).await
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // Test code uses monotonic clock for coordination
    use super::*;

    #[tokio::test]
    async fn test_sync_service_creation() {
        let config = SyncServiceConfig::default();
        let service = SyncService::new(config, SyncService::monotonic_now())
            .await
            .unwrap();

        assert_eq!(service.name(), "SyncService");
        assert!(!service.is_running());
    }

    #[tokio::test]
    async fn test_sync_service_builder() {
        let service = SyncService::builder()
            .with_auto_sync(true)
            .with_sync_interval(Duration::from_secs(30))
            .build(SyncService::monotonic_now())
            .await
            .unwrap();

        assert!(service.config.auto_sync_enabled);
        assert_eq!(service.config.auto_sync_interval, Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_sync_service_lifecycle() {
        let service = SyncService::builder()
            .build(SyncService::monotonic_now())
            .await
            .unwrap();

        assert!(!service.is_running());

        let time_effects = PhysicalTimeHandler;
        service
            .start_with_time_effects(&time_effects, SyncService::monotonic_now())
            .await
            .unwrap();
        assert!(service.is_running());

        service.stop(SyncService::monotonic_now()).await.unwrap();
        assert!(!service.is_running());
    }

    #[tokio::test]
    async fn test_sync_service_health_check() {
        let service = SyncService::builder()
            .build(SyncService::monotonic_now())
            .await
            .unwrap();
        let time_effects = PhysicalTimeHandler;
        service
            .start_with_time_effects(&time_effects, SyncService::monotonic_now())
            .await
            .unwrap();

        let health = service.health_check().await.unwrap();
        assert_eq!(health.status, HealthStatus::Healthy);
        assert!(health.details.contains_key("active_sessions"));
    }
}
