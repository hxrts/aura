//! Anti-entropy synchronization protocol for eventual consistency.
//!
//! Provides digest-based comparison and reconciliation of OpLog state between
//! peers, with guard chain enforcement for authorization and flow budgets.

use super::config::AntiEntropyRuntimeConfig;
use super::effects::{AntiEntropyConfig, BloomDigest, SyncError};
use super::pure;
use async_lock::RwLock;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::TransportEffects;
use aura_core::tree::verification::{check_attested_op, extract_target_node};
use aura_core::tree::{Epoch, NodeIndex, TreeHash32};
use aura_core::types::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_core::{tree::AttestedOp, FlowCost, Hash32};
use aura_guards::chain::create_send_guard_op;
use aura_guards::traits::GuardContextProvider;
use aura_guards::{
    DecodedIngress, GuardEffects, IngressSource, IngressVerificationEvidence, VerifiedIngress,
    VerifiedIngressMetadata,
};
use aura_guards::{GuardOperation, GuardOperationId};
use aura_journal::commitment_tree::{apply_structurally_verified, TreeState};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Composite trait bound for guard chain operations
pub trait GuardChainEffects: GuardEffects + GuardContextProvider + PhysicalTimeEffects {}

// Blanket impl for any type implementing all required traits
impl<T> GuardChainEffects for T where T: GuardEffects + GuardContextProvider + PhysicalTimeEffects {}

/// Composite trait bound for anti-entropy protocol operations.
pub trait AntiEntropyProtocolEffects: GuardChainEffects + TransportEffects {}

impl<T> AntiEntropyProtocolEffects for T where T: GuardChainEffects + TransportEffects {}

fn peer_sync_context(peer: DeviceId) -> ContextId {
    let entropy = peer
        .to_bytes()
        .unwrap_or_else(|_| aura_core::hash::hash(peer.to_string().as_bytes()));
    ContextId::new_from_entropy(entropy)
}

