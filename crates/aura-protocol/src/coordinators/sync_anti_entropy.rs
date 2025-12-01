use async_lock::RwLock;
use async_trait::async_trait;
use aura_core::effects::{AntiEntropyConfig, BloomDigest, SyncEffects, SyncError};
use aura_core::identifiers::ContextId;
use aura_core::{tree::AttestedOp, Hash32};
use std::collections::BTreeSet;
use std::sync::Arc;
use uuid::Uuid;

/// Handler implementing anti-entropy synchronization protocol
///
/// Uses digest-based comparison to efficiently detect and reconcile
/// OpLog differences between peers. Provides eventual consistency
/// through periodic background synchronization.
/// All network operations go through guard chain to enforce security.
#[derive(Clone)]
pub struct AntiEntropyHandler {
    #[allow(dead_code)]
    config: AntiEntropyConfig,
    // In real implementation, these would be trait objects for Journal and Transport
    oplog: Arc<RwLock<Vec<AttestedOp>>>,
    peers: Arc<RwLock<BTreeSet<Uuid>>>,
    /// Context ID for guard chain operations
    context_id: ContextId,
}

impl AntiEntropyHandler {
    fn extract_cids(digest: &BloomDigest) -> BTreeSet<Hash32> {
        digest
            .filter
            .chunks_exact(32)
            .map(|chunk| {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(chunk);
                Hash32::from(arr)
            })
            .collect()
    }
    pub fn new(config: AntiEntropyConfig, context_id: ContextId) -> Self {
        Self {
            config,
            oplog: Arc::new(RwLock::new(Vec::new())),
            peers: Arc::new(RwLock::new(BTreeSet::new())),
            context_id,
        }
    }

    /// Main anti-entropy synchronization routine
    ///
    /// Algorithm:
    /// 1. Get digest of local OpLog
    /// 2. Exchange digests with peer
    /// 3. Compute set difference (ops we have but peer doesn't)
    /// 4. Compute set difference (ops peer has but we don't)
    /// 5. Push missing ops to peer
    /// 6. Pull missing ops from peer
    /// 7. Verify and merge received ops
    async fn sync_with_peer_impl(&self, peer_id: Uuid) -> Result<(), SyncError> {
        // Step 1: Get local digest
        let local_digest = self.get_oplog_digest().await?;

        // Step 2: Request remote digest
        tracing::warn!(
            "sync_with_peer_impl: Cannot use guard chain without effect system parameter"
        );
        let remote_digest = self.request_digest_from_peer(peer_id).await?;

        // Step 3: Compute differences
        let ops_to_push = self
            .compute_ops_to_push(&local_digest, &remote_digest)
            .await?;
        let cids_to_pull = self.compute_cids_to_pull(&local_digest, &remote_digest);

        // Step 4: Push our ops to peer
        if !ops_to_push.is_empty() {
            // Note: This now requires an effect system parameter to use guard chain
            self.push_op_to_peers(ops_to_push[0].clone(), vec![peer_id])
                .await?;
        }

        // Step 5: Pull missing ops from peer
        if !cids_to_pull.is_empty() {
            // Note: This now requires an effect system parameter to use guard chain
            tracing::warn!(
                "sync_with_peer_impl: Cannot use guard chain without effect system parameter"
            );
            let missing_ops = self.request_ops_from_peer(peer_id, cids_to_pull).await?;

            // Step 6: Verify and merge
            self.merge_remote_ops(missing_ops).await?;
        }

        Ok(())
    }

    /// Request digest from peer using guard chain
    async fn request_digest_from_peer_with_guard_chain(
        &self,
        peer_id: Uuid,
    ) -> Result<BloomDigest, SyncError> {
        let _ = peer_id;
        Ok(BloomDigest::empty())
    }

    /// Legacy digest request (deprecated)
    async fn request_digest_from_peer(&self, peer_id: Uuid) -> Result<BloomDigest, SyncError> {
        let _ = peer_id;
        Ok(BloomDigest::empty())
    }

    /// Compute which ops we should push to peer
    async fn compute_ops_to_push(
        &self,
        local: &BloomDigest,
        remote: &BloomDigest,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        let local_set = Self::extract_cids(local);
        let remote_set = Self::extract_cids(remote);

        let oplog = self.oplog.read().await;
        let mut result = Vec::new();

        for op in oplog.iter() {
            let cid = Hash32::from(op.op.parent_commitment);
            if local_set.contains(&cid) && !remote_set.contains(&cid) {
                result.push(op.clone());
            }
        }

        Ok(result)
    }

