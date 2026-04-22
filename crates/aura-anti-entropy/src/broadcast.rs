//! Eager-push/lazy-pull broadcast protocol for operation dissemination.
//!
//! Implements operation broadcast with rate limiting, back pressure handling,
//! and configurable eager push to neighbors and lazy pull on request.

use super::effects::{
    validate_remote_attested_op, BloomDigest, SyncEffects, SyncError, SyncMetrics,
};
use async_lock::RwLock;
use async_trait::async_trait;
use aura_core::effects::NetworkEffects;
use aura_core::types::identifiers::{ContextId, DeviceId};
use aura_core::{tree::AttestedOp, Hash32};
use std::collections::VecDeque;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Configuration for broadcast behavior
#[derive(Debug, Clone)]
pub struct BroadcastConfig {
    /// Maximum operations to push per peer per interval
    pub max_ops_per_peer: usize,
    /// Maximum total operation pushes across all peers per interval
    pub max_global_fanout_per_interval: usize,
    /// Egress threshold that activates backoff until rate limits reset.
    pub backoff_fanout_threshold_per_interval: usize,
    /// Maximum pending announcements before applying back pressure
    pub max_pending_announcements: usize,
    /// Enable eager push to immediate neighbors
    pub eager_push_enabled: bool,
    /// Enable lazy pull on request
    pub lazy_pull_enabled: bool,
    /// Maximum number of ops to keep in the in-memory oplog cache
    pub max_oplog_entries: usize,
}

impl Default for BroadcastConfig {
    fn default() -> Self {
        Self {
            max_ops_per_peer: 100,
            max_global_fanout_per_interval: 1_000,
            backoff_fanout_threshold_per_interval: 800,
            max_pending_announcements: 1000,
            eager_push_enabled: true,
            lazy_pull_enabled: true,
            max_oplog_entries: 10_000,
        }
    }
}

/// Handler implementing operation broadcast protocol
///
/// Provides two dissemination strategies:
/// 1. Eager push: Immediately push new operations to immediate neighbors
/// 2. Lazy pull: Respond to requests from peers for specific operations
///
/// Includes rate limiting and back pressure handling to prevent overwhelming peers.
/// When network effects are configured, sends go through actual network transport.
pub struct BroadcasterHandler {
    config: BroadcastConfig,
    state: RwLock<BroadcasterState>,
    /// Context ID for guard chain operations
    context_id: ContextId,
    /// Optional network effects for actual message transport
    network: Option<Arc<dyn NetworkEffects + Send + Sync>>,
}

#[derive(Default)]
struct BroadcasterState {
    /// Local operation store
    oplog: BTreeMap<Hash32, AttestedOp>,
    /// Insertion order for oplog eviction
    oplog_order: VecDeque<Hash32>,
    peers: BTreeSet<DeviceId>,
    /// Pending announcements (CID -> set of peers that need it)
    pending_announcements: BTreeMap<Hash32, BTreeSet<DeviceId>>,
    /// Rate limiting: peer -> count of ops pushed in current interval
    rate_limits: BTreeMap<DeviceId, usize>,
    /// Global egress fanout used in the current interval
    global_fanout_used: usize,
    /// Whether adaptive egress backoff is active for the current interval
    fanout_backoff_active: bool,
}

impl BroadcasterState {
    fn insert_op_bounded(&mut self, op: AttestedOp, max_oplog_entries: usize) {
        let cid = Hash32::from(op.op.parent_commitment);
        if !self.oplog.contains_key(&cid) {
            self.oplog_order.push_back(cid);
        }
        self.oplog.insert(cid, op);
        self.trim_oplog(max_oplog_entries);
    }

    fn merge_op_bounded(&mut self, op: AttestedOp, max_oplog_entries: usize) {
        let cid = Hash32::from(op.op.parent_commitment);
        if !self.oplog.contains_key(&cid) {
            self.oplog_order.push_back(cid);
        }
        self.oplog.entry(cid).or_insert(op);
        self.trim_oplog(max_oplog_entries);
    }

