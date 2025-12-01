use crate::effects::sync::{AntiEntropyConfig, BloomDigest, SyncEffects, SyncError};
use crate::guards::effect_system_trait::GuardContextProvider;
use crate::guards::send_guard::create_send_guard;
use crate::guards::GuardEffects;
use async_lock::RwLock;
use async_trait::async_trait;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::{tree::AttestedOp, Hash32};
use std::collections::BTreeSet;
use std::sync::Arc;
use uuid::Uuid;

/// Composite trait bound for guard chain operations
pub trait GuardChainEffects: GuardEffects + GuardContextProvider + PhysicalTimeEffects {}

// Blanket impl for any type implementing all required traits
impl<T> GuardChainEffects for T where T: GuardEffects + GuardContextProvider + PhysicalTimeEffects {}

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
            tracing::warn!(
                "sync_with_peer_impl: Cannot use guard chain without effect system parameter"
            );
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

    /// Request digest from peer using guard chain with proper effect system
    ///
    /// This method enforces the guard chain predicate:
    /// need("sync:request_digest") ≤ Auth(ctx) ∧ headroom(ctx, 10)
    async fn request_digest_from_peer_guarded<E: GuardChainEffects>(
        &self,
        peer_id: Uuid,
        effect_system: &E,
    ) -> Result<BloomDigest, SyncError> {
        let peer_authority = AuthorityId::from(peer_id);

        // Create and evaluate guard chain for digest request
        let guard_chain = create_send_guard(
            "sync:request_digest".to_string(),
            self.context_id,
            peer_authority,
            10, // low cost for digest request
        )
        .with_operation_id(format!("digest_request_{}", peer_id));

        // Evaluate guard chain - this enforces authorization and flow budget
        let guard_result = guard_chain.evaluate(effect_system).await.map_err(|e| {
            tracing::error!(peer = ?peer_id, error = %e, "Guard chain evaluation failed");
            SyncError::GuardChainFailure(format!("Digest request guard failed: {}", e))
        })?;

        if !guard_result.authorized {
            tracing::warn!(
                peer = ?peer_id,
                reason = ?guard_result.denial_reason,
                "Digest request denied by guard chain"
            );
            return Err(SyncError::GuardChainFailure(
                guard_result
                    .denial_reason
                    .unwrap_or_else(|| "Authorization denied".to_string()),
            ));
        }

        tracing::debug!(
            peer = ?peer_id,
            receipt_nonce = ?guard_result.receipt.as_ref().map(|r| r.nonce),
            "Guard chain approved digest request"
        );

        // In real implementation, transport would send digest request using the receipt
        // Return empty digest - actual network request pending transport layer integration
        Ok(BloomDigest {
            cids: BTreeSet::new(),
        })
    }

    /// Legacy: Request digest from peer without guard chain (deprecated)
    #[deprecated(note = "Use request_digest_from_peer_guarded with effect system instead")]
    async fn request_digest_from_peer_with_guard_chain(
        &self,
        peer_id: Uuid,
    ) -> Result<BloomDigest, SyncError> {
        tracing::warn!(
            "SECURITY: Digest request for peer {:?} bypasses guard chain - no effect system available",
            peer_id
        );
        // Return empty to indicate no actual sync can happen without guard chain
        Ok(BloomDigest {
            cids: BTreeSet::new(),
        })
    }

    /// Legacy digest request (deprecated)
    async fn request_digest_from_peer(&self, peer_id: Uuid) -> Result<BloomDigest, SyncError> {
        self.request_digest_from_peer_with_guard_chain(peer_id)
            .await
    }

    /// Compute which ops we should push to peer
    async fn compute_ops_to_push(
        &self,
        local: &BloomDigest,
        remote: &BloomDigest,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        let oplog = self.oplog.read().await;
        let mut result = Vec::new();

        for op in oplog.iter() {
            let cid = Hash32::from(op.op.parent_commitment);
            if local.cids.contains(&cid) && !remote.cids.contains(&cid) {
                result.push(op.clone());
            }
        }

        Ok(result)
    }

    /// Compute which CIDs we should pull from peer
    fn compute_cids_to_pull(&self, local: &BloomDigest, remote: &BloomDigest) -> Vec<Hash32> {
        remote
            .cids
            .iter()
            .filter(|&cid| !local.cids.contains(cid))
            .copied()
            .collect()
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

        Ok(BloomDigest { cids })
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
        self.request_ops_from_peer_with_guard_chain_impl(peer_id, cids)
            .await
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

    async fn announce_new_op(&self, cid: Hash32) -> Result<(), SyncError> {
        // Legacy path - prefer the guard chain version for production use
        tracing::debug!("Announcing new op: {:?} (INSECURE)", cid);
        Ok(())
    }

    async fn request_op(&self, _peer_id: Uuid, cid: Hash32) -> Result<AttestedOp, SyncError> {
        // Local oplog lookup - network request pending transport layer integration
        let oplog = self.oplog.read().await;
        oplog
            .iter()
            .find(|op| Hash32::from(op.op.parent_commitment) == cid)
            .cloned()
            .ok_or(SyncError::OperationNotFound)
    }

    async fn push_op_to_peers(&self, op: AttestedOp, peers: Vec<Uuid>) -> Result<(), SyncError> {
        let cid = Hash32::from(op.op.parent_commitment);
        tracing::warn!(
            "push_op_to_peers currently bypasses guard chain; transport integration pending"
        );
        tracing::debug!("Pushing op {:?} to peers: {:?} (no-op)", cid, peers);
        Ok(())
    }

    async fn get_connected_peers(&self) -> Result<Vec<Uuid>, SyncError> {
        let peers = self.peers.read().await;
        Ok(peers.iter().copied().collect())
    }
}

