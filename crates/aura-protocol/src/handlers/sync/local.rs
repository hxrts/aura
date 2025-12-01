// Lock poisoning is fatal for this module - we prefer to panic than continue with corrupted state
#![allow(clippy::expect_used)]

use crate::effects::sync::{BloomDigest, SyncEffects, SyncError};
use async_trait::async_trait;
use aura_core::hash;
use aura_core::tree::AttestedOp;
use aura_core::{AuraError, Hash32};
use bincode;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// In-memory sync handler that shares the same OpLog buffer used by tree handlers.
#[derive(Clone)]
pub struct LocalSyncHandler {
    oplog: Arc<RwLock<Vec<AttestedOp>>>,
}

impl LocalSyncHandler {
    pub fn new(oplog: Arc<RwLock<Vec<AttestedOp>>>) -> Self {
        Self { oplog }
    }

    fn cid_for(op: &AttestedOp) -> Result<Hash32, AuraError> {
        let bytes = bincode::serialize(op)
            .map_err(|e| AuraError::internal(format!("cid serialize: {e}")))?;
        Ok(Hash32(hash::hash(&bytes)))
    }
}

#[async_trait]
impl SyncEffects for LocalSyncHandler {
    async fn sync_with_peer(&self, _peer_id: Uuid) -> Result<(), SyncError> {
        // No-op local sync; real networking handled in higher layers.
        Ok(())
    }

    async fn get_oplog_digest(&self) -> Result<BloomDigest, SyncError> {
        let ops = self.oplog.read().expect("LocalSyncHandler lock poisoned");
        let mut cids = BTreeSet::new();
        for op in ops.iter() {
            if let Ok(cid) = Self::cid_for(op) {
                cids.insert(cid);
            }
        }
        Ok(BloomDigest { cids })
    }

    async fn get_missing_ops(
        &self,
        _remote_digest: &BloomDigest,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        // Return full oplog; guard chain filters where needed.
        let ops = self.oplog.read().expect("LocalSyncHandler lock poisoned");
        Ok(ops.clone())
    }

    async fn request_ops_from_peer(
        &self,
        _peer_id: Uuid,
        _cids: Vec<Hash32>,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        // Local handler has no network; return empty to indicate no additional ops.
        Ok(Vec::new())
    }

    async fn merge_remote_ops(&self, ops: Vec<AttestedOp>) -> Result<(), SyncError> {
        let mut store = self.oplog.write().expect("LocalSyncHandler lock poisoned");
        let mut existing: HashSet<Hash32> = HashSet::new();
        for op in store.iter() {
            if let Ok(cid) = Self::cid_for(op) {
                existing.insert(cid);
            }
        }
        for op in ops {
            let cid =
                Self::cid_for(&op).map_err(|e| SyncError::VerificationFailed(e.to_string()))?;
            if !existing.contains(&cid) {
                store.push(op);
                existing.insert(cid);
            }
        }
        Ok(())
    }

    async fn announce_new_op(&self, _cid: Hash32) -> Result<(), SyncError> {
        Ok(())
    }

    async fn request_op(&self, _peer_id: Uuid, cid: Hash32) -> Result<AttestedOp, SyncError> {
        let store = self.oplog.read().expect("LocalSyncHandler lock poisoned");
        for op in store.iter() {
            if let Ok(found) = Self::cid_for(op) {
                if found == cid {
                    return Ok(op.clone());
                }
            }
        }
        Err(SyncError::OperationNotFound)
    }

    async fn push_op_to_peers(&self, _op: AttestedOp, _peers: Vec<Uuid>) -> Result<(), SyncError> {
        // Local handler doesn't push to peers; real networking handled in higher layers.
        Ok(())
    }

    async fn get_connected_peers(&self) -> Result<Vec<Uuid>, SyncError> {
        // Local handler has no network peers.
        Ok(Vec::new())
    }
}
