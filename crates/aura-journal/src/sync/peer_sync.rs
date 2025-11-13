//! Per-Peer Synchronization State Management
//!
//! Manages synchronization state and coordination for individual peers,
//! including sync session tracking, backoff logic, and peer-specific metrics.

use super::PeerInfo;
use super::{DeviceId, Hash32, OpLog, SyncMessage, SyncProtocol};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, info, warn};

/// Errors specific to peer synchronization
#[derive(Debug, Error)]
pub enum PeerSyncError {
    /// Peer synchronization timed out
    #[error("Peer synchronization timeout: {peer_id}")]
    SyncTimeout {
        /// The ID of the peer that timed out
        peer_id: DeviceId,
    },

    /// The synchronization state for the peer is invalid
    #[error("Invalid sync state for peer {peer_id}: {reason}")]
    InvalidSyncState {
        /// The ID of the peer with invalid state
        peer_id: DeviceId,
        /// Description of why the state is invalid
        reason: String,
    },

    /// Peer is currently in backoff mode
    #[error("Peer {peer_id} is currently backing off (retry at: {retry_at:?})")]
    BackoffActive {
        /// The ID of the peer backing off
        peer_id: DeviceId,
        /// When the peer can be retried
        retry_at: Instant,
    },

    /// An operation conflict was detected
    #[error("Operation conflict detected for peer {peer_id}: {operation_cid:?}")]
    OperationConflict {
        /// The ID of the peer with the conflict
        peer_id: DeviceId,
        /// The CID of the conflicting operation
        operation_cid: Hash32,
    },

    /// Maximum retry attempts exceeded for peer
    #[error("Peer {peer_id} exceeded maximum retry attempts ({max_retries})")]
    MaxRetriesExceeded {
        /// The ID of the peer that exceeded retries
        peer_id: DeviceId,
        /// The maximum number of retries
        max_retries: u32,
    },
}

/// State of synchronization with a specific peer
#[derive(Debug, Clone)]
pub struct PeerSyncState {
    /// Peer being synchronized with
    pub peer_id: DeviceId,
    /// Information about the peer
    pub peer_info: PeerInfo,
    /// Last successful synchronization time
    pub last_successful_sync: Option<Instant>,
    /// Last sync attempt time
    pub last_sync_attempt: Option<Instant>,
    /// Number of consecutive failures
    pub consecutive_failures: u32,
    /// Backoff until this time
    pub backoff_until: Option<Instant>,
    /// Operations pending to be sent to peer
    pub pending_operations: BTreeSet<Hash32>,
    /// Operations we're expecting from peer
    pub expected_operations: BTreeSet<Hash32>,
    /// Current sync session metrics
    pub session_metrics: PeerSessionMetrics,
}

impl PeerSyncState {
    /// Create new peer sync state
    pub fn new(peer_info: PeerInfo) -> Self {
        Self {
            peer_id: peer_info.device_id,
            peer_info,
            last_successful_sync: None,
            last_sync_attempt: None,
            consecutive_failures: 0,
            backoff_until: None,
            pending_operations: BTreeSet::new(),
            expected_operations: BTreeSet::new(),
            session_metrics: PeerSessionMetrics::default(),
        }
    }

    /// Check if peer can be synchronized now (not in backoff)
    #[allow(clippy::disallowed_methods)]
    pub fn can_sync_now(&self) -> bool {
        match self.backoff_until {
            Some(backoff_time) => Instant::now() >= backoff_time,
            None => true,
        }
    }

    /// Check if peer needs synchronization based on interval and pending operations
    pub fn needs_sync(&self, min_sync_interval: Duration) -> bool {
        if !self.can_sync_now() {
            return false;
        }

        // Always sync if we have pending operations
        if !self.pending_operations.is_empty() || !self.expected_operations.is_empty() {
            return true;
        }

        // Check if enough time has passed since last sync
        match self.last_successful_sync {
            Some(last_sync) => last_sync.elapsed() >= min_sync_interval,
            None => true, // Never synced
        }
    }

