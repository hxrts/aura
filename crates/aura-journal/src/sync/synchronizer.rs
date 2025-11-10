//! OpLog Synchronization Service
//!
//! Provides efficient anti-entropy synchronization for OpLog CRDTs using
//! summary-based state comparison and incremental operation transfer.

use super::SelectionCriteria;
use super::{AttestedOp, DeviceId, Hash32, OpLog, OpLogSummary, PeerInfo};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, error, info, warn};

/// Configuration for OpLog synchronization
#[derive(Debug, Clone)]
pub struct SyncConfiguration {
    /// Maximum number of operations to transfer in one sync round
    pub max_operations_per_round: usize,
    /// Maximum time to wait for a sync response
    pub sync_timeout: Duration,
    /// Minimum interval between sync attempts with the same peer
    pub min_sync_interval: Duration,
    /// Maximum number of concurrent sync sessions
    pub max_concurrent_syncs: usize,
    /// Whether to enable compression for large operation transfers
    pub enable_compression: bool,
    /// Retry configuration
    pub retry_config: RetryConfiguration,
}

impl Default for SyncConfiguration {
    fn default() -> Self {
        Self {
            max_operations_per_round: 1000,
            sync_timeout: Duration::from_secs(30),
            min_sync_interval: Duration::from_secs(10),
            max_concurrent_syncs: 5,
            enable_compression: true,
            retry_config: RetryConfiguration::default(),
        }
    }
}

/// Retry configuration for failed synchronization attempts
#[derive(Debug, Clone)]
pub struct RetryConfiguration {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Base delay for exponential backoff
    pub base_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Jitter factor for randomizing delays (0.0 to 1.0)
    pub jitter_factor: f64,
}

impl Default for RetryConfiguration {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            jitter_factor: 0.1,
        }
    }
}

/// Errors that can occur during synchronization
#[derive(Debug, Error)]
pub enum SyncError {
    /// Peer not found
    #[error("Peer not found: {peer_id}")]
    PeerNotFound {
        /// The ID of the peer that was not found
        peer_id: DeviceId,
    },

    /// Synchronization timed out
    #[error("Sync timeout with peer: {peer_id}")]
    SyncTimeout {
        /// The ID of the peer that timed out
        peer_id: DeviceId,
    },

    /// Too many concurrent synchronization sessions
    #[error("Too many concurrent sync sessions")]
    TooManyConcurrentSyncs,

    /// Operation validation failed
    #[error("Operation validation failed: {reason}")]
    OperationValidation {
        /// The reason for validation failure
        reason: String,
    },

    /// Network error occurred
    #[error("Network error: {reason}")]
    NetworkError {
        /// The reason for the network error
        reason: String,
    },

    /// Serialization error occurred
    #[error("Serialization error: {reason}")]
    SerializationError {
        /// The reason for serialization failure
        reason: String,
    },

    /// Peer synchronization is already in progress
    #[error("Peer sync in progress: {peer_id}")]
    SyncInProgress {
        /// The ID of the peer with sync in progress
        peer_id: DeviceId,
    },

    /// Rate limit exceeded for peer
    #[error("Rate limit exceeded for peer: {peer_id}")]
    RateLimitExceeded {
        /// The ID of the peer exceeding rate limit
        peer_id: DeviceId,
    },

    /// Invalid synchronization state
    #[error("Invalid sync state: {reason}")]
    InvalidState {
        /// The reason the state is invalid
        reason: String,
    },
}

/// Result of a synchronization operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    /// Peer that was synchronized with
    pub peer_id: DeviceId,
    /// Number of operations received from peer
    pub operations_received: usize,
    /// Number of operations sent to peer
    pub operations_sent: usize,
    /// Duration of the synchronization
    pub sync_duration: Duration,
    /// Whether the synchronization was successful
    pub success: bool,
    /// Error message if synchronization failed
    pub error_message: Option<String>,
    /// Synchronization metrics
    pub metrics: SyncMetrics,
}

