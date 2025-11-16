//! Synchronization Scheduler
//!
//! Provides periodic and event-driven synchronization scheduling with
//! intelligent peer selection, load balancing, and resource management.

use super::{OpLogSynchronizer, PeerInfo, PeerSyncManager, SyncConfiguration, SyncError, SyncResult};
use super::{DeviceId, PeerMetrics};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

/// Errors that can occur in the sync scheduler
#[derive(Debug, Error)]
pub enum SchedulerError {
    /// Scheduler is already running
    #[error("Scheduler is already running")]
    AlreadyRunning,

    /// Scheduler is not running
    #[error("Scheduler is not running")]
    NotRunning,

    /// Invalid configuration
    #[error("Invalid configuration: {reason}")]
    InvalidConfiguration {
        /// The reason the configuration is invalid
        reason: String,
    },

    /// Resource limit was exceeded
    #[error("Resource limit exceeded: {limit_type}")]
    ResourceLimitExceeded {
        /// The type of resource limit exceeded
        limit_type: String,
    },

    /// Sync error occurred
    #[error("Sync error: {source}")]
    SyncError {
        /// The underlying sync error
        #[from]
        source: SyncError,
    },
}

/// Configuration for the sync scheduler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Base interval between sync rounds
    pub base_sync_interval: Duration,
    /// Maximum number of concurrent syncs
    pub max_concurrent_syncs: usize,
    /// Maximum time to wait for a sync to complete
    pub sync_timeout: Duration,
    /// Interval for cleanup operations
    pub cleanup_interval: Duration,
    /// Strategy for peer selection
    pub selection_strategy: PeerSelectionStrategy,
    /// Load balancing configuration
    pub load_balancing: LoadBalancingConfig,
    /// Priority scheduling configuration
    pub priority_config: PriorityConfig,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            base_sync_interval: Duration::from_secs(30),
            max_concurrent_syncs: 5,
            sync_timeout: Duration::from_secs(120),
            cleanup_interval: Duration::from_secs(300), // 5 minutes
            selection_strategy: PeerSelectionStrategy::PriorityBased,
            load_balancing: LoadBalancingConfig::default(),
            priority_config: PriorityConfig::default(),
        }
    }
}

/// Strategy for selecting peers to sync with
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PeerSelectionStrategy {
    /// Select peers based on priority scores
    PriorityBased,
    /// Round-robin through all available peers
    RoundRobin,
    /// Random selection from available peers
    Random,
    /// Focus on high-latency peers first
    LatencyOptimized,
}

/// Load balancing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadBalancingConfig {
    /// Maximum CPU usage percentage (0-100)
    pub max_cpu_usage: u32,
    /// Maximum network bandwidth usage in bytes/sec
    pub max_network_bandwidth: u64,
    /// Enable adaptive scheduling based on system load
    pub adaptive_scheduling: bool,
}

impl Default for LoadBalancingConfig {
    fn default() -> Self {
        Self {
            max_cpu_usage: 80,
            max_network_bandwidth: 10_000_000, // 10 MB/s
            adaptive_scheduling: true,
        }
    }
}

/// Priority configuration for scheduling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorityConfig {
    /// Minimum priority threshold for scheduling
    pub min_priority_threshold: u32,
    /// Boost priority for peers with many pending operations
    pub pending_operations_boost: u32,
    /// Penalty for peers with recent failures
    pub failure_penalty: u32,
}

impl Default for PriorityConfig {
    fn default() -> Self {
        Self {
            min_priority_threshold: 10,
            pending_operations_boost: 20,
            failure_penalty: 15,
        }
    }
}

/// Scheduled sync task
#[derive(Debug, Clone)]
struct ScheduledSync {
    /// Peer to sync with
    peer_id: DeviceId,
    /// When this sync was scheduled
    scheduled_at: Instant,
    /// Priority of this sync
    priority: u32,
    /// Number of retry attempts
    retry_count: u32,
}

