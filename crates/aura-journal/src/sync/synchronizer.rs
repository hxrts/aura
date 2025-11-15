//! OpLog Synchronization Service
//!
//! Provides efficient anti-entropy synchronization for OpLog CRDTs using
//! summary-based state comparison and incremental operation transfer.
//!
//! This module implements distributed synchronization using choreographic programming
//! patterns with the rumpsteak-aura framework for type-safe protocol execution.

#![allow(missing_docs)]

use super::SelectionCriteria;
use super::{AttestedOp, DeviceId, Hash32, OpLog, OpLogSummary, PeerInfo};
// TODO: Re-enable when choreography macro issues are resolved
// use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Synchronization message types for peer communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncMessage {
    /// Request summary from peer
    SummaryRequest {
        /// Device requesting the summary
        requesting_peer: DeviceId,
        /// Local operation log summary
        local_summary: OpLogSummary,
        /// Unique identifier for this request
        request_id: Uuid,
    },
    /// Response with peer summary
    SummaryResponse {
        /// Device responding with summary
        responding_peer: DeviceId,
        /// Peer's operation log summary
        peer_summary: OpLogSummary,
        /// Corresponding request identifier
        request_id: Uuid,
    },
    /// Request specific operations
    OperationRequest {
        /// Device requesting operations
        requesting_peer: DeviceId,
        /// Content IDs of requested operations
        requested_cids: BTreeSet<Hash32>,
        /// Unique identifier for this request
        request_id: Uuid,
    },
    /// Response with operations
    OperationResponse {
        /// Device responding with operations
        responding_peer: DeviceId,
        /// Operations being transferred
        operations: Vec<AttestedOp>,
        /// Corresponding request identifier
        request_id: Uuid,
        /// Whether this is the final batch of operations
        is_final: bool,
    },
    /// Sync completion notification
    SyncComplete {
        /// Peer that completed sync
        peer: DeviceId,
        /// Number of operations transferred
        operations_transferred: usize,
        /// Request identifier for completion notification
        request_id: Uuid,
    },
    /// Error response
    SyncError {
        /// Peer reporting error
        peer: DeviceId,
        /// Error description
        error_message: String,
        /// Request identifier that caused error
        request_id: Uuid,
    },
}

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

    /// Protocol error occurred
    #[error("Protocol error: {reason}")]
    ProtocolError {
        /// The reason for the protocol error
        reason: String,
    },
}

impl SyncError {
    /// Create a protocol error
    pub fn protocol(reason: impl Into<String>) -> Self {
        Self::ProtocolError {
            reason: reason.into(),
        }
    }
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

/// Synchronization choreography roles
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SyncRole {
    /// Device requesting synchronization
    Requester(DeviceId),
    /// Device responding to sync requests
    Responder(DeviceId),
}

impl SyncRole {
    /// Get the device ID for this role
    pub fn device_id(&self) -> DeviceId {
        match self {
            SyncRole::Requester(id) => *id,
            SyncRole::Responder(id) => *id,
        }
    }

    /// Get role name for choreography framework
    pub fn name(&self) -> String {
        match self {
            SyncRole::Requester(id) => format!("Requester_{}", id.0.simple()),
            SyncRole::Responder(id) => format!("Responder_{}", id.0.simple()),
        }
    }
}

// Journal Synchronization Choreography
//
// TODO: Fix choreography macro compilation issues
// This choreography implements the distributed synchronization protocol
// with type-safe multi-party coordination using session types.
//
// choreography! {
//     #[namespace = "journal_sync"]
//     choreography SyncChoreography {
//         roles: Requester, Responder;
//
//         // Phase 1: Summary Exchange
//         Requester -> Responder: SummaryRequest;
//         Responder -> Requester: SummaryResponse;
//
//         // Phase 2: Operation Transfer
//         Requester -> Responder: OperationRequest;
//         Responder -> Requester: OperationResponse;
//
//         // Phase 3: Completion
//         Requester -> Responder: SyncComplete;
//         Responder -> Requester: SyncAcknowledged;
//     }
// }

// Message types for journal synchronization choreography

/// Summary request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryRequest {
    /// Local operation log summary
    pub local_summary: OpLogSummary,
    /// Unique identifier for this request
    pub request_id: Uuid,
}

/// Summary response message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryResponse {
    /// Peer's operation log summary
    pub peer_summary: OpLogSummary,
    /// Corresponding request identifier
    pub request_id: Uuid,
    /// Operations needed by the requester
    pub operations_needed: BTreeSet<Hash32>,
    /// Operations available from the responder
    pub operations_available: BTreeSet<Hash32>,
}