    /// Calculate backoff duration based on consecutive failures
    pub fn calculate_backoff_duration(
        &self,
        base_delay: Duration,
        max_delay: Duration,
        jitter_factor: f64,
    ) -> Duration {
        if self.consecutive_failures == 0 {
            return Duration::ZERO;
        }

        // Exponential backoff: base_delay * 2^(failures-1)
        let exponential_delay = base_delay.as_millis() as u64
            * (1u64 << (self.consecutive_failures.saturating_sub(1).min(10))); // Cap at 2^10

        let mut backoff =
            Duration::from_millis(exponential_delay.min(max_delay.as_millis() as u64));

        // Add jitter to prevent thundering herd
        if jitter_factor > 0.0 {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let mut hasher = DefaultHasher::new();
            self.peer_id.hash(&mut hasher);
            let hash = hasher.finish();

            let jitter_ratio = (hash % 1000) as f64 / 1000.0 * jitter_factor;
            let jitter = Duration::from_millis((backoff.as_millis() as f64 * jitter_ratio) as u64);
            backoff = backoff.saturating_add(jitter);
        }

        backoff
    }

    /// Record sync failure and update backoff
    #[allow(clippy::disallowed_methods)]
    pub fn record_sync_failure(
        &mut self,
        base_delay: Duration,
        max_delay: Duration,
        jitter_factor: f64,
    ) {
        self.consecutive_failures += 1;
        self.last_sync_attempt = Some(Instant::now());

        let backoff_duration =
            self.calculate_backoff_duration(base_delay, max_delay, jitter_factor);
        self.backoff_until = Some(Instant::now() + backoff_duration);

        warn!(
            "Sync failure with peer {} (failure #{}, backing off for {:?})",
            self.peer_id, self.consecutive_failures, backoff_duration
        );
    }

    /// Record successful sync
    #[allow(clippy::disallowed_methods)]
    pub fn record_sync_success(&mut self) {
        self.last_successful_sync = Some(Instant::now());
        self.last_sync_attempt = Some(Instant::now());
        self.consecutive_failures = 0;
        self.backoff_until = None;
        self.pending_operations.clear();
        self.expected_operations.clear();

        info!("Successful sync with peer {}", self.peer_id);
    }

    /// Add operations that need to be sent to this peer
    pub fn add_pending_operations(&mut self, cids: BTreeSet<Hash32>) {
        self.pending_operations.extend(cids);
    }

    /// Add operations we expect to receive from this peer
    pub fn add_expected_operations(&mut self, cids: BTreeSet<Hash32>) {
        self.expected_operations.extend(cids);
    }

    /// Remove operation from pending set (when successfully sent)
    pub fn mark_operation_sent(&mut self, cid: &Hash32) {
        self.pending_operations.remove(cid);
    }

    /// Remove operation from expected set (when successfully received)
    pub fn mark_operation_received(&mut self, cid: &Hash32) {
        self.expected_operations.remove(cid);
    }

    /// Get sync priority score (higher = more urgent)
    pub fn get_sync_priority(&self) -> u32 {
        let mut priority = 0u32;

        // High priority if we have many pending operations
        priority += (self.pending_operations.len() as u32 * 10).min(100);
        priority += (self.expected_operations.len() as u32 * 5).min(50);

        // Higher priority if we haven't synced recently
        if let Some(last_sync) = self.last_successful_sync {
            let hours_since_sync = last_sync.elapsed().as_secs() / 3600;
            priority += (hours_since_sync as u32).min(50);
        } else {
            priority += 100; // Never synced
        }

        // Lower priority if peer has many consecutive failures
        priority = priority.saturating_sub(self.consecutive_failures * 5);

        // Adjust based on peer metrics
        priority =
            ((priority as f64) * (self.peer_info.metrics.reliability_score as f64 / 100.0)) as u32;

        priority
    }
}