/// Metrics collected during synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMetrics {
    /// Size of summary comparison
    pub summary_size_bytes: usize,
    /// Size of operation transfer
    pub operation_transfer_bytes: usize,
    /// Time spent on network operations
    pub network_time: Duration,
    /// Time spent on CRDT operations
    pub crdt_time: Duration,
    /// Whether compression was used
    pub compression_used: bool,
    /// Compression ratio if used (original/compressed)
    pub compression_ratio: Option<f32>,
}

/// State of an active synchronization session
#[derive(Debug, Clone)]
struct ActiveSync {
    /// Peer being synchronized with
    peer_id: DeviceId,
    /// Start time of the sync
    start_time: Instant,
    /// Current sync state
    state: SyncSessionState,
}

/// States of a sync session
#[derive(Debug, Clone, PartialEq)]
enum SyncSessionState {
    /// Exchanging summaries
    ExchangingSummaries,
    /// Transferring operations
    TransferringOperations,
    /// Finalizing sync
    Finalizing,
}

/// OpLog synchronization service
pub struct OpLogSynchronizer {
    /// Local OpLog being synchronized
    local_oplog: OpLog,
    /// Configuration for synchronization behavior
    config: SyncConfiguration,
    /// Currently active sync sessions
    active_syncs: HashMap<DeviceId, ActiveSync>,
    /// Last sync time with each peer
    last_sync_times: HashMap<DeviceId, Instant>,
    /// Retry attempts for each peer
    retry_counts: HashMap<DeviceId, u32>,
    /// Known peers for synchronization
    known_peers: HashMap<DeviceId, PeerInfo>,
    /// Synchronization statistics
    sync_stats: SyncStatistics,
}

/// Accumulated statistics about synchronization
#[derive(Debug, Clone, Default)]
pub struct SyncStatistics {
    /// Total number of sync attempts
    pub total_sync_attempts: u64,
    /// Total number of successful syncs
    pub successful_syncs: u64,
    /// Total number of failed syncs
    pub failed_syncs: u64,
    /// Total operations received across all syncs
    pub total_operations_received: u64,
    /// Total operations sent across all syncs
    pub total_operations_sent: u64,
    /// Total time spent synchronizing
    pub total_sync_time: Duration,
    /// Average sync duration
    pub average_sync_duration: Duration,
}

impl OpLogSynchronizer {
    /// Create a new OpLog synchronizer
    pub fn new(local_oplog: OpLog, config: SyncConfiguration) -> Self {
        Self {
            local_oplog,
            config,
            active_syncs: HashMap::new(),
            last_sync_times: HashMap::new(),
            retry_counts: HashMap::new(),
            known_peers: HashMap::new(),
            sync_stats: SyncStatistics::default(),
        }
    }

    /// Add a known peer for synchronization
    pub fn add_peer(&mut self, peer: PeerInfo) {
        info!("Adding peer for synchronization: {}", peer.device_id);
        self.known_peers.insert(peer.device_id, peer);
    }

    /// Remove a peer
    pub fn remove_peer(&mut self, peer_id: &DeviceId) {
        info!("Removing peer: {}", peer_id);
        self.known_peers.remove(peer_id);
        self.last_sync_times.remove(peer_id);
        self.retry_counts.remove(peer_id);
        self.active_syncs.remove(peer_id);
    }

    /// Update local OpLog
    pub fn update_local_oplog(&mut self, oplog: OpLog) {
        debug!("Updating local OpLog with {} operations", oplog.len());
        self.local_oplog = oplog;
    }

    /// Get current local OpLog
    pub fn local_oplog(&self) -> &OpLog {
        &self.local_oplog
    }