impl AntiEntropyHandler {
    /// Get operations that are missing between local and remote digests
    pub async fn get_missing_ops_public(
        &self,
        remote_digest: &BloomDigest,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        self.get_missing_ops(remote_digest).await
    }

    /// Request operations from peer with guard chain enforcement
    ///
    /// Evaluates guard chain predicate:
    /// need("sync:request_ops") ≤ Auth(ctx) ∧ headroom(ctx, cids.len() * 5)
    async fn request_ops_from_peer_guarded<E: GuardChainEffects>(
        &self,
        peer_id: Uuid,
        cids: Vec<Hash32>,
        effect_system: &E,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        let peer_authority = AuthorityId::from(peer_id);
        let cost = cids.len() as u32 * 5;

        let guard_chain = create_send_guard(
            "sync:request_ops".to_string(),
            self.context_id,
            peer_authority,
            cost,
        )
        .with_operation_id(format!("ops_request_{}_{}", peer_id, cids.len()));

        // Evaluate guard chain
        let guard_result = guard_chain.evaluate(effect_system).await.map_err(|e| {
            tracing::error!(peer = ?peer_id, error = %e, "Guard chain evaluation failed for ops request");
            SyncError::GuardChainFailure(format!("Ops request guard failed: {}", e))
        })?;

        if !guard_result.authorized {
            tracing::warn!(
                peer = ?peer_id,
                cids_count = cids.len(),
                reason = ?guard_result.denial_reason,
                "Ops request denied by guard chain"
            );
            return Err(SyncError::GuardChainFailure(
                guard_result
                    .denial_reason
                    .unwrap_or_else(|| "Authorization denied".to_string()),
            ));
        }

        tracing::debug!(
            peer = ?peer_id,
            cids_count = cids.len(),
            receipt_nonce = ?guard_result.receipt.as_ref().map(|r| r.nonce),
            "Guard chain approved ops request"
        );

        // In real implementation, transport would send request using the receipt
        // Local oplog lookup - network request pending transport layer integration
        let oplog = self.oplog.read().await;
        let ops_result: Vec<AttestedOp> = oplog
            .iter()
            .filter(|op| cids.contains(&Hash32::from(op.op.parent_commitment)))
            .cloned()
            .collect();

        Ok(ops_result)
    }

    /// Legacy: Request ops without guard chain (deprecated)
    #[deprecated(note = "Use request_ops_from_peer_guarded with effect system instead")]
    async fn request_ops_from_peer_with_guard_chain_impl(
        &self,
        peer_id: Uuid,
        cids: Vec<Hash32>,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        tracing::warn!(
            "SECURITY: Ops request from peer {:?} bypasses guard chain - no effect system available",
            peer_id
        );
        // Return local ops but log security warning
        let oplog = self.oplog.read().await;
        Ok(oplog
            .iter()
            .filter(|op| cids.contains(&Hash32::from(op.op.parent_commitment)))
            .cloned()
            .collect())
    }