    fn trim_oplog(&mut self, max_oplog_entries: usize) {
        while self.oplog_order.len() > max_oplog_entries {
            if let Some(oldest) = self.oplog_order.pop_front() {
                self.oplog.remove(&oldest);
            }
        }
    }

    fn has_back_pressure(&self, max_pending_announcements: usize) -> bool {
        self.pending_announcements.len() >= max_pending_announcements
    }

    fn eligible_peers(
        &mut self,
        peers: Vec<DeviceId>,
        max_ops_per_peer: usize,
        max_global_fanout_per_interval: usize,
        backoff_fanout_threshold_per_interval: usize,
    ) -> Vec<DeviceId> {
        let mut eligible_peers = Vec::new();
        if self.fanout_backoff_active {
            return eligible_peers;
        }
        for peer in peers {
            if self.global_fanout_used >= max_global_fanout_per_interval {
                break;
            }
            let count = self.rate_limits.entry(peer).or_insert(0);
            if *count < max_ops_per_peer {
                eligible_peers.push(peer);
                *count += 1;
                self.global_fanout_used += 1;
                if self.global_fanout_used >= backoff_fanout_threshold_per_interval {
                    self.fanout_backoff_active = true;
                    break;
                }
            }
        }
        eligible_peers
    }
}

impl BroadcasterHandler {
    pub fn new(config: BroadcastConfig, context_id: ContextId) -> Self {
        Self {
            config,
            state: RwLock::new(BroadcasterState::default()),
            context_id,
            network: None,
        }
    }

    /// Create a broadcaster with network effects for actual transport
    pub fn with_network(
        config: BroadcastConfig,
        context_id: ContextId,
        network: Arc<dyn NetworkEffects + Send + Sync>,
    ) -> Self {
        Self {
            config,
            state: RwLock::new(BroadcasterState::default()),
            context_id,
            network: Some(network),
        }
    }

    /// Eager push: Send operation to all immediate neighbors
    ///
    /// Implements back pressure by checking pending announcements queue.
    /// If queue is full, returns error indicating back pressure.
    async fn eager_push_to_neighbors(&self, op: AttestedOp) -> Result<(), SyncError> {
        if !self.config.eager_push_enabled {
            return Ok(());
        }

        // Check back pressure
        let has_back_pressure = {
            let state = self.state.read().await;
            state.has_back_pressure(self.config.max_pending_announcements)
        };
        if has_back_pressure {
            return Err(SyncError::BackPressure);
        }

        let peers = self.get_connected_peers().await?;
        let cid = Hash32::from(op.op.parent_commitment);

        // Apply rate limiting per peer
        let eligible_peers = {
            let mut state = self.state.write().await;
            state.eligible_peers(
                peers,
                self.config.max_ops_per_peer,
                self.config.max_global_fanout_per_interval,
                self.config.backoff_fanout_threshold_per_interval,
            )
        };

        if !eligible_peers.is_empty() {
            self.push_op_to_peers(op, eligible_peers).await?;
        } else {
            // All peers at rate limit - queue for later
            let mut state = self.state.write().await;
            state.pending_announcements.insert(cid, BTreeSet::new());
        }

        Ok(())
    }

    /// Lazy pull: Respond to peer request for specific operation
    async fn lazy_pull_response(
        &self,
        _peer_id: DeviceId,
        cid: Hash32,
    ) -> Result<AttestedOp, SyncError> {
        if !self.config.lazy_pull_enabled {
            return Err(SyncError::OperationNotFound);
        }

        let state = self.state.read().await;
        state
            .oplog
            .get(&cid)
            .cloned()
            .ok_or(SyncError::OperationNotFound)
    }

    /// Add operation to local store
    pub async fn add_op(&self, op: AttestedOp) {
        let mut state = self.state.write().await;
        state.insert_op_bounded(op, self.config.max_oplog_entries);
    }

    /// Add peer to known peer set
    pub async fn add_peer(&self, peer_id: DeviceId) {
        let mut state = self.state.write().await;
        state.peers.insert(peer_id);
    }

