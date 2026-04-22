//! Anti-entropy synchronization protocol for eventual consistency.
//!
//! Provides digest-based comparison and reconciliation of OpLog state between
//! peers, with guard chain enforcement for authorization and flow budgets.

use super::config::AntiEntropyRuntimeConfig;
use super::effects::{validate_remote_attested_op, AntiEntropyConfig, BloomDigest, SyncError};
use super::pure;
use async_lock::RwLock;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::TransportEffects;
use aura_core::types::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_core::{tree::AttestedOp, FlowCost, Hash32};
use aura_guards::chain::create_send_guard_op;
use aura_guards::traits::GuardContextProvider;
use aura_guards::GuardEffects;
use aura_guards::{GuardOperation, GuardOperationId};
use std::collections::BTreeSet;
use std::fmt;

/// Composite trait bound for guard chain operations
pub trait GuardChainEffects: GuardEffects + GuardContextProvider + PhysicalTimeEffects {}

// Blanket impl for any type implementing all required traits
impl<T> GuardChainEffects for T where T: GuardEffects + GuardContextProvider + PhysicalTimeEffects {}

/// Composite trait bound for anti-entropy protocol operations.
pub trait AntiEntropyProtocolEffects: GuardChainEffects + TransportEffects {}

impl<T> AntiEntropyProtocolEffects for T where T: GuardChainEffects + TransportEffects {}

/// Handler implementing anti-entropy synchronization protocol
///
/// Uses digest-based comparison to efficiently detect and reconcile
/// OpLog differences between peers. Provides eventual consistency
/// through periodic background synchronization.
/// All network operations go through guard chain to enforce security.
pub struct AntiEntropyHandler {
    /// Anti-entropy configuration (sync intervals, batch sizes, etc.)
    config: AntiEntropyConfig,
    state: RwLock<AntiEntropyState>,
    /// Context ID for guard chain operations
    context_id: ContextId,
    /// Runtime cost configuration
    runtime: AntiEntropyRuntimeConfig,
}

#[derive(Default)]
struct AntiEntropyState {
    oplog: Vec<AttestedOp>,
    peers: BTreeSet<DeviceId>,
}

impl AntiEntropyHandler {
    pub fn new(config: AntiEntropyConfig, context_id: ContextId) -> Self {
        Self {
            config,
            state: RwLock::new(AntiEntropyState::default()),
            context_id,
            runtime: AntiEntropyRuntimeConfig::default(),
        }
    }

    /// Get the anti-entropy configuration
    pub fn config(&self) -> &AntiEntropyConfig {
        &self.config
    }

    /// Request digest from peer using guard chain with proper effect system
    ///
    /// This method enforces the guard chain predicate:
    /// need("sync:request_digest") ≤ Auth(ctx) ∧ headroom(ctx, 10)
    async fn request_digest_from_peer_guarded<E: AntiEntropyProtocolEffects>(
        &self,
        peer_id: DeviceId,
        effect_system: &E,
    ) -> Result<BloomDigest, SyncError> {
        self.approve_fail_closed_request(
            peer_id,
            effect_system,
            GuardOperation::SyncRequestDigest,
            GuardOperationId::SyncRequestDigest { peer: peer_id.0 },
            self.runtime.digest_cost,
            RequestGuardLog::Digest,
        )
        .await?;

        // In real implementation, transport would send digest request using the receipt
        // Return empty digest - actual network request pending transport layer integration
        Ok(BloomDigest {
            cids: BTreeSet::new(),
        })
    }

    /// Compute which ops we should push to peer
    async fn compute_ops_to_push(
        &self,
        local: &BloomDigest,
        remote: &BloomDigest,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        let state = self.state.read().await;
        pure::compute_ops_to_push(&state.oplog, local, remote)
    }

    /// Compute which CIDs we should pull from peer
    fn compute_cids_to_pull(&self, local: &BloomDigest, remote: &BloomDigest) -> Vec<Hash32> {
        pure::compute_cids_to_pull(local, remote)
            .into_iter()
            .collect()
    }

