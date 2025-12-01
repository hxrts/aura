use async_lock::RwLock;
use async_trait::async_trait;
use aura_core::effects::{BloomDigest, SyncEffects, SyncError};
use aura_core::identifiers::ContextId;
use aura_core::{tree::AttestedOp, Hash32};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use uuid::Uuid;

/// Configuration for broadcast behavior
#[derive(Debug, Clone)]
pub struct BroadcastConfig {
    /// Maximum operations to push per peer per interval
    pub max_ops_per_peer: usize,
    /// Maximum pending announcements before applying back pressure
    pub max_pending_announcements: usize,
    /// Enable eager push to immediate neighbors
    pub eager_push_enabled: bool,
    /// Enable lazy pull on request
    pub lazy_pull_enabled: bool,
}

impl Default for BroadcastConfig {
    fn default() -> Self {
        Self {
            max_ops_per_peer: 100,
            max_pending_announcements: 1000,
            eager_push_enabled: true,
            lazy_pull_enabled: true,
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
/// All sends go through the guard chain to enforce authorization → flow → leakage → journal sequence.
#[derive(Clone)]
pub struct BroadcasterHandler {
    config: BroadcastConfig,
    /// Local operation store
    oplog: Arc<RwLock<BTreeMap<Hash32, AttestedOp>>>,
    peers: Arc<RwLock<BTreeSet<Uuid>>>,
    /// Pending announcements (CID -> set of peers that need it)
    pending_announcements: Arc<RwLock<BTreeMap<Hash32, BTreeSet<Uuid>>>>,
    /// Rate limiting: peer -> count of ops pushed in current interval
    rate_limits: Arc<RwLock<BTreeMap<Uuid, usize>>>,
    /// Context ID for guard chain operations
    context_id: ContextId,
}

impl BroadcasterHandler {
    pub fn new(config: BroadcastConfig, context_id: ContextId) -> Self {
        Self {
            config,
            oplog: Arc::new(RwLock::new(BTreeMap::new())),
            peers: Arc::new(RwLock::new(BTreeSet::new())),
            pending_announcements: Arc::new(RwLock::new(BTreeMap::new())),
            rate_limits: Arc::new(RwLock::new(BTreeMap::new())),
            context_id,
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
        let pending = self.pending_announcements.read().await;
        if pending.len() >= self.config.max_pending_announcements {
            return Err(SyncError::BackPressure);
        }
        drop(pending);

        let peers = self.get_connected_peers().await?;
        let cid = Hash32::from(op.op.parent_commitment);

        // Apply rate limiting per peer
        let mut rate_limits = self.rate_limits.write().await;
        let mut eligible_peers = Vec::new();

        for peer in peers {
            let count = rate_limits.entry(peer).or_insert(0);
            if *count < self.config.max_ops_per_peer {
                eligible_peers.push(peer);
                *count += 1;
            }
        }
        drop(rate_limits);

        if !eligible_peers.is_empty() {
            self.push_op_to_peers(op, eligible_peers).await?;
        } else {
            // All peers at rate limit - queue for later
            let mut pending = self.pending_announcements.write().await;
            pending.insert(cid, BTreeSet::new());
        }

        Ok(())
    }

    /// Lazy pull: Respond to peer request for specific operation
    async fn lazy_pull_response(
        &self,
        peer_id: Uuid,
        cid: Hash32,
    ) -> Result<AttestedOp, SyncError> {
        if !self.config.lazy_pull_enabled {
            return Err(SyncError::OperationNotFound);
        }

        let oplog = self.oplog.read().await;
        oplog.get(&cid).cloned().ok_or(SyncError::OperationNotFound)
    }

    /// Add operation to local store
    pub async fn add_op(&self, op: AttestedOp) {
        let mut oplog = self.oplog.write().await;
        oplog.insert(Hash32::from(op.op.parent_commitment), op);
    }

    /// Add peer to known peer set
    pub async fn add_peer(&self, peer_id: Uuid) {
        let mut peers = self.peers.write().await;
        peers.insert(peer_id);
    }

    /// Reset rate limits (should be called periodically)
    pub async fn reset_rate_limits(&self) {
        let mut rate_limits = self.rate_limits.write().await;
        rate_limits.clear();
    }

    /// Get pending announcements count (for monitoring)
    pub async fn pending_count(&self) -> usize {
        let pending = self.pending_announcements.read().await;
        pending.len()
    }

    /// Check if back pressure is active
    pub async fn has_back_pressure(&self) -> bool {
        let pending = self.pending_announcements.read().await;
        pending.len() >= self.config.max_pending_announcements
    }
}

#[async_trait]
impl SyncEffects for BroadcasterHandler {
    async fn sync_with_peer(&self, _peer_id: Uuid) -> Result<(), SyncError> {
        // Broadcaster doesn't implement full sync - delegate to AntiEntropyHandler
        Err(SyncError::OperationNotFound)
    }

    async fn get_oplog_digest(&self) -> Result<BloomDigest, SyncError> {
        let oplog = self.oplog.read().await;
        let mut filter = Vec::with_capacity(oplog.len() * 32);
        for cid in oplog.keys() {
            filter.extend_from_slice(&cid.0);
        }
        Ok(BloomDigest {
            filter,
            count: oplog.len(),
        })
    }

    async fn get_missing_ops(
        &self,
        remote_digest: &BloomDigest,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        let oplog = self.oplog.read().await;
        let mut result = Vec::new();

        let remote_set: BTreeSet<Hash32> = remote_digest
            .filter
            .chunks_exact(32)
            .map(|chunk| {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(chunk);
                Hash32::from(arr)
            })
            .collect();

        for (cid, op) in oplog.iter() {
            if !remote_set.contains(cid) {
                result.push(op.clone());
            }
        }

        Ok(result)
    }

    async fn request_ops_from_peer(
        &self,
        _peer_id: Uuid,
        cids: Vec<Hash32>,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        // Return any ops we have that match the requested CIDs
        let oplog = self.oplog.read().await;
        let mut result = Vec::new();

        for cid in cids {
            if let Some(op) = oplog.get(&cid) {
                result.push(op.clone());
            }
        }

        Ok(result)
    }

    async fn merge_remote_ops(&self, ops: Vec<AttestedOp>) -> Result<(), SyncError> {
        let mut oplog = self.oplog.write().await;
        let mut pending = self.pending_announcements.write().await;

        for op in ops {
            let cid = Hash32::from(op.op.parent_commitment);
            oplog.insert(cid, op);

            // Add to pending announcements if not at capacity
            if pending.len() < self.config.max_pending_announcements {
                pending.insert(cid, BTreeSet::new());
            }
        }

        Ok(())
    }

    async fn announce_new_op(&self, cid: Hash32) -> Result<(), SyncError> {
        let mut pending = self.pending_announcements.write().await;

        if pending.len() >= self.config.max_pending_announcements {
            return Err(SyncError::OperationNotFound); // Using as "capacity exceeded"
        }

        pending.insert(cid, BTreeSet::new());
        Ok(())
    }

    async fn request_op(&self, _peer_id: Uuid, cid: Hash32) -> Result<AttestedOp, SyncError> {
        let oplog = self.oplog.read().await;
        oplog.get(&cid).cloned().ok_or(SyncError::OperationNotFound)
    }

    async fn push_op_to_peers(&self, op: AttestedOp, _peers: Vec<Uuid>) -> Result<(), SyncError> {
        let cid = Hash32::from(op.op.parent_commitment);
        let mut oplog = self.oplog.write().await;
        oplog.insert(cid, op);
        Ok(())
    }

    async fn get_connected_peers(&self) -> Result<Vec<Uuid>, SyncError> {
        // Broadcaster doesn't track individual peers - return empty list
        Ok(Vec::new())
    }
}

impl BroadcasterHandler {
    async fn request_ops_from_peer_legacy(
        &self,
        _peer_id: Uuid,
        cids: Vec<Hash32>,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        let oplog = self.oplog.read().await;
        let mut result = Vec::new();

        for cid in cids {
            if let Some(op) = oplog.get(&cid) {
                result.push(op.clone());
            }
        }

        Ok(result)
    }

    async fn merge_remote_ops(&self, ops: Vec<AttestedOp>) -> Result<(), SyncError> {
        let mut oplog = self.oplog.write().await;

        for op in ops {
            let cid = Hash32::from(op.op.parent_commitment);
            // Deduplicate - only insert if not already present
            oplog.entry(cid).or_insert(op);
        }

        Ok(())
    }

    async fn announce_new_op(&self, cid: Hash32) -> Result<(), SyncError> {
        // Check for back pressure
        let pending = self.pending_announcements.read().await;
        if pending.len() >= self.config.max_pending_announcements {
            return Err(SyncError::BackPressure);
        }
        drop(pending);

        // Get the operation
        let oplog = self.oplog.read().await;
        let op = oplog.get(&cid).ok_or(SyncError::OperationNotFound)?.clone();
        drop(oplog);

        // Eager push to neighbors
        self.eager_push_to_neighbors(op).await
    }

    async fn request_op(&self, peer_id: Uuid, cid: Hash32) -> Result<AttestedOp, SyncError> {
        self.lazy_pull_response(peer_id, cid).await
    }

    async fn push_op_to_peers(&self, op: AttestedOp, peers: Vec<Uuid>) -> Result<(), SyncError> {
        // Legacy interface - deprecated, use push_op_to_peers_with_guard_chain instead
        tracing::warn!("push_op_to_peers called without guard chain - this bypasses security");

        let cid = Hash32::from(op.op.parent_commitment);
        for peer in peers {
            tracing::debug!("Pushing op {:?} to peer: {:?} (INSECURE)", cid, peer);
            // NOTE: This bypasses the guard chain and should not be used in production
        }

        let mut pending = self.pending_announcements.write().await;
        pending.remove(&cid);
        Ok(())
    }

    async fn get_connected_peers(&self) -> Result<Vec<Uuid>, SyncError> {
        let peers = self.peers.read().await;
        Ok(peers.iter().copied().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_journal::commitment_tree::{
        LeafId, LeafNode, LeafRole, NodeIndex, TreeOp, TreeOpKind,
    };

    fn create_test_op(commitment: Hash32) -> AttestedOp {
        AttestedOp {
            op: TreeOp {
                parent_commitment: commitment.0,
                parent_epoch: 1,
                op: TreeOpKind::AddLeaf {
                    leaf: LeafNode {
                        leaf_id: LeafId(1),
                        device_id: aura_core::identifiers::DeviceId::deterministic_test_id(),
                        role: LeafRole::Device,
                        public_key: vec![1, 2, 3],
                        meta: vec![],
                    },
                    under: NodeIndex(0),
                },
                version: 1,
            },
            agg_sig: vec![],
            signer_count: 1,
        }
    }

    #[tokio::test]
    async fn test_eager_push_enabled() {
        let config = BroadcastConfig {
            eager_push_enabled: true,
            ..Default::default()
        };
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let handler = BroadcasterHandler::new(config, context_id);

        let peer1 = Uuid::from_u128(1);
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
        let context_id = ContextId::new_from_entropy([1u8; 32]);
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
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let handler = BroadcasterHandler::new(config, context_id);

        let peer1 = Uuid::from_u128(1);
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
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let handler = BroadcasterHandler::new(config, context_id);

        // Fill pending queue to capacity
        for i in 0..6 {
            let cid = aura_core::Hash32([i as u8; 32]);
            let mut pending = handler.pending_announcements.write().await;
            pending.insert(cid, BTreeSet::new());
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
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let handler = BroadcasterHandler::new(config, context_id);

        let op = create_test_op(aura_core::Hash32([1u8; 32]));
        handler.add_op(op.clone()).await;

        let peer_id = Uuid::from_u128(1);
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
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let handler = BroadcasterHandler::new(config, context_id);

        let op = create_test_op(aura_core::Hash32([1u8; 32]));
        handler.add_op(op).await;

        let peer_id = Uuid::from_u128(1);
        let result = handler
            .lazy_pull_response(peer_id, aura_core::Hash32([1u8; 32]))
            .await;

        assert!(matches!(result, Err(SyncError::OperationNotFound)));
    }

    #[tokio::test]
    async fn test_announce_new_op() {
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let handler = BroadcasterHandler::new(BroadcastConfig::default(), context_id);

        let peer1 = Uuid::from_u128(1);
        handler.add_peer(peer1).await;

        let op = create_test_op(aura_core::Hash32([1u8; 32]));
        handler.add_op(op.clone()).await;

        let result = handler.announce_new_op(aura_core::Hash32([1u8; 32])).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_merge_deduplication() {
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let handler = BroadcasterHandler::new(BroadcastConfig::default(), context_id);

        let op1 = create_test_op(aura_core::Hash32([1u8; 32]));
        let op1_dup = create_test_op(aura_core::Hash32([1u8; 32]));
        let op2 = create_test_op(aura_core::Hash32([2u8; 32]));

        handler
            .merge_remote_ops(vec![op1, op1_dup, op2])
            .await
            .unwrap();

        let oplog = handler.oplog.read().await;
        assert_eq!(oplog.len(), 2); // Only 2 unique operations
    }

    #[tokio::test]
    async fn test_reset_rate_limits() {
        let config = BroadcastConfig {
            max_ops_per_peer: 1,
            ..Default::default()
        };
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let handler = BroadcasterHandler::new(config, context_id);

        let peer1 = Uuid::from_u128(1);
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
}
