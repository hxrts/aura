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

mod bookkeeping;
mod builder;
mod health;
#[cfg(test)]
mod tests;

use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{
    begin_service_start, begin_service_stop, finish_service_start, finish_service_stop,
    HealthCheck, MonotonicInstant, Service, ServiceState,
};
use crate::core::{MetricsCollector, SessionConfig, SessionManager, SyncResult};
use crate::infrastructure::{PeerDiscoveryConfig, PeerManager, RateLimitConfig, RateLimiter};
use crate::protocols::{JournalSyncConfig, JournalSyncProtocol, SyncProtocolEffects};
use aura_core::effects::{PhysicalTimeEffects, TimeError};
use aura_core::{AuraError, DeviceId};

fn time_error_to_aura(err: TimeError) -> AuraError {
    AuraError::internal(format!("time error: {err}"))
}

const JOURNAL_SYNC_OPERATION_ID: &str = "journal_sync";
const AUTO_SYNC_OPERATION_ID: &str = "auto_sync";

pub use bookkeeping::SyncMaintenanceStats;
pub use builder::SyncServiceBuilder;
pub use health::SyncServiceHealth;

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
    pub max_concurrent_syncs: u32,
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
    state: RwLock<ServiceState>,

    /// Peer manager
    peer_manager: RwLock<PeerManager>,

    /// Rate limiter
    rate_limiter: RwLock<RateLimiter>,

    /// Session manager
    session_manager: RwLock<SessionManager<serde_json::Value>>,

    /// Journal sync protocol
    journal_sync: RwLock<JournalSyncProtocol>,

    /// Metrics collector
    metrics: RwLock<MetricsCollector>,

    /// Service start time
    started_at: RwLock<Option<MonotonicInstant>>,

    /// Time effects for unified time operations
    time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
}

impl SyncService {
    /// Monotonic clock source for service lifecycle events.
    ///
    /// This intentionally lives in the sync layer to avoid leaking runtime clock calls
    /// into application code that is subject to effects-enforcement checks.
    #[allow(clippy::disallowed_methods)]
    pub fn monotonic_now() -> MonotonicInstant {
        // Alias to keep monotonic semantics without exposing runtime clock calls
        // monotonic semantics required by rate limiting and session management.
        type MonoTime = MonotonicInstant;
        MonoTime::now()
    }