    /// Verify operation before storing
    ///
    /// Checks:
    /// 1. Valid aggregate signature (FROST)
    /// 2. Parent binding exists in local tree
    /// 3. Operation is well-formed
    fn verify_operation_with_known_parents(
        &self,
        op: &AttestedOp,
        known_parent_commitments: &BTreeSet<Hash32>,
    ) -> Result<(), SyncError> {
        validate_remote_attested_op(
            op,
            known_parent_commitments,
            self.config.remote_signing_witness.as_ref(),
        )
    }

    /// Add peer to known peer set
    pub async fn add_peer(&self, peer_id: DeviceId) {
        let mut state = self.state.write().await;
        state.peers.insert(peer_id);
    }

    /// Add operation to local OpLog
    pub async fn add_op(&self, op: AttestedOp) {
        let mut state = self.state.write().await;
        state.oplog.push(op);
    }

    /// Get all operations in local OpLog
    pub async fn get_ops(&self) -> Vec<AttestedOp> {
        let state = self.state.read().await;
        state.oplog.clone()
    }
}

impl AntiEntropyHandler {
    /// Get digest of local OpLog.
    pub async fn get_oplog_digest(&self) -> Result<BloomDigest, SyncError> {
        let state = self.state.read().await;
        let cids: BTreeSet<Hash32> = state
            .oplog
            .iter()
            .map(|op| op.op.parent_commitment.into())
            .collect();

        Ok(BloomDigest { cids })
    }

    /// Get operations that are missing between local and remote digests.
    pub async fn get_missing_ops(
        &self,
        remote_digest: &BloomDigest,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        let local_digest = self.get_oplog_digest().await?;
        self.compute_ops_to_push(&local_digest, remote_digest).await
    }

    /// Merge remote operations into the local OpLog.
    pub async fn merge_remote_ops(&self, ops: Vec<AttestedOp>) -> Result<(), SyncError> {
        let mut state = self.state.write().await;
        let mut known_parent_commitments: BTreeSet<Hash32> = state
            .oplog
            .iter()
            .map(|op| Hash32::from(op.op.parent_commitment))
            .collect();
        for op in ops {
            // Verify before merging
            self.verify_operation_with_known_parents(&op, &known_parent_commitments)?;

            // Add to local OpLog
            known_parent_commitments.insert(Hash32::from(op.op.parent_commitment));
            state.oplog.push(op);
        }

        Ok(())
    }

    /// Request a specific operation by CID.
    pub async fn request_op(
        &self,
        _peer_id: DeviceId,
        cid: Hash32,
    ) -> Result<AttestedOp, SyncError> {
        // Local oplog lookup - no network request needed
        let state = self.state.read().await;
        state
            .oplog
            .iter()
            .find(|op| Hash32::from(op.op.parent_commitment) == cid)
            .cloned()
            .ok_or(SyncError::OperationNotFound)
    }

    /// Get list of currently connected peers.
    pub async fn get_connected_peers(&self) -> Result<Vec<DeviceId>, SyncError> {
        let state = self.state.read().await;
        Ok(state.peers.iter().copied().collect())
    }

    /// Request operations from peer with guard chain enforcement
    ///
    /// Evaluates guard chain predicate:
    /// need("sync:request_ops") ≤ Auth(ctx) ∧ headroom(ctx, cids.len() * 5)
    async fn request_ops_from_peer_guarded<E: AntiEntropyProtocolEffects>(
        &self,
        peer_id: DeviceId,
        cids: Vec<Hash32>,
        effect_system: &E,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        self.approve_fail_closed_request(
            peer_id,
            effect_system,
            GuardOperation::SyncRequestOps,
            GuardOperationId::SyncRequestOps {
                peer: peer_id.0,
                count: cids.len() as u32,
            },
            FlowCost::new(cids.len() as u32 * self.runtime.request_cost_per_cid.value()),
            RequestGuardLog::Ops {
                cids_count: cids.len(),
            },
        )
        .await?;

        // In real implementation, transport would send request using the receipt
        // Local oplog lookup - network request pending transport layer integration
        let state = self.state.read().await;
        let ops_result: Vec<AttestedOp> = state
            .oplog
            .iter()
            .filter(|op| cids.contains(&Hash32::from(op.op.parent_commitment)))
            .cloned()
            .collect();

        Ok(ops_result)
    }