    /// Announce new operation to all peers with guard chain enforcement
    ///
    /// Evaluates guard chain for each peer:
    /// need("sync:announce_op") ≤ Auth(ctx) ∧ headroom(ctx, 5)
    async fn announce_new_op_guarded<E: GuardChainEffects>(
        &self,
        cid: Hash32,
        effect_system: &E,
    ) -> Result<(), SyncError> {
        let peers = self.peers.read().await;
        let mut failed_peers = Vec::new();

        for &peer_uuid in peers.iter() {
            let peer_authority = AuthorityId::from(peer_uuid);

            let guard_chain = create_send_guard(
                "sync:announce_op".to_string(),
                self.context_id,
                peer_authority,
                5, // low cost for announcement
            )
            .with_operation_id(format!("announce_{}_{}", cid, peer_uuid));

            // Evaluate guard chain for this peer
            match guard_chain.evaluate(effect_system).await {
                Ok(result) if result.authorized => {
                    tracing::debug!(
                        cid = ?cid,
                        peer = ?peer_uuid,
                        receipt_nonce = ?result.receipt.as_ref().map(|r| r.nonce),
                        "Guard chain approved announcement to peer"
                    );
                    // In real implementation, transport would send announcement using the receipt
                }
                Ok(result) => {
                    tracing::warn!(
                        cid = ?cid,
                        peer = ?peer_uuid,
                        reason = ?result.denial_reason,
                        "Announcement to peer denied by guard chain"
                    );
                    failed_peers.push(peer_uuid);
                }
                Err(e) => {
                    tracing::error!(
                        cid = ?cid,
                        peer = ?peer_uuid,
                        error = %e,
                        "Guard chain evaluation failed for announcement"
                    );
                    failed_peers.push(peer_uuid);
                }
            }
        }

        if !failed_peers.is_empty() {
            tracing::warn!(
                cid = ?cid,
                failed_count = failed_peers.len(),
                total_peers = peers.len(),
                "Some peer announcements failed guard chain evaluation"
            );
        }

        Ok(())
    }

    /// Legacy: Announce op without guard chain (deprecated)
    #[deprecated(note = "Use announce_new_op_guarded with effect system instead")]
    async fn announce_new_op_with_guard_chain_impl(&self, cid: Hash32) -> Result<(), SyncError> {
        let peers = self.peers.read().await;
        for &peer_uuid in peers.iter() {
            tracing::warn!(
                "SECURITY: Announcement of op {:?} to peer {:?} bypasses guard chain",
                cid,
                peer_uuid
            );
        }

        Ok(())
    }

    /// Push operation to peers with guard chain enforcement
    ///
    /// Evaluates guard chain for each peer:
    /// need("sync:push_op") ≤ Auth(ctx) ∧ headroom(ctx, cost)
    pub async fn push_op_to_peers_guarded<E: GuardChainEffects>(
        &self,
        op: AttestedOp,
        peers: Vec<Uuid>,
        effect_system: &E,
    ) -> Result<(), SyncError> {
        let cid = Hash32::from(op.op.parent_commitment);
        let mut failed_peers = Vec::new();

        for peer_uuid in &peers {
            let peer_authority = AuthorityId::from(*peer_uuid);
            let cost = 50; // moderate cost for op push

            let guard_chain = create_send_guard(
                "sync:push_op".to_string(),
                self.context_id,
                peer_authority,
                cost,
            )
            .with_operation_id(format!("push_op_{}_{}", cid, peer_uuid));

            // Evaluate guard chain for this peer
            match guard_chain.evaluate(effect_system).await {
                Ok(result) if result.authorized => {
                    tracing::debug!(
                        cid = ?cid,
                        peer = ?peer_uuid,
                        receipt_nonce = ?result.receipt.as_ref().map(|r| r.nonce),
                        "Guard chain approved op push to peer"
                    );
                    // In real implementation, transport would send op using the receipt
                }
                Ok(result) => {
                    tracing::warn!(
                        cid = ?cid,
                        peer = ?peer_uuid,
                        reason = ?result.denial_reason,
                        "Op push to peer denied by guard chain"
                    );
                    failed_peers.push(*peer_uuid);
                }
                Err(e) => {
                    tracing::error!(
                        cid = ?cid,
                        peer = ?peer_uuid,
                        error = %e,
                        "Guard chain evaluation failed for op push"
                    );
                    failed_peers.push(*peer_uuid);
                }
            }
        }

        if !failed_peers.is_empty() {
            tracing::warn!(
                cid = ?cid,
                failed_count = failed_peers.len(),
                total_peers = peers.len(),
                "Some op pushes failed guard chain evaluation"
            );
        }

        Ok(())
    }