    /// Start synchronization with a specific peer
    #[allow(clippy::disallowed_methods)]
    pub async fn sync_with_peer(&mut self, peer_id: DeviceId) -> Result<SyncResult, SyncError> {
        // Check if peer exists
        let peer_info = self
            .known_peers
            .get(&peer_id)
            .cloned()
            .ok_or(SyncError::PeerNotFound { peer_id })?;

        // Check rate limiting
        if let Some(last_sync) = self.last_sync_times.get(&peer_id) {
            if last_sync.elapsed() < self.config.min_sync_interval {
                return Err(SyncError::RateLimitExceeded { peer_id });
            }
        }

        // Check concurrent sync limit
        if self.active_syncs.len() >= self.config.max_concurrent_syncs {
            return Err(SyncError::TooManyConcurrentSyncs);
        }

        // Check if already syncing with this peer
        if self.active_syncs.contains_key(&peer_id) {
            return Err(SyncError::SyncInProgress { peer_id });
        }

        info!("Starting synchronization with peer: {}", peer_id);

        // Record sync start
        let start_time = Instant::now();
        let active_sync = ActiveSync {
            peer_id,
            start_time,
            state: SyncSessionState::ExchangingSummaries,
        };
        self.active_syncs.insert(peer_id, active_sync);
        self.sync_stats.total_sync_attempts += 1;

        // Execute synchronization
        let result = self.execute_sync(peer_id, peer_info).await;

        // Clean up and update statistics
        self.active_syncs.remove(&peer_id);
        self.last_sync_times.insert(peer_id, start_time);

        match &result {
            Ok(sync_result) => {
                info!(
                    "Synchronization with {} completed successfully: {} ops received, {} ops sent",
                    peer_id, sync_result.operations_received, sync_result.operations_sent
                );
                self.sync_stats.successful_syncs += 1;
                self.sync_stats.total_operations_received += sync_result.operations_received as u64;
                self.sync_stats.total_operations_sent += sync_result.operations_sent as u64;
                self.sync_stats.total_sync_time += sync_result.sync_duration;
                self.retry_counts.remove(&peer_id); // Reset retry count on success
            }
            Err(error) => {
                warn!("Synchronization with {} failed: {}", peer_id, error);
                self.sync_stats.failed_syncs += 1;

                // Update retry count
                let retry_count = self.retry_counts.entry(peer_id).or_insert(0);
                *retry_count += 1;
            }
        }

        // Update average sync duration
        if self.sync_stats.successful_syncs > 0 {
            self.sync_stats.average_sync_duration =
                self.sync_stats.total_sync_time / self.sync_stats.successful_syncs as u32;
        }

        result
    }

    /// Execute the actual synchronization protocol
    #[allow(clippy::disallowed_methods)]
    async fn execute_sync(
        &mut self,
        peer_id: DeviceId,
        _peer_info: PeerInfo,
    ) -> Result<SyncResult, SyncError> {
        let start_time = Instant::now();
        let mut metrics = SyncMetrics {
            summary_size_bytes: 0,
            operation_transfer_bytes: 0,
            network_time: Duration::ZERO,
            crdt_time: Duration::ZERO,
            compression_used: false,
            compression_ratio: None,
        };

        // Phase 1: Exchange summaries
        debug!("Phase 1: Exchanging summaries with {}", peer_id);
        let crdt_start = Instant::now();
        let local_summary = self.local_oplog.create_summary();
        metrics.crdt_time += crdt_start.elapsed();

        let network_start = Instant::now();
        let peer_summary = self.request_peer_summary(peer_id, &local_summary).await?;
        metrics.network_time += network_start.elapsed();
        metrics.summary_size_bytes = self.estimate_summary_size(&peer_summary);

        // Phase 2: Determine what operations to exchange
        debug!(
            "Phase 2: Determining operation differences with {}",
            peer_id
        );
        let crdt_start = Instant::now();
        let missing_from_local = local_summary.missing_cids(&peer_summary);
        let missing_from_peer = peer_summary.missing_cids(&local_summary);
        metrics.crdt_time += crdt_start.elapsed();

        debug!(
            "Synchronization plan: {} ops to receive, {} ops to send",
            missing_from_local.len(),
            missing_from_peer.len()
        );

        let mut operations_received = 0;
        let mut operations_sent = 0;

        // Phase 3: Receive missing operations
        if !missing_from_local.is_empty() {
            debug!(
                "Phase 3a: Receiving {} operations from {}",
                missing_from_local.len(),
                peer_id
            );

            let network_start = Instant::now();
            let received_ops = self
                .request_operations(peer_id, &missing_from_local)
                .await?;
            metrics.network_time += network_start.elapsed();

            let crdt_start = Instant::now();
            operations_received = self.apply_received_operations(received_ops)?;
            metrics.crdt_time += crdt_start.elapsed();

            metrics.operation_transfer_bytes += self.estimate_operations_size(operations_received);
        }

        // Phase 4: Send missing operations
        if !missing_from_peer.is_empty() {
            debug!(
                "Phase 3b: Sending {} operations to {}",
                missing_from_peer.len(),
                peer_id
            );

            let crdt_start = Instant::now();
            let ops_to_send = self.collect_operations_to_send(&missing_from_peer)?;
            metrics.crdt_time += crdt_start.elapsed();

            let network_start = Instant::now();
            self.send_operations(peer_id, &ops_to_send).await?;
            metrics.network_time += network_start.elapsed();

            operations_sent = ops_to_send.len();
            metrics.operation_transfer_bytes += self.estimate_operations_size(operations_sent);
        }

        let sync_duration = start_time.elapsed();

        Ok(SyncResult {
            peer_id,
            operations_received,
            operations_sent,
            sync_duration,
            success: true,
            error_message: None,
            metrics,
        })
    }

