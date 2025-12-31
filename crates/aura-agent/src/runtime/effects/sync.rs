use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::effects::SyncMetrics;
use aura_core::{AttestedOp, Hash32};
use aura_protocol::effects::{BloomDigest, SyncEffects, SyncError};

#[async_trait]
impl SyncEffects for AuraEffectSystem {
    async fn sync_with_peer(&self, peer_id: uuid::Uuid) -> Result<SyncMetrics, SyncError> {
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
        peer_id: uuid::Uuid,
        cids: Vec<Hash32>,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        self.sync_handler.request_ops_from_peer(peer_id, cids).await
    }

    async fn merge_remote_ops(&self, ops: Vec<AttestedOp>) -> Result<(), SyncError> {
        self.sync_handler.merge_remote_ops(ops).await
    }

    async fn announce_new_op(&self, cid: Hash32) -> Result<(), SyncError> {
        self.sync_handler.announce_new_op(cid).await
    }

    async fn request_op(&self, peer_id: uuid::Uuid, cid: Hash32) -> Result<AttestedOp, SyncError> {
        self.sync_handler.request_op(peer_id, cid).await
    }

    async fn push_op_to_peers(
        &self,
        op: AttestedOp,
        peers: Vec<uuid::Uuid>,
    ) -> Result<(), SyncError> {
        // Local handler has no network; treat push as merge then noop.
        self.sync_handler.merge_remote_ops(vec![op]).await?;
        let _ = peers;
        Ok(())
    }

    async fn get_connected_peers(&self) -> Result<Vec<uuid::Uuid>, SyncError> {
        Ok(Vec::new())
    }
}