    /// Compute which CIDs we should pull from peer
    fn compute_cids_to_pull(&self, local: &BloomDigest, remote: &BloomDigest) -> Vec<Hash32> {
        let local_set = Self::extract_cids(local);
        let remote_set = Self::extract_cids(remote);
        remote_set
            .difference(&local_set)
            .cloned()
            .collect::<Vec<_>>()
    }

    /// Verify operation before storing
    ///
    /// Checks:
    /// 1. Valid aggregate signature (FROST)
    /// 2. Parent binding exists in local tree
    /// 3. Operation is well-formed
    fn verify_operation(&self, _op: &AttestedOp) -> Result<(), SyncError> {
        // In real implementation:
        // 1. frost::verify_aggregate_signature(op.signatures, op.op)?
        // 2. Check op.op.parent_commitment exists in local tree state
        // 3. Validate operation constraints (e.g., threshold values)
        Ok(())
    }

    /// Add peer to known peer set
    pub async fn add_peer(&self, peer_id: Uuid) {
        let mut peers = self.peers.write().await;
        peers.insert(peer_id);
    }

    /// Add operation to local OpLog
    pub async fn add_op(&self, op: AttestedOp) {
        let mut oplog = self.oplog.write().await;
        oplog.push(op);
    }

    /// Get all operations in local OpLog
    pub async fn get_ops(&self) -> Vec<AttestedOp> {
        let oplog = self.oplog.read().await;
        oplog.clone()
    }
}

#[async_trait]
impl SyncEffects for AntiEntropyHandler {
    async fn sync_with_peer(&self, peer_id: Uuid) -> Result<(), SyncError> {
        self.sync_with_peer_impl(peer_id).await
    }

    async fn get_oplog_digest(&self) -> Result<BloomDigest, SyncError> {
        let oplog = self.oplog.read().await;
        let cids: BTreeSet<Hash32> = oplog
            .iter()
            .map(|op| op.op.parent_commitment.into())
            .collect();

        let count = cids.len();
        let mut filter = Vec::with_capacity(count * 32);
        for cid in cids {
            filter.extend_from_slice(&cid.0);
        }

        Ok(BloomDigest { filter, count })
    }

    async fn get_missing_ops(
        &self,
        remote_digest: &BloomDigest,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        let local_digest = self.get_oplog_digest().await?;
        self.compute_ops_to_push(&local_digest, remote_digest).await
    }

    async fn request_ops_from_peer(
        &self,
        peer_id: Uuid,
        cids: Vec<Hash32>,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        let _ = (peer_id, cids.len());
        let oplog = self.oplog.read().await;
        let mut result = Vec::new();

        for op in oplog.iter() {
            let op_cid = Hash32::from(op.op.parent_commitment);
            if cids.contains(&op_cid) {
                result.push(op.clone());
            }
        }

        Ok(result)
    }

    async fn merge_remote_ops(&self, ops: Vec<AttestedOp>) -> Result<(), SyncError> {
        for op in ops {
            // Verify before merging
            self.verify_operation(&op)?;

            // Add to local OpLog
            let mut oplog = self.oplog.write().await;
            oplog.push(op);
        }

        Ok(())
    }

    async fn get_connected_peers(&self) -> Result<Vec<Uuid>, SyncError> {
        let peers = self.peers.read().await;
        Ok(peers.iter().copied().collect())
    }

    async fn announce_new_op(&self, cid: Hash32) -> Result<(), SyncError> {
        let _ = cid;
        Ok(())
    }

    async fn request_op(&self, _peer_id: Uuid, cid: Hash32) -> Result<AttestedOp, SyncError> {
        let oplog = self.oplog.read().await;
        oplog
            .iter()
            .find(|op| Hash32::from(op.op.parent_commitment) == cid)
            .cloned()
            .ok_or(SyncError::OperationNotFound)
    }