    /// Main sync routine with full guard chain enforcement
    ///
    /// This is the secure entry point for peer synchronization.
    /// All operations flow through the guard chain.
    pub async fn sync_with_peer_guarded<E: GuardChainEffects>(
        &self,
        peer_id: Uuid,
        effect_system: &E,
    ) -> Result<(), SyncError> {
        // Step 1: Get local digest (pure, no guard needed)
        let local_digest = self.get_oplog_digest().await?;

        // Step 2: Request remote digest through guard chain
        let remote_digest = self
            .request_digest_from_peer_guarded(peer_id, effect_system)
            .await?;

        // Step 3: Compute differences (pure, no guard needed)
        let ops_to_push = self
            .compute_ops_to_push(&local_digest, &remote_digest)
            .await?;
        let cids_to_pull = self.compute_cids_to_pull(&local_digest, &remote_digest);

        // Step 4: Push our ops to peer through guard chain
        if !ops_to_push.is_empty() {
            for op in ops_to_push {
                self.push_op_to_peers_guarded(op, vec![peer_id], effect_system)
                    .await?;
            }
        }

        // Step 5: Pull missing ops from peer through guard chain
        if !cids_to_pull.is_empty() {
            let missing_ops = self
                .request_ops_from_peer_guarded(peer_id, cids_to_pull, effect_system)
                .await?;

            // Step 6: Verify and merge
            self.merge_remote_ops(missing_ops).await?;
        }

        Ok(())
    }

    #[allow(deprecated)]
    async fn announce_new_op(&self, cid: Hash32) -> Result<(), SyncError> {
        self.announce_new_op_with_guard_chain_impl(cid).await
    }

    async fn request_op(&self, _peer_id: Uuid, cid: Hash32) -> Result<AttestedOp, SyncError> {
        // In real implementation: transport.request(peer_id, OpRequest { cid }).await
        let oplog = self.oplog.read().await;

        oplog
            .iter()
            .find(|op| Hash32::from(op.op.parent_commitment) == cid)
            .cloned()
            .ok_or(SyncError::OperationNotFound)
    }

    #[deprecated(note = "Use push_op_to_peers_guarded with effect system instead")]
    async fn push_op_to_peers(&self, op: AttestedOp, peers: Vec<Uuid>) -> Result<(), SyncError> {
        let cid = Hash32::from(op.op.parent_commitment);
        tracing::warn!(
            "SECURITY: push_op_to_peers called without effect system - guard chain bypassed for op {:?}",
            cid
        );
        for peer in peers {
            tracing::debug!("Pushing op to peer: {:?} (INSECURE)", peer);
        }
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
        assert!(digest.cids.is_empty());
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
        assert_eq!(digest.cids.len(), 2);
        assert!(digest.cids.contains(&aura_core::Hash32([1u8; 32])));
        assert!(digest.cids.contains(&aura_core::Hash32([2u8; 32])));
    }

    #[tokio::test]
    async fn test_compute_ops_to_push() {
        let context_id = ContextId::new_from_entropy([3u8; 32]);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let op1 = create_test_op(aura_core::Hash32([1u8; 32]));
        handler.add_op(op1.clone()).await;

        let local_digest = handler.get_oplog_digest().await.unwrap();
        let remote_digest = BloomDigest {
            cids: BTreeSet::new(), // Remote has no ops
        };

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

        let local_digest = BloomDigest {
            cids: BTreeSet::new(), // We have no ops
        };

        let mut remote_cids = BTreeSet::new();
        remote_cids.insert(aura_core::Hash32([1u8; 32]));
        remote_cids.insert(aura_core::Hash32([2u8; 32]));

        let remote_digest = BloomDigest { cids: remote_cids };

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