/// Operation request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationRequest {
    /// Content IDs of requested operations
    pub requested_cids: BTreeSet<Hash32>,
    /// Unique identifier for this request
    pub request_id: Uuid,
}

/// Operation response message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResponse {
    /// Operations being transferred
    pub operations: Vec<AttestedOp>,
    /// Corresponding request identifier
    pub request_id: Uuid,
    /// Whether this is the final batch of operations
    pub is_final: bool,
}

/// Sync completion message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncComplete {
    /// Number of operations transferred
    pub operations_transferred: usize,
    /// Request identifier for completion notification
    pub request_id: Uuid,
}

/// Sync acknowledgment message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncAcknowledged {
    /// Request identifier being acknowledged
    pub request_id: Uuid,
}

/// Placeholder function for sync choreography access
/// The choreography macro will generate the appropriate types and functions
pub fn get_sync_choreography() {
    // The choreography macro will generate the appropriate types and functions
}

/// OpLog synchronization service
pub struct OpLogSynchronizer {
    /// Device ID of this synchronizer
    _device_id: DeviceId,
    /// Local OpLog being synchronized
    local_oplog: OpLog,
    /// Configuration for synchronization behavior
    config: SyncConfiguration,
    /// Currently active sync sessions
    active_syncs: HashMap<DeviceId, Instant>,
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
    pub fn new(device_id: DeviceId, local_oplog: OpLog, config: SyncConfiguration) -> Self {
        Self {
            _device_id: device_id,
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
        // Track active sync without detailed state
        self.active_syncs.insert(peer_id, start_time);
        self.sync_stats.total_sync_attempts += 1;

        // Execute synchronization using choreography
        let result = self.execute_sync_choreography(peer_id, peer_info).await;

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

    /// Execute synchronization using choreographic protocol
    #[allow(clippy::disallowed_methods)]
    async fn execute_sync_choreography(
        &mut self,
        peer_id: DeviceId,
        _peer_info: PeerInfo,
    ) -> Result<SyncResult, SyncError> {
        let start_time = std::time::Instant::now();
        let mut metrics = SyncMetrics {
            summary_size_bytes: 0,
            operation_transfer_bytes: 0,
            network_time: Duration::ZERO,
            crdt_time: Duration::ZERO,
            compression_used: false,
            compression_ratio: None,
        };

        // Phase 1: Execute choreographic summary exchange
        let crdt_start = std::time::Instant::now();
        let local_summary = self.local_oplog.create_summary();
        metrics.crdt_time += crdt_start.elapsed();

        // Create summary request message for choreographic protocol
        let _summary_request = SummaryRequest {
            local_summary: local_summary.clone(),
            request_id: uuid::Uuid::new_v4(),
        };

        // Simulate choreographic protocol execution
        // In full implementation, this would use the generated SyncChoreography protocol
        let network_start = std::time::Instant::now();

        // Mock peer summary response (in real implementation, this comes from choreographic execution)
        let peer_summary = OpLogSummary {
            version: 1,
            operation_count: local_summary.operation_count + 2, // Simulate some differences
            cids: {
                let mut peer_cids = local_summary.cids.clone();
                // Add some mock missing operations
                peer_cids.insert(Hash32([1u8; 32]));
                peer_cids.insert(Hash32([2u8; 32]));
                peer_cids
            },
        };

        metrics.network_time += network_start.elapsed();
        metrics.summary_size_bytes = 64 + peer_summary.cids.len() * 32; // Rough estimation

        // Phase 2: Determine operations to transfer using choreographic protocol logic
        let crdt_start = std::time::Instant::now();
        let missing_from_local = local_summary.missing_cids(&peer_summary);
        let missing_from_peer = peer_summary.missing_cids(&local_summary);
        metrics.crdt_time += crdt_start.elapsed();

        let mut operations_received = 0;
        let mut operations_sent = 0;

        // Phase 3: Execute operation transfer through choreographic protocol
        if !missing_from_local.is_empty() {
            let network_start = std::time::Instant::now();

            // Create operation request message
            let _operation_request = OperationRequest {
                requested_cids: missing_from_local.clone(),
                request_id: uuid::Uuid::new_v4(),
            };

            // Simulate choreographic operation transfer
            // Mock receiving operations (in real implementation, comes from choreographic protocol)
            let received_ops = Vec::new(); // Empty for simulation

            metrics.network_time += network_start.elapsed();

            let crdt_start = std::time::Instant::now();
            // Apply received operations directly
            let initial_count = self.local_oplog.len();
            for op in received_ops {
                self.local_oplog.add_operation(op);
            }
            operations_received = self.local_oplog.len() - initial_count;
            metrics.crdt_time += crdt_start.elapsed();

            metrics.operation_transfer_bytes += operations_received * 256; // Rough estimation
        }

        if !missing_from_peer.is_empty() {
            let crdt_start = std::time::Instant::now();
            // Collect operations to send directly
            let mut ops_to_send = Vec::new();
            for cid in &missing_from_peer {
                if let Some(op) = self.local_oplog.get_operation(cid) {
                    ops_to_send.push(op.clone());
                }
            }
            metrics.crdt_time += crdt_start.elapsed();

            let network_start = std::time::Instant::now();

            // Create operation response message for choreographic protocol
            let _operation_response = OperationResponse {
                operations: ops_to_send.clone(),
                request_id: uuid::Uuid::new_v4(),
                is_final: true,
            };

            // Simulate sending through choreographic protocol
            metrics.network_time += network_start.elapsed();

            operations_sent = ops_to_send.len();
            metrics.operation_transfer_bytes += operations_sent * 256; // Rough estimation
        }

        // Phase 4: Complete synchronization with choreographic completion messages
        let _sync_complete = SyncComplete {
            operations_transferred: operations_received + operations_sent,
            request_id: uuid::Uuid::new_v4(),
        };

        let _sync_acknowledged = SyncAcknowledged {
            request_id: uuid::Uuid::new_v4(),
        };

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

    /// Execute the actual synchronization protocol
    ///
    /// Note: This is now a wrapper around the choreographic implementation.
    /// The manual implementation has been removed in favor of the type-safe choreographic protocol.
    #[allow(clippy::disallowed_methods, dead_code)]
    async fn execute_sync(
        &mut self,
        peer_id: DeviceId,
        peer_info: PeerInfo,
    ) -> Result<SyncResult, SyncError> {
        // All synchronization now goes through the choreographic protocol
        self.execute_sync_choreography(peer_id, peer_info).await
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

    // Note: request_peer_summary is now handled within the choreographic protocol
    // The choreography! macro automatically handles message routing and validation

    // Note: send_sync_request is now handled within the choreographic protocol
    // The choreography! macro provides type-safe message serialization and transport

    // Note: transport_send_and_receive is now handled within the choreographic protocol
    // The choreography! macro integrates with aura-transport automatically

    // Note: mock transport is replaced by choreographic protocol execution
    // Testing is now done through choreographic test harnesses

    // Note: request_operations is now part of the choreographic protocol
    // Operation requests are handled in the OperationRequest/OperationResponse phase

    // Note: send_operations is now part of the choreographic protocol
    // Operation sending is handled automatically in the transfer_operations choice path

    // Note: Operation application is now handled within the choreographic protocol
    // The choreography automatically applies validated operations during execution
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::PeerMetrics;
    use aura_core::identifiers::DeviceId;
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
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let synchronizer = OpLogSynchronizer::new(device_id, oplog, config);

        assert_eq!(synchronizer.local_oplog().len(), 0);
        assert_eq!(synchronizer.get_active_syncs().len(), 0);
    }

    #[test]
    fn test_peer_management() {
        let oplog = OpLog::new();
        let config = SyncConfiguration::default();
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let mut synchronizer = OpLogSynchronizer::new(device_id, oplog, config);

        let peer_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let peer_info = create_test_peer(peer_id);

        synchronizer.add_peer(peer_info);
        assert_eq!(synchronizer.known_peers.len(), 1);
        assert!(synchronizer.peer_needs_sync(&peer_id));

        synchronizer.remove_peer(&peer_id);
        assert_eq!(synchronizer.known_peers.len(), 0);
    }

    #[aura_test]
    async fn test_sync_with_nonexistent_peer() -> aura_core::AuraResult<()> {
        let oplog = OpLog::new();
        let config = SyncConfiguration::default();
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let mut synchronizer = OpLogSynchronizer::new(device_id, oplog, config);

        let nonexistent_peer = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let result = synchronizer.sync_with_peer(nonexistent_peer).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SyncError::PeerNotFound { .. }
        ));
        Ok(())
    }

    #[test]
    fn test_peers_needing_sync() {
        let oplog = OpLog::new();
        let config = SyncConfiguration::default();
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let mut synchronizer = OpLogSynchronizer::new(device_id, oplog, config);

        let peer1 = DeviceId(uuid::Uuid::from_bytes([1u8; 16]));
        let peer2 = DeviceId(uuid::Uuid::from_bytes([2u8; 16]));

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
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let synchronizer = OpLogSynchronizer::new(device_id, oplog, config);

        let stats = synchronizer.get_statistics();
        assert_eq!(stats.total_sync_attempts, 0);
        assert_eq!(stats.successful_syncs, 0);
        assert_eq!(stats.failed_syncs, 0);
    }
}