/// Metrics for a specific peer sync session
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PeerSessionMetrics {
    /// Total operations sent in current session
    pub operations_sent: usize,
    /// Total operations received in current session
    pub operations_received: usize,
    /// Session start time
    #[serde(skip)]
    pub session_start: Option<Instant>,
    /// Total time spent in sync operations
    #[serde(skip)]
    pub total_sync_time: Duration,
    /// Number of round trips in current session
    pub round_trips: u32,
}

impl PeerSessionMetrics {
    /// Start a new session
    #[allow(clippy::disallowed_methods)]
    pub fn start_session(&mut self) {
        self.session_start = Some(Instant::now());
        self.operations_sent = 0;
        self.operations_received = 0;
        self.round_trips = 0;
    }

    /// Record operations sent
    pub fn record_operations_sent(&mut self, count: usize) {
        self.operations_sent += count;
    }

    /// Record operations received
    pub fn record_operations_received(&mut self, count: usize) {
        self.operations_received += count;
    }

    /// Record a round trip
    pub fn record_round_trip(&mut self) {
        self.round_trips += 1;
    }

    /// Complete the session and return total time
    pub fn complete_session(&mut self) -> Duration {
        if let Some(start) = self.session_start {
            let duration = start.elapsed();
            self.total_sync_time += duration;
            self.session_start = None;
            duration
        } else {
            Duration::ZERO
        }
    }
}

/// Manager for per-peer synchronization state
pub struct PeerSyncManager {
    /// Sync state for each peer
    peer_states: HashMap<DeviceId, PeerSyncState>,
    /// Protocol instances for active syncs
    active_protocols: HashMap<DeviceId, SyncProtocol>,
    /// Configuration for backoff and retry logic
    max_retries: u32,
    base_delay: Duration,
    max_delay: Duration,
    jitter_factor: f64,
}

impl PeerSyncManager {
    /// Create a new peer sync manager
    pub fn new(
        max_retries: u32,
        base_delay: Duration,
        max_delay: Duration,
        jitter_factor: f64,
    ) -> Self {
        Self {
            peer_states: HashMap::new(),
            active_protocols: HashMap::new(),
            max_retries,
            base_delay,
            max_delay,
            jitter_factor,
        }
    }

    /// Add or update a peer
    pub fn add_peer(&mut self, peer_info: PeerInfo) {
        let peer_id = peer_info.device_id;

        if let Some(existing_state) = self.peer_states.get_mut(&peer_id) {
            existing_state.peer_info = peer_info;
        } else {
            let peer_state = PeerSyncState::new(peer_info);
            self.peer_states.insert(peer_id, peer_state);
        }
    }

    /// Remove a peer
    pub fn remove_peer(&mut self, peer_id: &DeviceId) {
        self.peer_states.remove(peer_id);
        self.active_protocols.remove(peer_id);
    }

    /// Get peers that need synchronization
    pub fn get_peers_needing_sync(&self, min_sync_interval: Duration) -> Vec<DeviceId> {
        self.peer_states
            .values()
            .filter(|state| state.needs_sync(min_sync_interval))
            .map(|state| state.peer_id)
            .collect()
    }

    /// Get peers sorted by sync priority
    pub fn get_peers_by_priority(&self, min_sync_interval: Duration) -> Vec<(DeviceId, u32)> {
        let mut peers_with_priority: Vec<_> = self
            .peer_states
            .values()
            .filter(|state| state.needs_sync(min_sync_interval))
            .map(|state| (state.peer_id, state.get_sync_priority()))
            .collect();

        peers_with_priority.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by priority (descending)
        peers_with_priority
    }