fn verified_ops_from_peer(
    peer: DeviceId,
    ops: Vec<AttestedOp>,
) -> Result<VerifiedIngress<Vec<AttestedOp>>, SyncError> {
    let payload_hash = Hash32::from_value(&ops).map_err(|error| SyncError::VerificationFailed {
        target: "anti_entropy_ingress_payload",
        detail: error.to_string(),
    })?;
    let metadata = VerifiedIngressMetadata::new(
        IngressSource::Device(peer),
        peer_sync_context(peer),
        None,
        payload_hash,
        1,
    );
    let evidence = IngressVerificationEvidence::builder(metadata)
        .peer_identity(peer.to_bytes().is_ok(), "peer device id must be encodable")
        .and_then(|builder| {
            builder.envelope_authenticity(payload_hash != Hash32::zero(), "payload hash is empty")
        })
        .and_then(|builder| {
            builder.capability_authorization(
                !ops.is_empty(),
                "anti-entropy merge must carry at least one attested op",
            )
        })
        .and_then(|builder| builder.namespace_scope(true, "peer sync context derived from peer id"))
        .and_then(|builder| builder.schema_version(true, "anti-entropy ingress schema v1"))
        .and_then(|builder| builder.replay_freshness(true, "attested op cid set is fresh input"))
        .and_then(|builder| {
            builder.signer_membership(true, "attested op signatures are checked during merge")
        })
        .and_then(|builder| {
            builder.proof_evidence(true, "attested op proof evidence is checked during merge")
        })
        .and_then(|builder| builder.build())
        .map_err(|error| SyncError::VerificationFailed {
            target: "anti_entropy_ingress_evidence",
            detail: error.to_string(),
        })?;
    DecodedIngress::new(ops, evidence.metadata().clone())
        .verify(evidence)
        .map_err(|error| SyncError::VerificationFailed {
            target: "anti_entropy_ingress_promotion",
            detail: error.to_string(),
        })
}

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
    verification_anchor: RwLock<TreeState>,
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
            verification_anchor: RwLock::new(TreeState::new()),
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

    /// Replace the verification anchor state used for remote op validation.
    pub async fn set_verification_anchor(&self, anchor: TreeState) {
        let mut state = self.verification_anchor.write().await;
        *state = anchor;
    }

    fn operation_fingerprint(op: &AttestedOp) -> Result<Hash32, SyncError> {
        Hash32::from_value(op).map_err(|error| SyncError::VerificationFailed {
            target: "anti_entropy_op_fingerprint",
            detail: error.to_string(),
        })
    }

    fn state_key(state: &TreeState) -> (Epoch, TreeHash32) {
        (state.current_epoch(), state.current_commitment())
    }

    fn parent_key(op: &AttestedOp) -> (Epoch, TreeHash32) {
        (op.op.parent_epoch, op.op.parent_commitment)
    }

    fn resolve_target_node(state: &TreeState, op: &AttestedOp) -> Result<NodeIndex, SyncError> {
        extract_target_node(&op.op.op)
            .or_else(|| match &op.op.op {
                aura_core::tree::TreeOpKind::RemoveLeaf { leaf, .. } => {
                    state.get_remove_leaf_affected_parent(leaf)
                }
                _ => None,
            })
            .ok_or(SyncError::VerificationFailed {
                target: "anti_entropy_target_node",
                detail: "unable to resolve attested-op target node".to_string(),
            })
    }

    fn verify_operation(state: &TreeState, op: &AttestedOp) -> Result<(), SyncError> {
        if op.op.parent_epoch != state.current_epoch() {
            return Err(SyncError::VerificationFailed {
                target: "anti_entropy_parent_epoch",
                detail: format!(
                    "operation references stale or future epoch {} but current epoch is {}",
                    op.op.parent_epoch,
                    state.current_epoch()
                ),
            });
        }
        if op.op.parent_commitment != state.current_commitment() {
            return Err(SyncError::VerificationFailed {
                target: "anti_entropy_parent_commitment",
                detail: format!(
                    "operation references unknown parent commitment {:?}; expected {:?}",
                    Hash32(op.op.parent_commitment),
                    Hash32(state.current_commitment())
                ),
            });
        }

        let target_node = Self::resolve_target_node(state, op)?;
        check_attested_op(state, op, target_node).map_err(|error| SyncError::VerificationFailed {
            target: "anti_entropy_attested_op",
            detail: error.to_string(),
        })
    }

    fn advance_state(parent: &TreeState, op: &AttestedOp) -> Result<TreeState, SyncError> {
        let mut next = parent.clone();
        apply_structurally_verified(&mut next, op).map_err(|error| {
            SyncError::VerificationFailed {
                target: "anti_entropy_apply_verified",
                detail: error.to_string(),
            }
        })?;
        Ok(next)
    }

    fn build_known_states(
        anchor: &TreeState,
        ops: &[AttestedOp],
    ) -> Result<BTreeMap<(Epoch, TreeHash32), TreeState>, SyncError> {
        let mut known_states = BTreeMap::from([(Self::state_key(anchor), anchor.clone())]);
        let mut pending = ops.to_vec();

        while !pending.is_empty() {
            let mut next_pending = Vec::new();
            let mut progressed = false;

            for op in pending {
                let Some(parent_state) = known_states.get(&Self::parent_key(&op)).cloned() else {
                    next_pending.push(op);
                    continue;
                };

                Self::verify_operation(&parent_state, &op)?;
                let next_state = Self::advance_state(&parent_state, &op)?;
                known_states.insert(Self::state_key(&next_state), next_state);
                progressed = true;
            }

            if !progressed {
                return Err(SyncError::VerificationFailed {
                    target: "anti_entropy_parent_chain",
                    detail:
                        "oplog contains an operation with an unknown or inconsistent parent state"
                            .to_string(),
                });
            }

            pending = next_pending;
        }

        Ok(known_states)
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
    pub async fn merge_remote_ops(
        &self,
        ops: VerifiedIngress<Vec<AttestedOp>>,
    ) -> Result<(), SyncError> {
        let (ops, _) = ops.into_parts();
        let anchor = self.verification_anchor.read().await.clone();
        let mut state = self.state.write().await;
        let mut known_states = Self::build_known_states(&anchor, &state.oplog)?;
        let mut existing_fingerprints = state
            .oplog
            .iter()
            .map(Self::operation_fingerprint)
            .collect::<Result<BTreeSet<_>, _>>()?;
        let mut batch_fingerprints = BTreeSet::new();
        let mut pending = Vec::new();

        for op in ops {
            let fingerprint = Self::operation_fingerprint(&op)?;
            if !batch_fingerprints.insert(fingerprint) {
                return Err(SyncError::VerificationFailed {
                    target: "anti_entropy_batch_replay",
                    detail: format!("duplicate operation in remote batch {fingerprint:?}"),
                });
            }
            if existing_fingerprints.contains(&fingerprint) {
                return Err(SyncError::VerificationFailed {
                    target: "anti_entropy_replay",
                    detail: format!("duplicate remote operation {fingerprint:?}"),
                });
            }
            existing_fingerprints.insert(fingerprint);
            pending.push(op);
        }

        while !pending.is_empty() {
            let mut next_pending = Vec::new();
            let mut progressed = false;

            for op in pending {
                let Some(parent_state) = known_states.get(&Self::parent_key(&op)).cloned() else {
                    next_pending.push(op);
                    continue;
                };

                Self::verify_operation(&parent_state, &op)?;
                let next_state = Self::advance_state(&parent_state, &op)?;
                known_states.insert(Self::state_key(&next_state), next_state);
                state.oplog.push(op);
                progressed = true;
            }

            if !progressed {
                return Err(SyncError::VerificationFailed {
                    target: "anti_entropy_remote_parent_chain",
                    detail:
                        "remote batch contains an operation with an unknown or inconsistent parent state"
                            .to_string(),
                });
            }

            pending = next_pending;
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
            let missing_ops = verified_ops_from_peer(peer_id, missing_ops)?;
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
    use aura_core::crypto::tree_signing::tree_op_binding_message;
    use aura_core::tree::{commit_branch, BranchNode, BranchSigningKey, LeafId};
    use aura_core::{LeafNode, Policy, TreeOp, TreeOpKind};
    use frost_ed25519 as frost;
    use rand_chacha::ChaCha20Rng;
    use rand_core::SeedableRng;
    use std::collections::BTreeMap;

    struct TestSigningFixture {
        key_packages: Vec<frost::keys::KeyPackage>,
        public_key_package: frost::keys::PublicKeyPackage,
        group_public_key: [u8; 32],
    }

    fn verification_anchor() -> (TreeState, TestSigningFixture) {
        let mut rng = ChaCha20Rng::from_seed([7u8; 32]);
        let (secret_shares, public_key_package) =
            frost::keys::generate_with_dealer(2, 2, frost::keys::IdentifierList::Default, &mut rng)
                .expect("generate test FROST keys");
        let key_packages = secret_shares
            .values()
            .map(|share| {
                share
                    .clone()
                    .try_into()
                    .expect("convert secret share to key package")
            })
            .collect::<Vec<frost::keys::KeyPackage>>();
        let group_public_key = public_key_package.verifying_key().serialize();

        let mut state = TreeState::new();
        state.epoch = Epoch::new(1);

        let root = NodeIndex(0);
        let policy = Policy::All;
        let left_child_commitment = commit_branch(
            NodeIndex(1),
            state.current_epoch(),
            &Policy::Any,
            &[0u8; 32],
            &[0u8; 32],
        );
        let right_child_commitment = commit_branch(
            NodeIndex(2),
            state.current_epoch(),
            &Policy::Any,
            &[0u8; 32],
            &[0u8; 32],
        );
        let root_commitment = commit_branch(
            root,
            state.current_epoch(),
            &policy,
            &left_child_commitment,
            &right_child_commitment,
        );

        state.add_branch_with_parent(
            BranchNode {
                node: root,
                policy,
                commitment: root_commitment,
            },
            None,
        );
        state.add_branch_with_parent(
            BranchNode {
                node: NodeIndex(1),
                policy: Policy::Any,
                commitment: left_child_commitment,
            },
            Some(root),
        );
        state.add_branch_with_parent(
            BranchNode {
                node: NodeIndex(2),
                policy: Policy::Any,
                commitment: right_child_commitment,
            },
            Some(root),
        );
        state.set_root_commitment(root_commitment);
        state.set_signing_key(
            root,
            BranchSigningKey::new(group_public_key, state.current_epoch()),
        );

        (
            state,
            TestSigningFixture {
                key_packages,
                public_key_package,
                group_public_key,
            },
        )
    }

    fn signed_add_leaf_op(
        parent_state: &TreeState,
        fixture: &TestSigningFixture,
        leaf_id: u64,
        parent_epoch: Option<Epoch>,
        parent_commitment: Option<TreeHash32>,
    ) -> AttestedOp {
        let op = TreeOp {
            parent_epoch: parent_epoch.unwrap_or(parent_state.current_epoch()),
            parent_commitment: parent_commitment.unwrap_or(parent_state.current_commitment()),
            op: TreeOpKind::AddLeaf {
                leaf: LeafNode::new_device(
                    LeafId(u32::try_from(leaf_id).expect("leaf id fits in u32")),
                    test_device(100 + u128::from(leaf_id)),
                    vec![u8::try_from(leaf_id).expect("leaf id fits in u8"); 32],
                )
                .expect("valid leaf"),
                under: NodeIndex(0),
            },
            version: 1,
        };
        let mut attested = AttestedOp {
            op,
            agg_sig: Vec::new(),
            signer_count: u16::try_from(fixture.key_packages.len())
                .expect("test signer count fits in u16"),
        };
        let binding = tree_op_binding_message(
            &attested,
            parent_state.current_epoch(),
            &fixture.group_public_key,
        );
        let mut commitments = BTreeMap::new();
        let mut signing_inputs = Vec::new();
        for (index, key_package) in fixture.key_packages.iter().enumerate() {
            let mut rng = ChaCha20Rng::from_seed(
                [u8::try_from(leaf_id + index as u64).expect("seed fits in u8"); 32],
            );
            let identifier = *key_package.identifier();
            let (nonce, commitment) = frost::round1::commit(key_package.signing_share(), &mut rng);
            commitments.insert(identifier, commitment);
            signing_inputs.push((identifier, nonce, key_package));
        }
        let signing_package = frost::SigningPackage::new(commitments, &binding);
        let signature_shares = signing_inputs
            .into_iter()
            .map(|(identifier, nonce, key_package)| {
                let share = frost::round2::sign(&signing_package, &nonce, key_package)
                    .expect("sign attested op");
                (identifier, share)
            })
            .collect::<BTreeMap<_, _>>();
        let signature = frost::aggregate(
            &signing_package,
            &signature_shares,
            &fixture.public_key_package,
        )
        .expect("aggregate attested op signature");
        attested.agg_sig = signature.serialize().as_ref().to_vec();
        attested
    }

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
        let (anchor, fixture) = verification_anchor();
        handler.set_verification_anchor(anchor.clone()).await;

        let op1 = signed_add_leaf_op(&anchor, &fixture, 2, None, None);
        let state_after_op1 = AntiEntropyHandler::advance_state(&anchor, &op1).unwrap();
        let op2 = signed_add_leaf_op(&state_after_op1, &fixture, 3, None, None);

        let peer = test_device(9);
        handler
            .merge_remote_ops(verified_ops_from_peer(peer, vec![op1, op2]).unwrap())
            .await
            .unwrap();

        let ops = handler.get_ops().await;
        assert_eq!(ops.len(), 2);
    }

    #[tokio::test]
    async fn merge_remote_ops_rejects_forged_signature() {
        let context_id = test_context(8);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);
        let (anchor, fixture) = verification_anchor();
        handler.set_verification_anchor(anchor.clone()).await;

        let mut op = signed_add_leaf_op(&anchor, &fixture, 4, None, None);
        op.agg_sig[0] ^= 0x55;

        let error = handler
            .merge_remote_ops(verified_ops_from_peer(test_device(10), vec![op]).unwrap())
            .await
            .expect_err("forged signature must be rejected");
        assert!(matches!(
            error,
            SyncError::VerificationFailed {
                target: "anti_entropy_attested_op",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn merge_remote_ops_rejects_stale_epoch() {
        let context_id = test_context(9);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);
        let (anchor, fixture) = verification_anchor();
        handler.set_verification_anchor(anchor.clone()).await;

        let stale_op = signed_add_leaf_op(
            &anchor,
            &fixture,
            5,
            Some(Epoch::initial()),
            Some(anchor.current_commitment()),
        );

        let error = handler
            .merge_remote_ops(verified_ops_from_peer(test_device(11), vec![stale_op]).unwrap())
            .await
            .expect_err("stale epoch must be rejected");
        assert!(matches!(
            error,
            SyncError::VerificationFailed {
                target: "anti_entropy_parent_chain"
                    | "anti_entropy_parent_epoch"
                    | "anti_entropy_remote_parent_chain",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn merge_remote_ops_rejects_wrong_parent_commitment() {
        let context_id = test_context(10);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);
        let (anchor, fixture) = verification_anchor();
        handler.set_verification_anchor(anchor.clone()).await;

        let wrong_parent =
            signed_add_leaf_op(&anchor, &fixture, 6, None, Some(Hash32([9u8; 32]).0));

        let error = handler
            .merge_remote_ops(verified_ops_from_peer(test_device(12), vec![wrong_parent]).unwrap())
            .await
            .expect_err("wrong parent must be rejected");
        assert!(matches!(
            error,
            SyncError::VerificationFailed {
                target: "anti_entropy_remote_parent_chain" | "anti_entropy_parent_commitment",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn merge_remote_ops_rejects_duplicate_operations() {
        let context_id = test_context(11);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);
        let (anchor, fixture) = verification_anchor();
        handler.set_verification_anchor(anchor.clone()).await;

        let op = signed_add_leaf_op(&anchor, &fixture, 7, None, None);
        handler
            .merge_remote_ops(verified_ops_from_peer(test_device(13), vec![op.clone()]).unwrap())
            .await
            .expect("initial merge should succeed");

        let error = handler
            .merge_remote_ops(verified_ops_from_peer(test_device(13), vec![op]).unwrap())
            .await
            .expect_err("duplicate op must be rejected");
        assert!(matches!(
            error,
            SyncError::VerificationFailed {
                target: "anti_entropy_replay",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn merge_remote_ops_rejects_duplicate_operations_within_batch() {
        let context_id = test_context(12);
        let handler = AntiEntropyHandler::new(AntiEntropyConfig::default(), context_id);
        let (anchor, fixture) = verification_anchor();
        handler.set_verification_anchor(anchor.clone()).await;

        let op = signed_add_leaf_op(&anchor, &fixture, 8, None, None);

        let error = handler
            .merge_remote_ops(
                verified_ops_from_peer(test_device(14), vec![op.clone(), op]).unwrap(),
            )
            .await
            .expect_err("batch duplicate must be rejected");
        assert!(matches!(
            error,
            SyncError::VerificationFailed {
                target: "anti_entropy_batch_replay",
                ..
            }
        ));
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