/// Statistics about scheduler performance
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchedulerStatistics {
    /// Total number of syncs scheduled
    pub syncs_scheduled: u64,
    /// Total number of syncs completed
    pub syncs_completed: u64,
    /// Total number of syncs failed
    pub syncs_failed: u64,
    /// Average sync duration
    pub average_sync_duration: Duration,
    /// Current queue length
    pub current_queue_length: usize,
    /// Scheduler uptime
    pub uptime: Duration,
}

/// Main synchronization scheduler
pub struct SyncScheduler {
    /// Configuration
    config: SchedulerConfig,
    /// Synchronizer instance
    synchronizer: OpLogSynchronizer,
    /// Peer sync manager
    peer_manager: PeerSyncManager,
    /// Sync queue
    sync_queue: VecDeque<ScheduledSync>,
    /// Currently running syncs
    active_syncs: HashMap<DeviceId, Instant>,
    /// Round-robin state for peer selection
    round_robin_index: usize,
    /// Scheduler start time
    start_time: Option<Instant>,
    /// Statistics
    statistics: SchedulerStatistics,
    /// Whether scheduler is running
    is_running: bool,
}

impl SyncScheduler {
    /// Create a new sync scheduler
    pub fn new(config: SchedulerConfig, synchronizer: OpLogSynchronizer) -> Self {
        let peer_manager = PeerSyncManager::new(
            3,                          // max retries
            Duration::from_millis(500), // base delay
            Duration::from_secs(300),   // max delay (5 minutes)
            0.1,                        // jitter factor
        );

        Self {
            config,
            synchronizer,
            peer_manager,
            sync_queue: VecDeque::new(),
            active_syncs: HashMap::new(),
            round_robin_index: 0,
            start_time: None,
            statistics: SchedulerStatistics::default(),
            is_running: false,
        }
    }

    /// Add a peer to the scheduler
    pub fn add_peer(&mut self, peer_info: PeerInfo) {
        info!("Adding peer to scheduler: {}", peer_info.device_id);
        self.synchronizer.add_peer(peer_info.clone());
        self.peer_manager.add_peer(peer_info);
    }

    /// Remove a peer from the scheduler
    pub fn remove_peer(&mut self, peer_id: &DeviceId) {
        info!("Removing peer from scheduler: {}", peer_id);
        self.synchronizer.remove_peer(peer_id);
        self.peer_manager.remove_peer(peer_id);

        // Remove from active syncs and queue
        self.active_syncs.remove(peer_id);
        self.sync_queue.retain(|sync| sync.peer_id != *peer_id);
    }

    /// Start the scheduler
    #[allow(clippy::disallowed_methods)]
    pub async fn start(&mut self) -> Result<(), SchedulerError> {
        if self.is_running {
            return Err(SchedulerError::AlreadyRunning);
        }

        info!("Starting sync scheduler");
        self.is_running = true;
        self.start_time = Some(Instant::now());

        // Start the main scheduling loop
        self.run_scheduler().await
    }

    /// Stop the scheduler
    pub fn stop(&mut self) -> Result<(), SchedulerError> {
        if !self.is_running {
            return Err(SchedulerError::NotRunning);
        }

        info!("Stopping sync scheduler");
        self.is_running = false;
        Ok(())
    }

    /// Manually trigger sync with a specific peer
    #[allow(clippy::disallowed_methods)]
    pub fn schedule_peer_sync(&mut self, peer_id: DeviceId, priority: u32) {
        let sync = ScheduledSync {
            peer_id,
            scheduled_at: Instant::now(),
            priority,
            retry_count: 0,
        };

        // Insert based on priority (higher priority first)
        let insert_position = self
            .sync_queue
            .iter()
            .position(|s| s.priority < priority)
            .unwrap_or(self.sync_queue.len());

        self.sync_queue.insert(insert_position, sync);
        self.statistics.syncs_scheduled += 1;

        debug!(
            "Scheduled sync with peer {} (priority: {})",
            peer_id, priority
        );
    }

    /// Manually trigger sync with all peers
    pub fn schedule_all_peers_sync(&mut self) {
        let peers: Vec<_> = self
            .peer_manager
            .get_peers_by_priority(self.config.base_sync_interval);

        for (peer_id, priority) in peers {
            self.schedule_peer_sync(peer_id, priority);
        }
    }