    /// Start sync with a specific peer
    pub fn start_sync(
        &mut self,
        peer_id: DeviceId,
        local_oplog: OpLog,
    ) -> Result<SyncMessage, PeerSyncError> {
        let peer_state =
            self.peer_states
                .get_mut(&peer_id)
                .ok_or_else(|| PeerSyncError::InvalidSyncState {
                    peer_id,
                    reason: "Peer not found".to_string(),
                })?;

        // Check if peer can sync now
        if !peer_state.can_sync_now() {
            #[allow(clippy::expect_used)]
            let retry_at = peer_state
                .backoff_until
                .expect("backoff_until must be Some when can_sync_now is false");
            return Err(PeerSyncError::BackoffActive { peer_id, retry_at });
        }

        // Check retry limit
        if peer_state.consecutive_failures >= self.max_retries {
            return Err(PeerSyncError::MaxRetriesExceeded {
                peer_id,
                max_retries: self.max_retries,
            });
        }

        // Start session metrics
        peer_state.session_metrics.start_session();

        // Create and start sync protocol
        let mut protocol = SyncProtocol::new(local_oplog.local_device_id(), local_oplog);
        let sync_message =
            protocol
                .start_sync(peer_id)
                .map_err(|e| PeerSyncError::InvalidSyncState {
                    peer_id,
                    reason: format!("Protocol error: {}", e),
                })?;

        self.active_protocols.insert(peer_id, protocol);

        debug!("Started sync with peer {}", peer_id);
        Ok(sync_message)
    }

    /// Handle incoming sync message for a peer
    pub fn handle_sync_message(
        &mut self,
        peer_id: DeviceId,
        message: SyncMessage,
    ) -> Result<Option<SyncMessage>, PeerSyncError> {
        let protocol = self.active_protocols.get_mut(&peer_id).ok_or_else(|| {
            PeerSyncError::InvalidSyncState {
                peer_id,
                reason: "No active sync protocol".to_string(),
            }
        })?;

        let response =
            protocol
                .handle_message(message)
                .map_err(|e| PeerSyncError::InvalidSyncState {
                    peer_id,
                    reason: format!("Protocol error: {}", e),
                })?;

        // Update session metrics
        if let Some(peer_state) = self.peer_states.get_mut(&peer_id) {
            peer_state.session_metrics.record_round_trip();
        }

        Ok(response)
    }

    /// Complete sync with a peer (success)
    pub fn complete_sync(
        &mut self,
        peer_id: DeviceId,
        operations_sent: usize,
        operations_received: usize,
    ) {
        // Remove active protocol
        self.active_protocols.remove(&peer_id);

        if let Some(peer_state) = self.peer_states.get_mut(&peer_id) {
            peer_state
                .session_metrics
                .record_operations_sent(operations_sent);
            peer_state
                .session_metrics
                .record_operations_received(operations_received);
            peer_state.session_metrics.complete_session();
            peer_state.record_sync_success();
        }
    }

    /// Fail sync with a peer
    pub fn fail_sync(&mut self, peer_id: DeviceId, _error: &str) {
        // Remove active protocol
        self.active_protocols.remove(&peer_id);

        if let Some(peer_state) = self.peer_states.get_mut(&peer_id) {
            peer_state.session_metrics.complete_session();
            peer_state.record_sync_failure(self.base_delay, self.max_delay, self.jitter_factor);
        }
    }

    /// Get sync state for a peer
    pub fn get_peer_state(&self, peer_id: &DeviceId) -> Option<&PeerSyncState> {
        self.peer_states.get(peer_id)
    }

    /// Get mutable sync state for a peer
    pub fn get_peer_state_mut(&mut self, peer_id: &DeviceId) -> Option<&mut PeerSyncState> {
        self.peer_states.get_mut(peer_id)
    }

    /// Get all peer states
    pub fn get_all_peer_states(&self) -> impl Iterator<Item = &PeerSyncState> {
        self.peer_states.values()
    }

    /// Check if any syncs are active
    pub fn has_active_syncs(&self) -> bool {
        !self.active_protocols.is_empty()
    }

    /// Get active sync count
    pub fn active_sync_count(&self) -> usize {
        self.active_protocols.len()
    }