    /// Announce new operation to all peers with guard chain enforcement
    ///
    /// Evaluates guard chain for each peer:
    /// need("sync:announce_op") ≤ Auth(ctx) ∧ headroom(ctx, 5)
    async fn announce_new_op_guarded<E: AntiEntropyProtocolEffects>(
        &self,
        cid: Hash32,
        effect_system: &E,
    ) -> Result<(), SyncError> {
        let peers: Vec<DeviceId> = {
            let state = self.state.read().await;
            state.peers.iter().copied().collect()
        };
        let mut failed_peers = Vec::new();

        for peer_id in peers.iter().copied() {
            if !self
                .approve_best_effort_peer_send(
                    peer_id,
                    effect_system,
                    GuardOperation::SyncAnnounceOp,
                    GuardOperationId::SyncAnnounceOp {
                        peer: peer_id.0,
                        cid,
                    },
                    self.runtime.announce_cost,
                    BestEffortGuardLog::Announcement { cid },
                )
                .await
            {
                failed_peers.push(peer_id);
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

    /// Push operation to peers with guard chain enforcement
    ///
    /// Evaluates guard chain for each peer:
    /// need("sync:push_op") ≤ Auth(ctx) ∧ headroom(ctx, cost)
    pub async fn push_op_to_peers_guarded<E: AntiEntropyProtocolEffects>(
        &self,
        op: AttestedOp,
        peers: Vec<DeviceId>,
        effect_system: &E,
    ) -> Result<(), SyncError> {
        let cid = Hash32::from(op.op.parent_commitment);
        let mut failed_peers = Vec::new();

        for peer_id in &peers {
            if !self
                .approve_best_effort_peer_send(
                    *peer_id,
                    effect_system,
                    GuardOperation::SyncPushOp,
                    GuardOperationId::SyncPushOp {
                        peer: peer_id.0,
                        cid,
                    },
                    self.runtime.push_cost,
                    BestEffortGuardLog::Push { cid },
                )
                .await
            {
                failed_peers.push(*peer_id);
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
    pub async fn sync_with_peer_guarded<E: AntiEntropyProtocolEffects>(
        &self,
        peer_id: DeviceId,
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

    async fn approve_fail_closed_request<E: AntiEntropyProtocolEffects>(
        &self,
        peer_id: DeviceId,
        effect_system: &E,
        operation: GuardOperation,
        operation_id: GuardOperationId,
        cost: FlowCost,
        log: RequestGuardLog,
    ) -> Result<(), SyncError> {
        let peer_authority = AuthorityId::from(peer_id.0);
        let guard_chain = create_send_guard_op(operation, self.context_id, peer_authority, cost)
            .with_operation_id(operation_id);
        let guard_result = guard_chain.evaluate(effect_system).await.map_err(|e| {
            log.log_eval_error(peer_id, &e);
            SyncError::GuardChainFailure {
                operation: log.error_operation(),
                detail: format!("guard evaluation failed: {e}"),
            }
        })?;

        if !guard_result.authorized {
            let denial_reason = guard_result
                .denial_reason
                .unwrap_or_else(|| "Authorization denied".to_string());
            log.log_denied(peer_id, &denial_reason);
            return Err(SyncError::GuardChainFailure {
                operation: log.error_operation(),
                detail: denial_reason,
            });
        }

        log.log_approved(
            peer_id,
            guard_result.receipt.as_ref().map(|receipt| receipt.nonce),
        );
        Ok(())
    }

    async fn approve_best_effort_peer_send<E: AntiEntropyProtocolEffects>(
        &self,
        peer_id: DeviceId,
        effect_system: &E,
        operation: GuardOperation,
        operation_id: GuardOperationId,
        cost: FlowCost,
        log: BestEffortGuardLog,
    ) -> bool {
        let peer_authority = AuthorityId::from(peer_id.0);
        let guard_chain = create_send_guard_op(operation, self.context_id, peer_authority, cost)
            .with_operation_id(operation_id);

        match guard_chain.evaluate(effect_system).await {
            Ok(result) if result.authorized => {
                log.log_approved(
                    peer_id,
                    result.receipt.as_ref().map(|receipt| receipt.nonce),
                );
                true
            }
            Ok(result) => {
                log.log_denied(
                    peer_id,
                    result
                        .denial_reason
                        .as_deref()
                        .unwrap_or("Authorization denied"),
                );
                false
            }
            Err(error) => {
                log.log_eval_error(peer_id, &error);
                false
            }
        }
    }
}

#[derive(Clone, Copy)]
enum RequestGuardLog {
    Digest,
    Ops { cids_count: usize },
}

impl RequestGuardLog {
    fn error_operation(self) -> &'static str {
        match self {
            Self::Digest => "digest_request",
            Self::Ops { .. } => "ops_request",
        }
    }

    fn log_eval_error(self, peer_id: DeviceId, error: &impl fmt::Display) {
        match self {
            Self::Digest => {
                tracing::error!(peer = ?peer_id, error = %error, "Guard chain evaluation failed");
            }
            Self::Ops { cids_count } => {
                tracing::error!(
                    peer = ?peer_id,
                    cids_count,
                    error = %error,
                    "Guard chain evaluation failed for ops request"
                );
            }
        }
    }

    fn log_denied(self, peer_id: DeviceId, denial_reason: &str) {
        match self {
            Self::Digest => {
                tracing::warn!(
                    peer = ?peer_id,
                    reason = denial_reason,
                    "Digest request denied by guard chain"
                );
            }
            Self::Ops { cids_count } => {
                tracing::warn!(
                    peer = ?peer_id,
                    cids_count,
                    reason = denial_reason,
                    "Ops request denied by guard chain"
                );
            }
        }
    }

    fn log_approved(self, peer_id: DeviceId, receipt_nonce: Option<aura_core::FlowNonce>) {
        match self {
            Self::Digest => {
                tracing::debug!(
                    peer = ?peer_id,
                    receipt_nonce = ?receipt_nonce,
                    "Guard chain approved digest request"
                );
            }
            Self::Ops { cids_count } => {
                tracing::debug!(
                    peer = ?peer_id,
                    cids_count,
                    receipt_nonce = ?receipt_nonce,
                    "Guard chain approved ops request"
                );
            }
        }
    }
}

#[derive(Clone, Copy)]
enum BestEffortGuardLog {
    Announcement { cid: Hash32 },
    Push { cid: Hash32 },
}

impl BestEffortGuardLog {
    fn log_approved(self, peer_id: DeviceId, receipt_nonce: Option<aura_core::FlowNonce>) {
        match self {
            Self::Announcement { cid } => {
                tracing::debug!(
                    cid = ?cid,
                    peer = ?peer_id,
                    receipt_nonce = ?receipt_nonce,
                    "Guard chain approved announcement to peer"
                );
            }
            Self::Push { cid } => {
                tracing::debug!(
                    cid = ?cid,
                    peer = ?peer_id,
                    receipt_nonce = ?receipt_nonce,
                    "Guard chain approved op push to peer"
                );
            }
        }
    }

    fn log_denied(self, peer_id: DeviceId, denial_reason: &str) {
        match self {
            Self::Announcement { cid } => {
                tracing::warn!(
                    cid = ?cid,
                    peer = ?peer_id,
                    reason = denial_reason,
                    "Announcement to peer denied by guard chain"
                );
            }
            Self::Push { cid } => {
                tracing::warn!(
                    cid = ?cid,
                    peer = ?peer_id,
                    reason = denial_reason,
                    "Op push to peer denied by guard chain"
                );
            }
        }
    }

    fn log_eval_error(self, peer_id: DeviceId, error: &impl fmt::Display) {
        match self {
            Self::Announcement { cid } => {
                tracing::error!(
                    cid = ?cid,
                    peer = ?peer_id,
                    error = %error,
                    "Guard chain evaluation failed for announcement"
                );
            }
            Self::Push { cid } => {
                tracing::error!(
                    cid = ?cid,
                    peer = ?peer_id,
                    error = %error,
                    "Guard chain evaluation failed for op push"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{create_test_op, digest_from_hashes, test_context, test_device};

    #[tokio::test]
    async fn test_empty_digest() {
        let context_id = test_context(1);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);
        let digest = handler.get_oplog_digest().await.unwrap();
        assert!(digest.cids.is_empty());
    }

    #[tokio::test]
    async fn test_digest_with_ops() {
        let context_id = test_context(2);
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
        let context_id = test_context(3);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let op1 = create_test_op(aura_core::Hash32([1u8; 32]));
        handler.add_op(op1.clone()).await;

        let local_digest = handler.get_oplog_digest().await.unwrap();
        let remote_digest = BloomDigest::empty(); // Remote has no ops

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
        let context_id = test_context(4);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let local_digest = BloomDigest::empty(); // We have no ops
        let remote_digest =
            digest_from_hashes([aura_core::Hash32([1u8; 32]), aura_core::Hash32([2u8; 32])]);

        let to_pull = handler.compute_cids_to_pull(&local_digest, &remote_digest);

        assert_eq!(to_pull.len(), 2);
        assert!(to_pull.contains(&aura_core::Hash32([1u8; 32])));
        assert!(to_pull.contains(&aura_core::Hash32([2u8; 32])));
    }

    #[tokio::test]
    async fn test_merge_remote_ops() {
        let context_id = test_context(5);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let op1 = create_test_op(aura_core::Hash32::default());
        let op2 = create_test_op(aura_core::Hash32::default());

        handler.merge_remote_ops(vec![op1, op2]).await.unwrap();

        let ops = handler.get_ops().await;
        assert_eq!(ops.len(), 2);
    }

    #[tokio::test]
    async fn test_merge_remote_ops_rejects_forged_signature() {
        let context_id = test_context(55);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let mut op = create_test_op(aura_core::Hash32::default());
        op.agg_sig.clear();

        let result = handler.merge_remote_ops(vec![op]).await;
        assert!(matches!(result, Err(SyncError::VerificationFailed { .. })));
    }

    #[tokio::test]
    async fn test_merge_remote_ops_verifies_frost_signature_with_known_group_key() {
        let context_id = test_context(58);
        let config = AntiEntropyConfig {
            remote_signing_witness: Some(aura_core::tree::SigningWitness::new(
                [0xCC; 32],
                1,
                aura_core::Epoch::new(1),
            )),
            ..Default::default()
        };
        let handler = AntiEntropyHandler::new(config, context_id);

        let op = create_test_op(aura_core::Hash32::default());

        let result = handler.merge_remote_ops(vec![op]).await;
        assert!(matches!(result, Err(SyncError::VerificationFailed { .. })));
    }

    #[tokio::test]
    async fn test_merge_remote_ops_rejects_unknown_parent() {
        let context_id = test_context(56);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let op = create_test_op(aura_core::Hash32([9u8; 32]));

        let result = handler.merge_remote_ops(vec![op]).await;
        assert!(matches!(result, Err(SyncError::VerificationFailed { .. })));
    }

    #[tokio::test]
    async fn test_merge_remote_ops_rejects_zero_signer_count() {
        let context_id = test_context(57);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let mut op = create_test_op(aura_core::Hash32::default());
        op.signer_count = 0;

        let result = handler.merge_remote_ops(vec![op]).await;
        assert!(matches!(result, Err(SyncError::VerificationFailed { .. })));
    }

    #[tokio::test]
    async fn test_peer_management() {
        let context_id = test_context(6);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let peer1 = test_device(1);
        let peer2 = test_device(2);

        handler.add_peer(peer1).await;
        handler.add_peer(peer2).await;

        let peers = handler.get_connected_peers().await.unwrap();
        assert_eq!(peers.len(), 2);
        assert!(peers.contains(&peer1));
        assert!(peers.contains(&peer2));
    }

    #[tokio::test]
    async fn test_request_op() {
        let context_id = test_context(7);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let op = create_test_op(aura_core::Hash32([1u8; 32]));
        handler.add_op(op.clone()).await;

        let peer_id = test_device(1);
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
        let context_id = test_context(8);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);

        let peer_id = test_device(1);
        let result = handler
            .request_op(peer_id, aura_core::Hash32([99u8; 32]))
            .await;

        assert!(matches!(result, Err(SyncError::OperationNotFound)));
    }
}