    /// Sync with all available peers
    pub async fn sync_with_all_peers(&mut self) -> Vec<Result<SyncResult, SyncError>> {
        let peers: Vec<DeviceId> = self.known_peers.keys().copied().collect();
        let mut results = Vec::new();

        info!("Starting synchronization with {} peers", peers.len());

        for peer_id in peers {
            let result = self.sync_with_peer(peer_id).await;
            results.push(result);

            // Small delay between syncs to avoid overwhelming the network
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        info!("Completed synchronization round with all peers");
        results
    }

    /// Sync with peers matching specific criteria
    #[allow(clippy::disallowed_methods)]
    pub async fn sync_with_criteria(
        &mut self,
        criteria: SelectionCriteria,
    ) -> Vec<Result<SyncResult, SyncError>> {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let matching_peers: Vec<DeviceId> = self
            .known_peers
            .values()
            .filter(|peer| criteria.matches(peer, current_time))
            .map(|peer| peer.device_id)
            .collect();

        info!(
            "Synchronizing with {} peers matching criteria",
            matching_peers.len()
        );

        let mut results = Vec::new();
        for peer_id in matching_peers {
            let result = self.sync_with_peer(peer_id).await;
            results.push(result);
        }

        results
    }

    /// Get synchronization statistics
    pub fn get_statistics(&self) -> &SyncStatistics {
        &self.sync_stats
    }

    /// Get active synchronization sessions
    pub fn get_active_syncs(&self) -> Vec<DeviceId> {
        self.active_syncs.keys().copied().collect()
    }

    /// Check if a peer needs synchronization based on last sync time
    pub fn peer_needs_sync(&self, peer_id: &DeviceId) -> bool {
        match self.last_sync_times.get(peer_id) {
            Some(last_sync) => last_sync.elapsed() >= self.config.min_sync_interval,
            None => true, // Never synced
        }
    }

    /// Get peers that need synchronization
    pub fn get_peers_needing_sync(&self) -> Vec<DeviceId> {
        self.known_peers
            .keys()
            .filter(|&peer_id| self.peer_needs_sync(peer_id))
            .copied()
            .collect()
    }

    // === Private helper methods ===

    /// Request summary from peer
    async fn request_peer_summary(
        &self,
        peer_id: DeviceId,
        _local_summary: &OpLogSummary,
    ) -> Result<OpLogSummary, SyncError> {
        // TODO fix - In a real implementation, this would use the transport layer
        // TODO fix - For now, we'll simulate this operation
        debug!("Requesting summary from peer: {}", peer_id);

        // Simulate network delay
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Return a mock summary - in real implementation this would be over the network
        Ok(OpLogSummary {
            version: 1,
            operation_count: 5,
            cids: BTreeSet::new(),
        })
    }

    /// Request specific operations from peer
    async fn request_operations(
        &self,
        peer_id: DeviceId,
        cids: &BTreeSet<Hash32>,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        debug!(
            "Requesting {} operations from peer: {}",
            cids.len(),
            peer_id
        );

        // Simulate network delay
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Return empty list TODO fix - For now - in real implementation this would fetch actual operations
        Ok(Vec::new())
    }

    /// Send operations to peer
    async fn send_operations(
        &self,
        peer_id: DeviceId,
        operations: &[AttestedOp],
    ) -> Result<(), SyncError> {
        debug!(
            "Sending {} operations to peer: {}",
            operations.len(),
            peer_id
        );

        // Simulate network delay
        tokio::time::sleep(Duration::from_millis(50)).await;

        Ok(())
    }

    /// Apply received operations to local OpLog
    fn apply_received_operations(
        &mut self,
        operations: Vec<AttestedOp>,
    ) -> Result<usize, SyncError> {
        let initial_count = self.local_oplog.len();

        for op in operations {
            // Validate operation before applying
            if let Err(reason) = self.validate_operation(&op) {
                return Err(SyncError::OperationValidation { reason });
            }

            self.local_oplog.add_operation(op);
        }

        Ok(self.local_oplog.len() - initial_count)
    }

    /// Collect operations to send to peer
    fn collect_operations_to_send(
        &self,
        cids: &BTreeSet<Hash32>,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        let mut operations = Vec::new();

        for cid in cids {
            if let Some(op) = self.local_oplog.get_operation(cid) {
                operations.push(op.clone());
            }
        }

        Ok(operations)
    }

    /// Validate an operation before applying it
    fn validate_operation(&self, _op: &AttestedOp) -> Result<(), String> {
        // Basic validation - in real implementation this would be comprehensive
        Ok(())
    }

    /// Estimate size of a summary in bytes
    fn estimate_summary_size(&self, summary: &OpLogSummary) -> usize {
        // Rough estimation: base size + CID size
        64 + summary.cids.len() * 32
    }

    /// Estimate size of operations in bytes
    fn estimate_operations_size(&self, count: usize) -> usize {
        // Rough estimation: average operation size
        count * 256
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::PeerMetrics;
    use aura_core::identifiers::{AccountId, DeviceId};
    fn create_test_peer(peer_id: DeviceId) -> PeerInfo {
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
    fn test_synchronizer_creation() {
        let oplog = OpLog::new();
        let config = SyncConfiguration::default();
        let synchronizer = OpLogSynchronizer::new(oplog, config);

        assert_eq!(synchronizer.local_oplog().len(), 0);
        assert_eq!(synchronizer.get_active_syncs().len(), 0);
    }

    #[test]
    fn test_peer_management() {
        let oplog = OpLog::new();
        let config = SyncConfiguration::default();
        let mut synchronizer = OpLogSynchronizer::new(oplog, config);

        let peer_id = DeviceId::new();
        let peer_info = create_test_peer(peer_id);

        synchronizer.add_peer(peer_info);
        assert_eq!(synchronizer.known_peers.len(), 1);
        assert!(synchronizer.peer_needs_sync(&peer_id));

        synchronizer.remove_peer(&peer_id);
        assert_eq!(synchronizer.known_peers.len(), 0);
    }

    #[tokio::test]
    async fn test_sync_with_nonexistent_peer() {
        let oplog = OpLog::new();
        let config = SyncConfiguration::default();
        let mut synchronizer = OpLogSynchronizer::new(oplog, config);

        let nonexistent_peer = DeviceId::new();
        let result = synchronizer.sync_with_peer(nonexistent_peer).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SyncError::PeerNotFound { .. }
        ));
    }

    #[test]
    fn test_peers_needing_sync() {
        let oplog = OpLog::new();
        let config = SyncConfiguration::default();
        let mut synchronizer = OpLogSynchronizer::new(oplog, config);

        let peer1 = DeviceId::new();
        let peer2 = DeviceId::new();

        synchronizer.add_peer(create_test_peer(peer1));
        synchronizer.add_peer(create_test_peer(peer2));

        let needing_sync = synchronizer.get_peers_needing_sync();
        assert_eq!(needing_sync.len(), 2);
        assert!(needing_sync.contains(&peer1));
        assert!(needing_sync.contains(&peer2));
    }

    #[test]
    fn test_sync_statistics() {
        let oplog = OpLog::new();
        let config = SyncConfiguration::default();
        let synchronizer = OpLogSynchronizer::new(oplog, config);

        let stats = synchronizer.get_statistics();
        assert_eq!(stats.total_sync_attempts, 0);
        assert_eq!(stats.successful_syncs, 0);
        assert_eq!(stats.failed_syncs, 0);
    }
}