    /// Clean up stale sync sessions (called periodically)
    #[allow(clippy::disallowed_methods)]
    pub fn cleanup_stale_sessions(&mut self, timeout: Duration) {
        let now = Instant::now();
        let mut stale_peers = Vec::new();

        for (peer_id, peer_state) in &self.peer_states {
            if let Some(start_time) = peer_state.session_metrics.session_start {
                if now.duration_since(start_time) > timeout {
                    stale_peers.push(*peer_id);
                }
            }
        }

        for peer_id in stale_peers {
            warn!("Cleaning up stale sync session for peer {}", peer_id);
            self.fail_sync(peer_id, "Session timeout");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{PeerInfo, PeerMetrics};
    use aura_core::identifiers::DeviceId;
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
    fn test_peer_sync_state_creation() {
        let peer_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let peer_info = create_test_peer_info(peer_id);
        let state = PeerSyncState::new(peer_info);

        assert_eq!(state.peer_id, peer_id);
        assert_eq!(state.consecutive_failures, 0);
        assert!(state.can_sync_now());
        assert!(state.needs_sync(Duration::from_secs(10)));
    }

    #[test]
    fn test_backoff_calculation() {
        let peer_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let peer_info = create_test_peer_info(peer_id);
        let mut state = PeerSyncState::new(peer_info);

        // No failures = no backoff
        let backoff = state.calculate_backoff_duration(
            Duration::from_millis(100),
            Duration::from_secs(30),
            0.0,
        );
        assert_eq!(backoff, Duration::ZERO);

        // One failure = base delay
        state.consecutive_failures = 1;
        let backoff = state.calculate_backoff_duration(
            Duration::from_millis(100),
            Duration::from_secs(30),
            0.0,
        );
        assert_eq!(backoff, Duration::from_millis(100));

        // Two failures = 2 * base delay
        state.consecutive_failures = 2;
        let backoff = state.calculate_backoff_duration(
            Duration::from_millis(100),
            Duration::from_secs(30),
            0.0,
        );
        assert_eq!(backoff, Duration::from_millis(200));
    }

    #[test]
    fn test_sync_priority_calculation() {
        let peer_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let peer_info = create_test_peer_info(peer_id);
        let mut state = PeerSyncState::new(peer_info);

        // Base priority for never-synced peer
        let priority = state.get_sync_priority();
        assert!(priority > 50); // Should be high priority

        // Add pending operations
        state
            .pending_operations
            .insert(aura_core::Hash32([0u8; 32]));
        state
            .pending_operations
            .insert(aura_core::Hash32([1u8; 32]));
        let priority_with_pending = state.get_sync_priority();
        assert!(priority_with_pending > priority);

        // Record failures
        state.consecutive_failures = 3;
        let priority_with_failures = state.get_sync_priority();
        assert!(priority_with_failures < priority_with_pending);
    }

    #[test]
    fn test_peer_sync_manager() {
        let mut manager =
            PeerSyncManager::new(3, Duration::from_millis(100), Duration::from_secs(30), 0.1);

        let peer_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let peer_info = create_test_peer_info(peer_id);

        // Add peer
        manager.add_peer(peer_info);
        assert_eq!(manager.peer_states.len(), 1);

        // Check peers needing sync
        let needing_sync = manager.get_peers_needing_sync(Duration::from_secs(10));
        assert_eq!(needing_sync.len(), 1);
        assert_eq!(needing_sync[0], peer_id);

        // Remove peer
        manager.remove_peer(&peer_id);
        assert_eq!(manager.peer_states.len(), 0);
    }

    #[test]
    fn test_session_metrics() {
        let mut metrics = PeerSessionMetrics::default();

        metrics.start_session();
        assert!(metrics.session_start.is_some());

        metrics.record_operations_sent(5);
        metrics.record_operations_received(3);
        metrics.record_round_trip();

        assert_eq!(metrics.operations_sent, 5);
        assert_eq!(metrics.operations_received, 3);
        assert_eq!(metrics.round_trips, 1);

        let duration = metrics.complete_session();
        assert!(duration > Duration::ZERO);
        assert!(metrics.session_start.is_none());
    }
}