    /// Reset rate limits (should be called periodically)
    pub async fn reset_rate_limits(&self) {
        let mut state = self.state.write().await;
        state.rate_limits.clear();
        state.global_fanout_used = 0;
        state.fanout_backoff_active = false;
    }

    /// Get pending announcements count (for monitoring)
    pub async fn pending_count(&self) -> usize {
        let state = self.state.read().await;
        state.pending_announcements.len()
    }

    /// Check if back pressure is active
    pub async fn has_back_pressure(&self) -> bool {
        let state = self.state.read().await;
        state.has_back_pressure(self.config.max_pending_announcements)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl SyncEffects for BroadcasterHandler {
    async fn sync_with_peer(&self, _peer_id: DeviceId) -> Result<SyncMetrics, SyncError> {
        // Broadcaster doesn't implement full sync - delegate to AntiEntropyHandler
        Err(SyncError::OperationNotFound)
    }

    async fn get_oplog_digest(&self) -> Result<BloomDigest, SyncError> {
        let state = self.state.read().await;
        let cids: BTreeSet<Hash32> = state.oplog.keys().copied().collect();
        Ok(BloomDigest { cids })
    }

    async fn get_missing_ops(
        &self,
        remote_digest: &BloomDigest,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        let state = self.state.read().await;
        let mut result = Vec::new();

        for (cid, op) in &state.oplog {
            if !remote_digest.cids.contains(cid) {
                result.push(op.clone());
            }
        }

        Ok(result)
    }

    async fn request_ops_from_peer(
        &self,
        _peer_id: DeviceId,
        cids: Vec<Hash32>,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        // Return any ops we have that match the requested CIDs
        let state = self.state.read().await;
        let mut result = Vec::new();

        for cid in cids {
            if let Some(op) = state.oplog.get(&cid) {
                result.push(op.clone());
            }
        }

        Ok(result)
    }

    async fn merge_remote_ops(&self, ops: Vec<AttestedOp>) -> Result<(), SyncError> {
        let mut state = self.state.write().await;
        let mut known_parent_commitments = state
            .oplog
            .values()
            .map(|op| Hash32::from(op.op.parent_commitment))
            .collect::<BTreeSet<_>>();

        for op in ops {
            validate_remote_attested_op(&op, &known_parent_commitments, None)?;
            known_parent_commitments.insert(Hash32::from(op.op.parent_commitment));
            state.merge_op_bounded(op, self.config.max_oplog_entries);
        }

        Ok(())
    }

    async fn announce_new_op(&self, cid: Hash32) -> Result<(), SyncError> {
        // Check for back pressure
        let op = {
            let state = self.state.read().await;
            if state.has_back_pressure(self.config.max_pending_announcements) {
                return Err(SyncError::BackPressure);
            }
            state
                .oplog
                .get(&cid)
                .cloned()
                .ok_or(SyncError::OperationNotFound)?
        };

        // Eager push to neighbors
        self.eager_push_to_neighbors(op).await
    }

    async fn request_op(&self, peer_id: DeviceId, cid: Hash32) -> Result<AttestedOp, SyncError> {
        self.lazy_pull_response(peer_id, cid).await
    }

    async fn push_op_to_peers(
        &self,
        op: AttestedOp,
        peers: Vec<DeviceId>,
    ) -> Result<(), SyncError> {
        let cid = Hash32::from(op.op.parent_commitment);

        // Check if we have network effects configured
        let Some(network) = &self.network else {
            // No network configured - log warning and skip actual send
            tracing::warn!(
                cid = ?cid,
                peer_count = peers.len(),
                "push_op_to_peers called without network effects - message not sent"
            );
            let mut state = self.state.write().await;
            state.pending_announcements.remove(&cid);
            return Ok(());
        };

        // Serialize the operation for transport using the wire module
        let op_data =
            crate::wire::serialize_message(&crate::wire::SyncWireMessage::op(op.clone()))?;

        // Send to each peer
        let mut send_errors = Vec::new();
        for peer in &peers {
            tracing::debug!(cid = ?cid, peer = ?peer, "Pushing operation to peer");
            if let Err(e) = network.send_to_peer(peer.0, op_data.clone()).await {
                tracing::warn!(
                    cid = ?cid,
                    peer = ?peer,
                    error = %e,
                    "Failed to send operation to peer"
                );
                send_errors.push((*peer, e));
            }
        }

        // Remove from pending announcements
        let mut state = self.state.write().await;
        state.pending_announcements.remove(&cid);

        // If all sends failed, return an error
        if !send_errors.is_empty() && send_errors.len() == peers.len() {
            return Err(SyncError::NetworkError {
                operation: "broadcast_op",
                detail: format!(
                    "failed to send operation to any peer: {} errors",
                    send_errors.len()
                ),
            });
        }

        Ok(())
    }

    async fn get_connected_peers(&self) -> Result<Vec<DeviceId>, SyncError> {
        let state = self.state.read().await;
        Ok(state.peers.iter().copied().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{create_test_op, test_context, test_device};

    #[tokio::test]
    async fn test_eager_push_enabled() {
        let config = BroadcastConfig {
            eager_push_enabled: true,
            ..Default::default()
        };
        let context_id = test_context(1);
        let handler = BroadcasterHandler::new(config, context_id);

        let peer1 = test_device(1);
        handler.add_peer(peer1).await;

        let op = create_test_op(aura_core::Hash32([1u8; 32]));
        handler.add_op(op.clone()).await;

        let result = handler.eager_push_to_neighbors(op).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_eager_push_disabled() {
        let config = BroadcastConfig {
            eager_push_enabled: false,
            ..Default::default()
        };
        let context_id = test_context(2);
        let handler = BroadcasterHandler::new(config, context_id);

        let op = create_test_op(aura_core::Hash32([1u8; 32]));
        let result = handler.eager_push_to_neighbors(op).await;
        assert!(result.is_ok()); // Returns Ok but does nothing
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let config = BroadcastConfig {
            max_ops_per_peer: 2,
            eager_push_enabled: true,
            ..Default::default()
        };
        let context_id = test_context(3);
        let handler = BroadcasterHandler::new(config, context_id);

        let peer1 = test_device(1);
        handler.add_peer(peer1).await;

        // Push 3 ops - third should be queued due to rate limit
        for i in 0..3 {
            let op = create_test_op(aura_core::Hash32([i as u8; 32]));
            handler.add_op(op.clone()).await;
            let _ = handler.eager_push_to_neighbors(op).await;
        }

        // After 2 ops, rate limit should be hit
        let pending = handler.pending_count().await;
        assert!(pending > 0);
    }

    #[tokio::test]
    async fn test_back_pressure() {
        let config = BroadcastConfig {
            max_pending_announcements: 5,
            eager_push_enabled: true,
            ..Default::default()
        };
        let context_id = test_context(4);
        let handler = BroadcasterHandler::new(config, context_id);

        // Fill pending queue to capacity
        for i in 0..6 {
            let cid = aura_core::Hash32([i as u8; 32]);
            let mut state = handler.state.write().await;
            state.pending_announcements.insert(cid, BTreeSet::new());
        }

        let op = create_test_op(aura_core::Hash32([99u8; 32]));
        let result = handler.eager_push_to_neighbors(op).await;

        assert!(matches!(result, Err(SyncError::BackPressure)));
        assert!(handler.has_back_pressure().await);
    }

    #[tokio::test]
    async fn test_lazy_pull_enabled() {
        let config = BroadcastConfig {
            lazy_pull_enabled: true,
            ..Default::default()
        };
        let context_id = test_context(5);
        let handler = BroadcasterHandler::new(config, context_id);

        let op = create_test_op(aura_core::Hash32([1u8; 32]));
        handler.add_op(op.clone()).await;

        let peer_id = test_device(1);
        let retrieved = handler
            .lazy_pull_response(peer_id, aura_core::Hash32([1u8; 32]))
            .await;

        assert!(retrieved.is_ok());
        assert_eq!(
            retrieved.unwrap().op.parent_commitment,
            aura_core::Hash32([1u8; 32]).0
        );
    }

    #[tokio::test]
    async fn test_lazy_pull_disabled() {
        let config = BroadcastConfig {
            lazy_pull_enabled: false,
            ..Default::default()
        };
        let context_id = test_context(6);
        let handler = BroadcasterHandler::new(config, context_id);

        let op = create_test_op(aura_core::Hash32([1u8; 32]));
        handler.add_op(op).await;

        let peer_id = test_device(1);
        let result = handler
            .lazy_pull_response(peer_id, aura_core::Hash32([1u8; 32]))
            .await;

        assert!(matches!(result, Err(SyncError::OperationNotFound)));
    }

    #[tokio::test]
    async fn test_announce_new_op() {
        let context_id = test_context(7);
        let handler = BroadcasterHandler::new(BroadcastConfig::default(), context_id);

        let peer1 = test_device(1);
        handler.add_peer(peer1).await;

        let op = create_test_op(aura_core::Hash32([1u8; 32]));
        handler.add_op(op.clone()).await;

        let result = handler.announce_new_op(aura_core::Hash32([1u8; 32])).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_merge_deduplication() {
        let context_id = test_context(8);
        let handler = BroadcasterHandler::new(BroadcastConfig::default(), context_id);

        let op1 = create_test_op(aura_core::Hash32::default());
        let op1_dup = create_test_op(aura_core::Hash32::default());

        handler.merge_remote_ops(vec![op1, op1_dup]).await.unwrap();

        let state = handler.state.read().await;
        assert_eq!(state.oplog.len(), 1); // Duplicate parent commitment is deduplicated
    }

    #[tokio::test]
    async fn test_reset_rate_limits() {
        let config = BroadcastConfig {
            max_ops_per_peer: 1,
            ..Default::default()
        };
        let context_id = test_context(9);
        let handler = BroadcasterHandler::new(config, context_id);

        let peer1 = test_device(1);
        handler.add_peer(peer1).await;

        // Hit rate limit
        let op = create_test_op(aura_core::Hash32([1u8; 32]));
        handler.add_op(op.clone()).await;
        let _ = handler.eager_push_to_neighbors(op.clone()).await;

        // Reset and verify we can push again
        handler.reset_rate_limits().await;
        let result = handler.eager_push_to_neighbors(op).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_global_fanout_limit_caps_eligible_peers() {
        let config = BroadcastConfig {
            max_ops_per_peer: 100,
            max_global_fanout_per_interval: 2,
            ..Default::default()
        };
        let context_id = test_context(10);
        let handler = BroadcasterHandler::new(config, context_id);

        handler.add_peer(test_device(1)).await;
        handler.add_peer(test_device(2)).await;
        handler.add_peer(test_device(3)).await;

        let op = create_test_op(aura_core::Hash32([1u8; 32]));
        handler.eager_push_to_neighbors(op).await.unwrap();

        let state = handler.state.read().await;
        assert_eq!(state.global_fanout_used, 2);
        assert_eq!(state.rate_limits.len(), 2);
    }

    #[tokio::test]
    async fn test_adaptive_backoff_queues_after_threshold() {
        let config = BroadcastConfig {
            max_ops_per_peer: 100,
            max_global_fanout_per_interval: 10,
            backoff_fanout_threshold_per_interval: 1,
            ..Default::default()
        };
        let context_id = test_context(11);
        let handler = BroadcasterHandler::new(config, context_id);

        handler.add_peer(test_device(1)).await;
        handler.add_peer(test_device(2)).await;

        let first = create_test_op(aura_core::Hash32([1u8; 32]));
        handler.eager_push_to_neighbors(first).await.unwrap();
        let second = create_test_op(aura_core::Hash32([2u8; 32]));
        handler.eager_push_to_neighbors(second).await.unwrap();

        let state = handler.state.read().await;
        assert!(state.fanout_backoff_active);
        assert_eq!(state.global_fanout_used, 1);
        assert_eq!(state.pending_announcements.len(), 1);
    }
}
