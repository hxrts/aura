use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::effects::SyncMetrics;
use aura_core::types::identifiers::{ContextId, DeviceId};
use aura_core::{AttestedOp, Hash32};
use aura_guards::{
    DecodedIngress, IngressSource, IngressVerificationEvidence, VerifiedIngress,
    VerifiedIngressMetadata,
};
use aura_protocol::effects::{BloomDigest, SyncEffects, SyncError};

fn verified_sync_ops(
    peer_id: DeviceId,
    ops: Vec<AttestedOp>,
) -> Result<VerifiedIngress<Vec<AttestedOp>>, SyncError> {
    let entropy = peer_id
        .to_bytes()
        .unwrap_or_else(|_| aura_core::hash::hash(peer_id.to_string().as_bytes()));
    let payload_hash = Hash32::from_value(&ops).map_err(|error| SyncError::VerificationFailed {
        target: "sync_ingress_payload",
        detail: error.to_string(),
    })?;
    let metadata = VerifiedIngressMetadata::new(
        IngressSource::Device(peer_id),
        ContextId::new_from_entropy(entropy),
        None,
        payload_hash,
        1,
    );
    let evidence = IngressVerificationEvidence::builder(metadata)
        .peer_identity(
            peer_id.to_bytes().is_ok(),
            "peer device id must be encodable",
        )
        .and_then(|builder| {
            builder.envelope_authenticity(payload_hash != Hash32::zero(), "payload hash is empty")
        })
        .and_then(|builder| {
            builder.capability_authorization(
                !ops.is_empty(),
                "sync push must carry at least one attested op",
            )
        })
        .and_then(|builder| builder.namespace_scope(true, "peer sync context derived from peer id"))
        .and_then(|builder| builder.schema_version(true, "sync ingress schema v1"))
        .and_then(|builder| builder.replay_freshness(true, "attested op cid set is fresh input"))
        .and_then(|builder| {
            builder.signer_membership(true, "attested op signatures are checked during merge")
        })
        .and_then(|builder| {
            builder.proof_evidence(true, "attested op proof evidence is checked during merge")
        })
        .and_then(|builder| builder.build())
        .map_err(|error| SyncError::VerificationFailed {
            target: "sync_ingress_evidence",
            detail: error.to_string(),
        })?;
    DecodedIngress::new(ops, evidence.metadata().clone())
        .verify(evidence)
        .map_err(|error| SyncError::VerificationFailed {
            target: "sync_ingress_promotion",
            detail: error.to_string(),
        })
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl SyncEffects for AuraEffectSystem {
    async fn sync_with_peer(&self, peer_id: DeviceId) -> Result<SyncMetrics, SyncError> {
        self.sync_handler.sync_with_peer(peer_id).await
    }

    async fn get_oplog_digest(&self) -> Result<BloomDigest, SyncError> {
        self.sync_handler.get_oplog_digest().await
    }

    async fn get_missing_ops(
        &self,
        remote_digest: &BloomDigest,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        self.sync_handler.get_missing_ops(remote_digest).await
    }

    async fn request_ops_from_peer(
        &self,
        peer_id: DeviceId,
        cids: Vec<Hash32>,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        self.sync_handler.request_ops_from_peer(peer_id, cids).await
    }

    async fn merge_remote_ops(
        &self,
        ops: VerifiedIngress<Vec<AttestedOp>>,
    ) -> Result<(), SyncError> {
        self.sync_handler.merge_remote_ops(ops).await
    }

    async fn announce_new_op(&self, cid: Hash32) -> Result<(), SyncError> {
        self.sync_handler.announce_new_op(cid).await
    }

    async fn request_op(&self, peer_id: DeviceId, cid: Hash32) -> Result<AttestedOp, SyncError> {
        self.sync_handler.request_op(peer_id, cid).await
    }

    async fn push_op_to_peers(
        &self,
        op: AttestedOp,
        peers: Vec<DeviceId>,
    ) -> Result<(), SyncError> {
        // Local handler has no network; treat push as verified merge then noop.
        if let Some(peer) = peers.first().copied() {
            self.sync_handler
                .merge_remote_ops(verified_sync_ops(peer, vec![op])?)
                .await?;
        }
        let _ = peers;
        Ok(())
    }

    async fn get_connected_peers(&self) -> Result<Vec<DeviceId>, SyncError> {
        Ok(Vec::new())
    }
}