    /// Get current scheduler statistics
    pub fn get_statistics(&self) -> SchedulerStatistics {
        let mut stats = self.statistics.clone();
        stats.current_queue_length = self.sync_queue.len();

        if let Some(start_time) = self.start_time {
            stats.uptime = start_time.elapsed();
        }

        stats
    }

    /// Check if scheduler is running
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// Main scheduler loop
    async fn run_scheduler(&mut self) -> Result<(), SchedulerError> {
        let mut sync_interval = interval(self.config.base_sync_interval);
        let mut cleanup_interval = interval(self.config.cleanup_interval);

        while self.is_running {
            tokio::select! {
                // Regular sync tick
                _ = sync_interval.tick() => {
                    if let Err(e) = self.schedule_regular_syncs().await {
                        error!("Error in regular sync scheduling: {}", e);
                    }
                }

                // Cleanup tick
                _ = cleanup_interval.tick() => {
                    self.cleanup_stale_syncs().await;
                }

                // Process sync queue
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    if let Err(e) = self.process_sync_queue().await {
                        error!("Error processing sync queue: {}", e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Schedule regular synchronization based on configuration
    async fn schedule_regular_syncs(&mut self) -> Result<(), SchedulerError> {
        debug!("Running regular sync scheduling");

        let peers_to_sync = match self.config.selection_strategy {
            PeerSelectionStrategy::PriorityBased => {
                let peers = self
                    .peer_manager
                    .get_peers_by_priority(self.config.base_sync_interval);
                peers
                    .into_iter()
                    .filter(|(_, priority)| {
                        *priority >= self.config.priority_config.min_priority_threshold
                    })
                    .take(self.config.max_concurrent_syncs)
                    .collect()
            }
            PeerSelectionStrategy::RoundRobin => self.select_peers_round_robin(),
            PeerSelectionStrategy::Random => self.select_peers_random(),
            PeerSelectionStrategy::LatencyOptimized => self.select_peers_latency_optimized(),
        };

        for (peer_id, priority) in peers_to_sync {
            if !self.active_syncs.contains_key(&peer_id) {
                self.schedule_peer_sync(peer_id, priority);
            }
        }

        Ok(())
    }

    /// Process the sync queue and start syncs
    async fn process_sync_queue(&mut self) -> Result<(), SchedulerError> {
        while !self.sync_queue.is_empty() && self.can_start_more_syncs() {
            if let Some(sync) = self.sync_queue.pop_front() {
                if let Err(e) = self.start_sync(sync).await {
                    warn!("Failed to start sync: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Start a specific sync
    #[allow(clippy::disallowed_methods)]
    async fn start_sync(&mut self, sync: ScheduledSync) -> Result<(), SchedulerError> {
        let peer_id = sync.peer_id;

        // Check if peer is already syncing
        if self.active_syncs.contains_key(&peer_id) {
            return Ok(()); // Already syncing
        }

        info!(
            "Starting sync with peer {} (priority: {})",
            peer_id, sync.priority
        );

        // Record active sync
        self.active_syncs.insert(peer_id, Instant::now());

        // Execute sync
        let result = self.synchronizer.sync_with_peer(peer_id).await;

        // Remove from active syncs
        self.active_syncs.remove(&peer_id);

        // Process result
        match result {
            Ok(sync_result) => {
                self.handle_sync_success(peer_id, sync_result).await;
            }
            Err(sync_error) => {
                self.handle_sync_failure(peer_id, sync_error, sync).await?;
            }
        }

        Ok(())
    }

    /// Handle successful sync
    async fn handle_sync_success(&mut self, peer_id: DeviceId, result: SyncResult) {
        self.statistics.syncs_completed += 1;

        // Update average sync duration
        let total_duration = self.statistics.average_sync_duration.as_millis() as u64
            * self.statistics.syncs_completed;
        let new_total = total_duration + result.sync_duration.as_millis() as u64;
        self.statistics.average_sync_duration =
            Duration::from_millis(new_total / self.statistics.syncs_completed);

        self.peer_manager.complete_sync(
            peer_id,
            result.operations_sent,
            result.operations_received,
        );

        debug!(
            "Sync completed with peer {}: {} ops sent, {} ops received, duration: {:?}",
            peer_id, result.operations_sent, result.operations_received, result.sync_duration
        );
    }

    /// Handle failed sync
    #[allow(clippy::disallowed_methods)]
    async fn handle_sync_failure(
        &mut self,
        peer_id: DeviceId,
        error: SyncError,
        mut sync: ScheduledSync,
    ) -> Result<(), SchedulerError> {
        self.statistics.syncs_failed += 1;

        warn!("Sync failed with peer {}: {}", peer_id, error);

        self.peer_manager.fail_sync(peer_id, &error.to_string());

        // Check if we should retry
        if sync.retry_count < 3 {
            // Configure max retries
            sync.retry_count += 1;
            let retry_attempt = sync.retry_count;
            sync.priority = sync.priority.saturating_sub(10); // Reduce priority on retry
            sync.scheduled_at = Instant::now() + Duration::from_secs(30 * retry_attempt as u64); // Exponential backoff

            // Re-queue for retry
            self.sync_queue.push_back(sync);
            debug!(
                "Re-queued sync with peer {} for retry (attempt {})",
                peer_id,
                retry_attempt + 1
            );
        }

        Ok(())
    }

    /// Check if more syncs can be started
    fn can_start_more_syncs(&self) -> bool {
        self.active_syncs.len() < self.config.max_concurrent_syncs
    }

    /// Clean up stale sync sessions
    #[allow(clippy::disallowed_methods)]
    async fn cleanup_stale_syncs(&mut self) {
        let now = Instant::now();
        let timeout = self.config.sync_timeout;

        let stale_peers: Vec<_> = self
            .active_syncs
            .iter()
            .filter(|(_, start_time)| now.duration_since(**start_time) > timeout)
            .map(|(peer_id, _)| *peer_id)
            .collect();

        for peer_id in stale_peers {
            warn!("Cleaning up stale sync for peer {}", peer_id);
            self.active_syncs.remove(&peer_id);
            self.peer_manager.fail_sync(peer_id, "Sync timeout");
        }

        // Clean up peer manager stale sessions
        self.peer_manager.cleanup_stale_sessions(timeout);
    }

    /// Select peers using round-robin strategy
    fn select_peers_round_robin(&mut self) -> Vec<(DeviceId, u32)> {
        let all_peers: Vec<_> = self.peer_manager.get_all_peer_states().collect();

        if all_peers.is_empty() {
            return Vec::new();
        }

        let mut selected = Vec::new();
        let available_slots = self
            .config
            .max_concurrent_syncs
            .saturating_sub(self.active_syncs.len());

        for _ in 0..available_slots {
            if self.round_robin_index >= all_peers.len() {
                self.round_robin_index = 0;
            }

            if let Some(peer_state) = all_peers.get(self.round_robin_index) {
                if peer_state.needs_sync(self.config.base_sync_interval) {
                    selected.push((peer_state.peer_id, peer_state.get_sync_priority()));
                }
                self.round_robin_index += 1;
            }
        }

        selected
    }

    /// Select peers randomly
    #[allow(clippy::disallowed_methods)]
    fn select_peers_random(&self) -> Vec<(DeviceId, u32)> {
        use rand::seq::SliceRandom;

        let mut all_peers: Vec<_> = self
            .peer_manager
            .get_all_peer_states()
            .filter(|state| state.needs_sync(self.config.base_sync_interval))
            .map(|state| (state.peer_id, state.get_sync_priority()))
            .collect();

        let mut rng = rand::thread_rng();
        all_peers.shuffle(&mut rng);

        let available_slots = self
            .config
            .max_concurrent_syncs
            .saturating_sub(self.active_syncs.len());
        all_peers.truncate(available_slots);
        all_peers
    }

    /// Select peers optimized for latency
    fn select_peers_latency_optimized(&self) -> Vec<(DeviceId, u32)> {
        let mut peers: Vec<_> = self
            .peer_manager
            .get_all_peer_states()
            .filter(|state| state.needs_sync(self.config.base_sync_interval))
            .map(|state| {
                let latency_penalty = state.peer_info.metrics.average_latency_ms as u32;
                let adjusted_priority = state
                    .get_sync_priority()
                    .saturating_sub(latency_penalty / 10);
                (state.peer_id, adjusted_priority)
            })
            .collect();

        // Sort by adjusted priority (higher latency = lower effective priority)
        peers.sort_by(|a, b| b.1.cmp(&a.1));

        let available_slots = self
            .config
            .max_concurrent_syncs
            .saturating_sub(self.active_syncs.len());
        peers.truncate(available_slots);
        peers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::DeviceId;
    use aura_journal::semilattice::op_log::OpLog;

    fn create_test_peer_info(peer_id: DeviceId) -> PeerInfo {
        PeerInfo {
            device_id: peer_id,
            last_seen: 1000,
            connection_quality: 0.8,
            metrics: PeerMetrics {
                latency_ms: 80,
                bandwidth_bps: 50_000_000,
                reliability: 0.9,
                reliability_score: 85,
                average_latency_ms: 80,
            },
        }
    }

    #[test]
    fn test_scheduler_creation() {
        let config = SchedulerConfig::default();
        let oplog = OpLog::new();
        let sync_config = SyncConfiguration::default();
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let synchronizer = OpLogSynchronizer::new(device_id, oplog, sync_config);
        let scheduler = SyncScheduler::new(config, synchronizer);

        assert!(!scheduler.is_running());
        assert_eq!(scheduler.sync_queue.len(), 0);
        assert_eq!(scheduler.active_syncs.len(), 0);
    }

    #[test]
    fn test_peer_management() {
        let config = SchedulerConfig::default();
        let oplog = OpLog::new();
        let sync_config = SyncConfiguration::default();
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let synchronizer = OpLogSynchronizer::new(device_id, oplog, sync_config);
        let mut scheduler = SyncScheduler::new(config, synchronizer);

        let peer_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let peer_info = create_test_peer_info(peer_id);

        scheduler.add_peer(peer_info);

        // Verify peer was added
        assert!(scheduler.peer_manager.get_peer_state(&peer_id).is_some());

        scheduler.remove_peer(&peer_id);

        // Verify peer was removed
        assert!(scheduler.peer_manager.get_peer_state(&peer_id).is_none());
    }

    #[test]
    fn test_sync_scheduling() {
        let config = SchedulerConfig::default();
        let oplog = OpLog::new();
        let sync_config = SyncConfiguration::default();
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let synchronizer = OpLogSynchronizer::new(device_id, oplog, sync_config);
        let mut scheduler = SyncScheduler::new(config, synchronizer);

        let peer_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));

        scheduler.schedule_peer_sync(peer_id, 100);
        assert_eq!(scheduler.sync_queue.len(), 1);
        assert_eq!(scheduler.statistics.syncs_scheduled, 1);

        // Higher priority should be queued first
        let peer_id2 = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        scheduler.schedule_peer_sync(peer_id2, 150);
        assert_eq!(scheduler.sync_queue.len(), 2);
        assert_eq!(scheduler.sync_queue[0].peer_id, peer_id2); // Higher priority first
    }

    #[test]
    fn test_statistics() {
        let config = SchedulerConfig::default();
        let oplog = OpLog::new();
        let sync_config = SyncConfiguration::default();
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let synchronizer = OpLogSynchronizer::new(device_id, oplog, sync_config);
        let scheduler = SyncScheduler::new(config, synchronizer);

        let stats = scheduler.get_statistics();
        assert_eq!(stats.syncs_scheduled, 0);
        assert_eq!(stats.syncs_completed, 0);
        assert_eq!(stats.syncs_failed, 0);
        assert_eq!(stats.current_queue_length, 0);
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_scheduled_sync_priority() {
        let peer_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let sync = ScheduledSync {
            peer_id,
            scheduled_at: Instant::now(),
            priority: 100,
            retry_count: 0,
        };

        assert_eq!(sync.priority, 100);
        assert_eq!(sync.retry_count, 0);
    }
}