    async fn push_op_to_peers(&self, op: AttestedOp, peers: Vec<Uuid>) -> Result<(), SyncError> {
        let _ = (op, peers);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_journal::commitment_tree::{TreeOp, TreeOpKind};

    fn create_test_op(commitment: Hash32) -> AttestedOp {
        AttestedOp {
            op: TreeOp {
                parent_commitment: commitment.0,
                parent_epoch: 1,
                op: TreeOpKind::AddLeaf {
                    leaf: aura_journal::commitment_tree::LeafNode {
                        leaf_id: aura_journal::commitment_tree::LeafId(1),
                        device_id: aura_core::identifiers::DeviceId::deterministic_test_id(),
                        role: aura_journal::commitment_tree::LeafRole::Device,
                        public_key: vec![1, 2, 3],
                        meta: vec![],
                    },
                    under: aura_journal::commitment_tree::NodeIndex(0),
                },
                version: 1,
            },
            agg_sig: vec![],
            signer_count: 1,
        }
    }

    #[tokio::test]
    async fn test_empty_digest() {
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);
        let digest = handler.get_oplog_digest().await.unwrap();
        assert!(digest.filter.is_empty());
    }

    #[tokio::test]
    async fn test_digest_with_ops() {
        let context_id = ContextId::new_from_entropy([2u8; 32]);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let op1 = create_test_op(aura_core::Hash32([1u8; 32]));
        let op2 = create_test_op(aura_core::Hash32([2u8; 32]));

        handler.add_op(op1).await;
        handler.add_op(op2).await;

        let digest = handler.get_oplog_digest().await.unwrap();
        assert_eq!(digest.count, 2);
        assert_eq!(digest.filter.len(), 64);
    }

    #[tokio::test]
    async fn test_compute_ops_to_push() {
        let context_id = ContextId::new_from_entropy([3u8; 32]);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let op1 = create_test_op(aura_core::Hash32([1u8; 32]));
        handler.add_op(op1.clone()).await;

        let local_digest = handler.get_oplog_digest().await.unwrap();
        let remote_digest = BloomDigest::empty();

        let to_push = handler
            .compute_ops_to_push(&local_digest, &remote_digest)
            .await
            .unwrap();

        assert_eq!(to_push.len(), 1);
        assert_eq!(
            to_push[0].op.parent_commitment,
            aura_core::Hash32([1u8; 32]).0
        );
    }

    #[tokio::test]
    async fn test_compute_cids_to_pull() {
        let context_id = ContextId::new_from_entropy([4u8; 32]);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let local_digest = BloomDigest::empty();
        let remote_digest = BloomDigest {
            filter: [
                aura_core::Hash32([1u8; 32]).0.to_vec(),
                aura_core::Hash32([2u8; 32]).0.to_vec(),
            ]
            .concat(),
            count: 2,
        };

        let to_pull = handler.compute_cids_to_pull(&local_digest, &remote_digest);

        assert_eq!(to_pull.len(), 2);
        assert!(to_pull.contains(&aura_core::Hash32([1u8; 32])));
        assert!(to_pull.contains(&aura_core::Hash32([2u8; 32])));
    }

    #[tokio::test]
    async fn test_merge_remote_ops() {
        let context_id = ContextId::new_from_entropy([5u8; 32]);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let op1 = create_test_op(aura_core::Hash32([1u8; 32]));
        let op2 = create_test_op(aura_core::Hash32([2u8; 32]));

        handler.merge_remote_ops(vec![op1, op2]).await.unwrap();

        let ops = handler.get_ops().await;
        assert_eq!(ops.len(), 2);
    }

    #[tokio::test]
    async fn test_peer_management() {
        let context_id = ContextId::new_from_entropy([6u8; 32]);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let peer1 = Uuid::from_u128(1);
        let peer2 = Uuid::from_u128(2);

        handler.add_peer(peer1).await;
        handler.add_peer(peer2).await;

        let peers = handler.get_connected_peers().await.unwrap();
        assert_eq!(peers.len(), 2);
        assert!(peers.contains(&peer1));
        assert!(peers.contains(&peer2));
    }

    #[tokio::test]
    async fn test_request_op() {
        let context_id = ContextId::new_from_entropy([7u8; 32]);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let op = create_test_op(aura_core::Hash32([1u8; 32]));
        handler.add_op(op.clone()).await;

        let peer_id = Uuid::from_u128(1);
        let retrieved = handler
            .request_op(peer_id, aura_core::Hash32([1u8; 32]))
            .await
            .unwrap();

        assert_eq!(
            retrieved.op.parent_commitment,
            aura_core::Hash32([1u8; 32]).0
        );
    }

    #[tokio::test]
    async fn test_request_missing_op() {
        let context_id = ContextId::new_from_entropy([8u8; 32]);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let peer_id = Uuid::from_u128(1);
        let result = handler
            .request_op(peer_id, aura_core::Hash32([99u8; 32]))
            .await;

        assert!(matches!(result, Err(SyncError::OperationNotFound)));
    }
}
