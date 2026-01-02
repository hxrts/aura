// Lock poisoning is fatal for this module - we prefer to panic than continue with corrupted state
#![allow(clippy::expect_used)]

//! Persistent sync handler backed by StorageEffects.
//!
//! This handler shares the same storage keys as `PersistentTreeHandler` via
//! `aura_journal::commitment_tree::storage`, ensuring both handlers operate on
//! the same source of truth for tree operations.

use super::effects::{BloomDigest, SyncEffects, SyncError, SyncMetrics};
use async_lock::RwLock;
use async_trait::async_trait;
use aura_core::effects::storage::StorageEffects;
use aura_core::identifiers::DeviceId;
use aura_core::tree::AttestedOp;
use aura_core::Hash32;
use aura_journal::commitment_tree::storage as tree_storage;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Persistent sync handler backed by StorageEffects.
///
/// This handler shares the same storage backend as `PersistentTreeHandler`,
/// ensuring consistent view of tree operations for sync and tree reduction.
///
/// ## Storage Layout
///
/// Uses the same storage keys as `PersistentTreeHandler`:
/// - `tree_ops/<hash>`: Individual attested operations keyed by content hash
/// - `tree_ops_index`: Ordered list of operation hashes
///
/// ## Lazy Loading
///
/// Operations are loaded from storage lazily on first access.
pub struct PersistentSyncHandler {
    /// Storage backend (shared with PersistentTreeHandler)
    storage: Arc<dyn StorageEffects>,
    /// In-memory cache of operations (loaded from storage on first access)
    ops_cache: RwLock<Vec<AttestedOp>>,
    /// Whether we've loaded from storage yet
    initialized: AtomicBool,
}

impl PersistentSyncHandler {
    /// Create a new persistent sync handler (synchronous, lazy loading).
    pub fn new(storage: Arc<dyn StorageEffects>) -> Self {
        Self {
            storage,
            ops_cache: RwLock::new(Vec::new()),
            initialized: AtomicBool::new(false),
        }
    }

    /// Create a new persistent sync handler with eager loading (async).
    pub async fn new_eager(storage: Arc<dyn StorageEffects>) -> Result<Self, aura_core::AuraError> {
        let ops_cache = Self::load_ops_from_storage(&*storage).await?;
        Ok(Self {
            storage,
            ops_cache: RwLock::new(ops_cache),
            initialized: AtomicBool::new(true),
        })
    }

    /// Ensure operations are loaded from storage (lazy initialization).
    async fn ensure_initialized(&self) -> Result<(), aura_core::AuraError> {
        if !self.initialized.load(Ordering::Acquire) {
            let ops = Self::load_ops_from_storage(&*self.storage).await?;
            let mut cache = self.ops_cache.write().await;
            if !self.initialized.load(Ordering::Acquire) {
                *cache = ops;
                self.initialized.store(true, Ordering::Release);
            }
        }
        Ok(())
    }

    /// Load all operations from storage in order.
    async fn load_ops_from_storage(
        storage: &dyn StorageEffects,
    ) -> Result<Vec<AttestedOp>, aura_core::AuraError> {
        use aura_core::AuraError;

        // Load the index of op hashes
        let index_bytes = storage
            .retrieve(tree_storage::TREE_OPS_INDEX_KEY)
            .await
            .map_err(|e| AuraError::storage(format!("Failed to load tree ops index: {e}")))?;

        let op_hashes: Vec<[u8; 32]> = match index_bytes {
            Some(bytes) => tree_storage::deserialize_op_index(&bytes)?,
            None => Vec::new(),
        };

        // Load each operation by hash
        let mut ops = Vec::with_capacity(op_hashes.len());
        for op_hash in op_hashes {
            let key = tree_storage::op_key(op_hash);
            let op_bytes = storage
                .retrieve(&key)
                .await
                .map_err(|e| AuraError::storage(format!("Failed to load tree op {key}: {e}")))?
                .ok_or_else(|| AuraError::storage(format!("Missing tree op: {key}")))?;

            let op: AttestedOp = tree_storage::deserialize_op(&op_bytes)?;
            ops.push(op);
        }

        Ok(ops)
    }