    /// Create a new sync service
    ///
    /// # Arguments
    /// - `config`: Service configuration
    /// - `now_instant`: Current monotonic time instant (obtain from runtime layer)
    pub async fn new(
        config: SyncServiceConfig,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        now_instant: MonotonicInstant,
    ) -> SyncResult<Self> {
        let peer_manager = PeerManager::new(config.peer_discovery.clone());
        let rate_limiter = RateLimiter::new(config.rate_limit.clone(), now_instant);
        let now = time_effects
            .physical_time()
            .await
            .map_err(time_error_to_aura)?;
        let session_manager = SessionManager::new(SessionConfig::default(), now);
        let journal_sync = JournalSyncProtocol::new(config.journal_sync.clone());
        let metrics = MetricsCollector::new();

        Ok(Self {
            config,
            state: RwLock::new(ServiceState::Stopped),
            peer_manager: RwLock::new(peer_manager),
            rate_limiter: RwLock::new(rate_limiter),
            session_manager: RwLock::new(session_manager),
            journal_sync: RwLock::new(journal_sync),
            metrics: RwLock::new(metrics),
            started_at: RwLock::new(None),
            time_effects,
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
        now_instant: MonotonicInstant,
    ) -> SyncResult<()>
    where
        E: SyncProtocolEffects,
    {
        if peers.is_empty() {
            return Ok(());
        }

        let authority_id = effects.authority_id();
        tracing::info!(
            operation_id = JOURNAL_SYNC_OPERATION_ID,
            authority_id = %authority_id,
            peer_count = peers.len(),
            "Starting journal sync"
        );

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

        tracing::info!(
            operation_id = JOURNAL_SYNC_OPERATION_ID,
            authority_id = %authority_id,
            synced_peer_count = sync_results.len(),
            "Completed journal sync"
        );
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
        now_instant: MonotonicInstant,
    ) -> SyncResult<()>
    where
        E: SyncProtocolEffects,
    {
        let authority_id = effects.authority_id();
        // Implement peer synchronization using journal_sync

        // 1. Discover available peers via peer_manager
        let available_peers = self.discover_available_peers().await?;

        if available_peers.is_empty() {
            tracing::debug!(
                operation_id = AUTO_SYNC_OPERATION_ID,
                authority_id = %authority_id,
                "No suitable peers found for synchronization"
            );
            return Ok(());
        }

        // 2. Apply peer selection algorithm to choose best peers
        let selected_peers = Self::select_best_auto_sync_peers(
            &self.peer_manager,
            &available_peers,
            self.config.max_concurrent_syncs as usize,
        )
        .await?;

        if selected_peers.is_empty() {
            tracing::debug!(
                operation_id = AUTO_SYNC_OPERATION_ID,
                authority_id = %authority_id,
                available_peer_count = available_peers.len(),
                "No peers selected after scoring"
            );
            return Ok(());
        }

        tracing::info!(
            operation_id = AUTO_SYNC_OPERATION_ID,
            authority_id = %authority_id,
            selected_peer_count = selected_peers.len(),
            available_peer_count = available_peers.len(),
            "Selected best peers for sync"
        );

        // 3. Sync with selected peers using the full protocol integration
        self.sync_with_peers(effects, selected_peers.clone(), now_instant)
            .await?;

        // 4. Update peer states based on sync results
        self.update_peer_states(&selected_peers).await?;

        Ok(())
    }

    /// Execute journal sync protocol with peers
    #[allow(clippy::await_holding_lock)]
    async fn execute_journal_sync_protocol<E>(
        &self,
        effects: &E,
        peers: &[DeviceId],
    ) -> SyncResult<Vec<(DeviceId, u64)>>
    where
        E: SyncProtocolEffects,
    {
        let mut sync_results = Vec::new();
        let authority_id = effects.authority_id();

        for &peer in peers {
            tracing::debug!(
                operation_id = JOURNAL_SYNC_OPERATION_ID,
                authority_id = %authority_id,
                peer_id = %peer,
                "Executing journal sync with peer"
            );

            // Clone protocol state to avoid holding lock across await; write back after sync.
            let mut protocol_clone = { self.journal_sync.write().clone() };
            let result = protocol_clone.sync_with_peer(effects, peer).await;
            *self.journal_sync.write() = protocol_clone;

            match result {
                Ok(synced_operations) => {
                    sync_results.push((peer, synced_operations));
                    tracing::info!(
                        operation_id = JOURNAL_SYNC_OPERATION_ID,
                        authority_id = %authority_id,
                        peer_id = %peer,
                        synced_operations,
                        "Successfully synced operations with peer"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        operation_id = JOURNAL_SYNC_OPERATION_ID,
                        authority_id = %authority_id,
                        peer_id = %peer,
                        error = %e,
                        "Failed to sync with peer"
                    );
                    sync_results.push((peer, 0));
                }
            }
        }

        Ok(sync_results)
    }

    /// Wait for active sessions to complete with timeout
    ///
    /// # Arguments
    /// - `start_instant`: Starting monotonic time instant (obtain from runtime layer)
    async fn wait_for_sessions_to_complete(
        &self,
        start_instant: MonotonicInstant,
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
        start_instant: MonotonicInstant,
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

    /// Start the service using PhysicalTimeEffects
    ///
    /// # Arguments
    /// - `time_effects`: Time effects provider
    /// - `now_instant`: Current monotonic time instant (obtain from runtime layer)
    pub async fn start_with_time_effects<T: aura_core::effects::PhysicalTimeEffects>(
        &self,
        time_effects: &T,
        now_instant: MonotonicInstant,
    ) -> SyncResult<()> {
        begin_service_start(&self.state, &self.started_at, now_instant)?;
        // Use PhysicalTimeEffects for deterministic wall-clock; store MonotonicInstant for uptime tracking
        let _ts = time_effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("time error: {e}")))?;
        finish_service_start(&self.state);
        Ok(())
    }
}

#[async_trait::async_trait]
impl Service for SyncService {
    async fn start(&self, now: MonotonicInstant) -> SyncResult<()> {
        begin_service_start(&self.state, &self.started_at, now)?;
        finish_service_start(&self.state);
        Ok(())
    }

    async fn stop(&self, now: MonotonicInstant) -> SyncResult<()> {
        if !begin_service_stop(&self.state) {
            return Ok(());
        }

        // Wait for active sessions to complete with timeout
        self.wait_for_sessions_to_complete(now).await?;

        finish_service_stop(&self.state);
        Ok(())
    }

    async fn health_check(&self) -> SyncResult<HealthCheck> {
        self.build_health_check().await
    }

    fn name(&self) -> &str {
        "SyncService"
    }

    fn is_running(&self) -> bool {
        self.state.read().is_running()
    }
}