    /// Persist an operation to storage.
    async fn persist_op(
        &self,
        op: &AttestedOp,
        op_hash: [u8; 32],
    ) -> Result<(), aura_core::AuraError> {
        use aura_core::AuraError;

        // Serialize the operation
        let op_bytes = tree_storage::serialize_op(op)?;

        // Store the operation by hash
        let key = tree_storage::op_key(op_hash);
        self.storage
            .store(&key, op_bytes)
            .await
            .map_err(|e| AuraError::storage(format!("Failed to store tree op: {e}")))?;

        // Update the index
        let hashes: Vec<[u8; 32]> = {
            let ops = self.ops_cache.read().await;
            let mut hashes = Vec::with_capacity(ops.len());
            for op in ops.iter() {
                let op_hash = tree_storage::op_hash(op)?;
                hashes.push(op_hash);
            }
            hashes
        };

        let index_bytes = tree_storage::serialize_op_index(&hashes)?;

        self.storage
            .store(tree_storage::TREE_OPS_INDEX_KEY, index_bytes)
            .await
            .map_err(|e| AuraError::storage(format!("Failed to store ops index: {e}")))?;

        Ok(())
    }

    /// Invalidate the cache to force reload from storage on next access.
    ///
    /// This is useful when the tree handler has written new ops and we need
    /// to refresh the sync handler's view.
    pub fn invalidate_cache(&self) {
        self.initialized.store(false, Ordering::Release);
    }
}

#[async_trait]
impl SyncEffects for PersistentSyncHandler {
    async fn sync_with_peer(&self, _peer_id: DeviceId) -> Result<SyncMetrics, SyncError> {
        // No-op for local persistent sync; real networking handled by AntiEntropyHandler
        Ok(SyncMetrics::empty())
    }

    async fn get_oplog_digest(&self) -> Result<BloomDigest, SyncError> {
        self.ensure_initialized()
            .await
            .map_err(|e| SyncError::NetworkError(e.to_string()))?;

        let ops = self.ops_cache.read().await;

        let mut cids = BTreeSet::new();
        for op in ops.iter() {
            if let Ok(hash) = tree_storage::op_hash(op) {
                cids.insert(Hash32(hash));
            }
        }
        Ok(BloomDigest { cids })
    }

    async fn get_missing_ops(
        &self,
        _remote_digest: &BloomDigest,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        self.ensure_initialized()
            .await
            .map_err(|e| SyncError::NetworkError(e.to_string()))?;

        // Return full oplog; guard chain filters where needed
        let ops = self.ops_cache.read().await;
        Ok(ops.clone())
    }

    async fn request_ops_from_peer(
        &self,
        _peer_id: DeviceId,
        _cids: Vec<Hash32>,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        // Local handler has no network; return empty
        Ok(Vec::new())
    }

    async fn merge_remote_ops(&self, ops: Vec<AttestedOp>) -> Result<(), SyncError> {
        self.ensure_initialized()
            .await
            .map_err(|e| SyncError::NetworkError(e.to_string()))?;

        for op in ops {
            let op_hash = tree_storage::op_hash(&op)
                .map_err(|e| SyncError::VerificationFailed(e.to_string()))?;

            // Check for duplicate
            let already = {
                let store = self.ops_cache.read().await;
                store.iter().any(|existing| {
                    tree_storage::op_hash(existing)
                        .map(|h| h == op_hash)
                        .unwrap_or(false)
                })
            };

            if !already {
                // Add to cache
                {
                    let mut store = self.ops_cache.write().await;
                    store.push(op.clone());
                }

                // Persist to storage
                self.persist_op(&op, op_hash)
                    .await
                    .map_err(|e| SyncError::NetworkError(e.to_string()))?;
            }
        }
        Ok(())
    }

    async fn announce_new_op(&self, _cid: Hash32) -> Result<(), SyncError> {
        Ok(())
    }

    async fn request_op(&self, _peer_id: DeviceId, cid: Hash32) -> Result<AttestedOp, SyncError> {
        self.ensure_initialized()
            .await
            .map_err(|e| SyncError::NetworkError(e.to_string()))?;

        let store = self.ops_cache.read().await;

        for op in store.iter() {
            if let Ok(hash) = tree_storage::op_hash(op) {
                if Hash32(hash) == cid {
                    return Ok(op.clone());
                }
            }
        }
        Err(SyncError::OperationNotFound)
    }

    async fn push_op_to_peers(
        &self,
        _op: AttestedOp,
        _peers: Vec<DeviceId>,
    ) -> Result<(), SyncError> {
        // Local handler doesn't push to peers; real networking handled by BroadcasterHandler
        Ok(())
    }

    async fn get_connected_peers(&self) -> Result<Vec<DeviceId>, SyncError> {
        // Local handler has no network peers
        Ok(Vec::new())
    }
}
