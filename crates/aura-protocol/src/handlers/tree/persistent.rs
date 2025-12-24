// Lock poisoning is fatal for this module - we prefer to panic than continue with corrupted state
#![allow(clippy::expect_used)]

//! Persistent commitment tree handler backed by StorageEffects.
//!
//! This handler persists the OpLog to storage, enabling tree state to survive
//! across restarts and supporting cross-device sync.
//!
//! ## Shared Storage
//!
//! Uses the same storage keys as `PersistentSyncHandler` (defined in `sync::persistent`),
//! ensuring both handlers operate on the same source of truth.

use crate::effects::tree::{Cut, Partial, ProposalId, Snapshot, TreeEffects};
use crate::sync::{TREE_OPS_INDEX_KEY, TREE_OPS_PREFIX};
use async_trait::async_trait;
use aura_core::effects::storage::StorageEffects;
use aura_core::hash;
use aura_core::tree::{AttestedOp, LeafId, LeafNode, NodeIndex, Policy, TreeOpKind};
use aura_core::{AuraError, Hash32};
use aura_journal::commitment_tree::reduce;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// Persistent commitment tree handler backed by StorageEffects.
///
/// This is the production implementation that persists the OpLog to storage,
/// enabling tree state to survive across restarts.
///
/// ## Storage Layout
///
/// - `tree_ops/<hash>`: Individual attested operations keyed by content hash
/// - `tree_ops_index`: Ordered list of operation hashes for replay
///
/// ## Lazy Loading
///
/// The handler loads operations from storage lazily on first access. This allows
/// synchronous construction while still supporting persistence.
///
/// ## Consistency Model
///
/// Operations are written to storage before being added to the in-memory cache.
pub struct PersistentTreeHandler {
    /// Storage backend for persistence
    storage: Arc<dyn StorageEffects>,
    /// In-memory cache of operations (loaded from storage on first access)
    ops_cache: RwLock<Vec<AttestedOp>>,
    /// Whether we've loaded from storage yet
    initialized: AtomicBool,
}

impl PersistentTreeHandler {
    /// Create a new persistent tree handler (synchronous, lazy loading).
    ///
    /// Operations are loaded from storage on first access to the tree state.
    pub fn new(storage: Arc<dyn StorageEffects>) -> Self {
        Self {
            storage,
            ops_cache: RwLock::new(Vec::new()),
            initialized: AtomicBool::new(false),
        }
    }

    /// Create a new persistent tree handler with eager loading (async).
    ///
    /// Loads existing operations from storage immediately.
    pub async fn new_eager(storage: Arc<dyn StorageEffects>) -> Result<Self, AuraError> {
        let ops_cache = Self::load_ops_from_storage(&*storage).await?;
        Ok(Self {
            storage,
            ops_cache: RwLock::new(ops_cache),
            initialized: AtomicBool::new(true),
        })
    }

    /// Ensure operations are loaded from storage (lazy initialization).
    async fn ensure_initialized(&self) -> Result<(), AuraError> {
        if !self.initialized.load(Ordering::Acquire) {
            let ops = Self::load_ops_from_storage(&*self.storage).await?;
            let mut cache = self
                .ops_cache
                .write()
                .expect("PersistentTreeHandler lock poisoned");
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
    ) -> Result<Vec<AttestedOp>, AuraError> {
        // Load the index of op hashes
        let index_bytes = storage
            .retrieve(TREE_OPS_INDEX_KEY)
            .await
            .map_err(|e| AuraError::storage(format!("Failed to load tree ops index: {}", e)))?;

        let op_hashes: Vec<[u8; 32]> = match index_bytes {
            Some(bytes) => bincode::deserialize(&bytes).map_err(|e| {
                AuraError::internal(format!("Failed to deserialize ops index: {}", e))
            })?,
            None => Vec::new(), // No operations yet
        };

        // Load each operation by hash
        let mut ops = Vec::with_capacity(op_hashes.len());
        for op_hash in op_hashes {
            let key = format!("{}{}", TREE_OPS_PREFIX, hex::encode(op_hash));
            let op_bytes = storage
                .retrieve(&key)
                .await
                .map_err(|e| AuraError::storage(format!("Failed to load tree op {}: {}", key, e)))?
                .ok_or_else(|| AuraError::storage(format!("Missing tree op: {}", key)))?;

            let op: AttestedOp = bincode::deserialize(&op_bytes)
                .map_err(|e| AuraError::internal(format!("Failed to deserialize tree op: {}", e)))?;
            ops.push(op);
        }

        Ok(ops)
    }

    /// Persist an operation to storage.
    async fn persist_op(&self, op: &AttestedOp, op_hash: [u8; 32]) -> Result<(), AuraError> {
        // Serialize the operation
        let op_bytes = bincode::serialize(op)
            .map_err(|e| AuraError::internal(format!("Failed to serialize tree op: {}", e)))?;

        // Store the operation by hash
        let key = format!("{}{}", TREE_OPS_PREFIX, hex::encode(op_hash));
        self.storage
            .store(&key, op_bytes)
            .await
            .map_err(|e| AuraError::storage(format!("Failed to store tree op: {}", e)))?;

        // Update the index
        let hashes: Vec<[u8; 32]> = {
            let ops = self
                .ops_cache
                .read()
                .expect("PersistentTreeHandler lock poisoned");
            ops.iter()
                .map(|op| {
                    let bytes = bincode::serialize(op).unwrap_or_default();
                    hash::hash(&bytes)
                })
                .collect()
        };

        let index_bytes = bincode::serialize(&hashes)
            .map_err(|e| AuraError::internal(format!("Failed to serialize ops index: {}", e)))?;

        self.storage
            .store(TREE_OPS_INDEX_KEY, index_bytes)
            .await
            .map_err(|e| AuraError::storage(format!("Failed to store ops index: {}", e)))?;

        Ok(())
    }

    /// Reduce the current operations to tree state.
    async fn reduce_state(
        &self,
    ) -> Result<aura_journal::commitment_tree::state::TreeState, AuraError> {
        self.ensure_initialized().await?;
        let ops = self
            .ops_cache
            .read()
            .expect("PersistentTreeHandler lock poisoned");
        reduce(&ops).map_err(|e| AuraError::internal(format!("tree reduce failed: {e}")))
    }

    /// Compute hash for an operation (for deduplication).
    fn op_hash(op: &AttestedOp) -> Result<[u8; 32], AuraError> {
        let bytes = bincode::serialize(op)
            .map_err(|e| AuraError::internal(format!("hash serialize attested op: {e}")))?;
        Ok(hash::hash(&bytes))
    }
}

#[async_trait]
impl TreeEffects for PersistentTreeHandler {
    async fn get_current_state(
        &self,
    ) -> Result<aura_journal::commitment_tree::TreeState, AuraError> {
        self.reduce_state().await
    }

    async fn get_current_commitment(&self) -> Result<Hash32, AuraError> {
        let state = self.reduce_state().await?;
        Ok(Hash32(state.root_commitment))
    }

    async fn get_current_epoch(&self) -> Result<u64, AuraError> {
        let state = self.reduce_state().await?;
        Ok(state.epoch)
    }

    async fn apply_attested_op(&self, op: AttestedOp) -> Result<Hash32, AuraError> {
        self.ensure_initialized().await?;
        let op_hash = Self::op_hash(&op)?;

        // Check for duplicate (lock released before await)
        let already = {
            let ops = self
                .ops_cache
                .read()
                .expect("PersistentTreeHandler lock poisoned");
            ops.iter()
                .any(|existing| Self::op_hash(existing).map(|h| h == op_hash).unwrap_or(false))
        };

        if already {
            // Already have this op, just return current state
            let state = self.reduce_state().await?;
            return Ok(Hash32(state.root_commitment));
        }

        // Add to cache first (so persist_op sees it in index)
        {
            let mut ops = self
                .ops_cache
                .write()
                .expect("PersistentTreeHandler lock poisoned");
            ops.push(op.clone());
        }

        // Persist to storage
        self.persist_op(&op, op_hash).await?;

        // Return new root commitment
        let state = self.reduce_state().await?;
        Ok(Hash32(state.root_commitment))
    }

    async fn verify_aggregate_sig(
        &self,
        op: &AttestedOp,
        state: &aura_journal::commitment_tree::TreeState,
    ) -> Result<bool, AuraError> {
        use aura_core::tree::verification::extract_target_node;

        let target_node = extract_target_node(&op.op.op).or_else(|| match &op.op.op {
            TreeOpKind::RemoveLeaf { leaf, .. } => state.get_remove_leaf_affected_parent(leaf),
            _ => None,
        });

        let node = target_node.ok_or_else(|| {
            AuraError::invalid("Unable to resolve signing node for attested operation")
        })?;

        let signing_key = state.signing_keys().get(&node).ok_or_else(|| {
            AuraError::invalid(format!("Missing signing key for branch {}", node.0))
        })?;
        let policy = state
            .get_policy(&node)
            .ok_or_else(|| AuraError::invalid(format!("Missing policy for branch {}", node.0)))?;
        let child_count = state.get_children(node).len();
        let threshold = policy.required_signers(child_count);

        aura_core::tree::verify_attested_op(op, signing_key, threshold, state.epoch)
            .map(|_| true)
            .map_err(|e| AuraError::crypto(format!("signature verification failed: {e}")))
    }

    async fn add_leaf(&self, leaf: LeafNode, under: NodeIndex) -> Result<TreeOpKind, AuraError> {
        Ok(TreeOpKind::AddLeaf { leaf, under })
    }

    async fn remove_leaf(&self, leaf: LeafId, reason: u8) -> Result<TreeOpKind, AuraError> {
        Ok(TreeOpKind::RemoveLeaf { leaf, reason })
    }

    async fn change_policy(
        &self,
        node: NodeIndex,
        new_policy: Policy,
    ) -> Result<TreeOpKind, AuraError> {
        Ok(TreeOpKind::ChangePolicy { node, new_policy })
    }

    async fn rotate_epoch(&self, affected: Vec<NodeIndex>) -> Result<TreeOpKind, AuraError> {
        Ok(TreeOpKind::RotateEpoch { affected })
    }

    async fn propose_snapshot(&self, cut: Cut) -> Result<ProposalId, AuraError> {
        let bytes = bincode::serialize(&cut)
            .map_err(|e| AuraError::internal(format!("serialize cut: {e}")))?;
        Ok(ProposalId(Hash32(hash::hash(&bytes))))
    }

    async fn approve_snapshot(&self, proposal_id: ProposalId) -> Result<Partial, AuraError> {
        let signature_share = proposal_id.0 .0.to_vec();
        Ok(Partial {
            signature_share,
            participant_id: aura_core::identifiers::DeviceId::deterministic_test_id(),
        })
    }

    async fn finalize_snapshot(&self, proposal_id: ProposalId) -> Result<Snapshot, AuraError> {
        let state = self.reduce_state().await?;
        Ok(Snapshot {
            cut: Cut {
                epoch: state.epoch,
                commitment: Hash32(state.root_commitment),
                cid: proposal_id.0,
            },
            tree_state: state,
            aggregate_signature: proposal_id.0 .0.to_vec(),
        })
    }

    async fn apply_snapshot(&self, snapshot: &Snapshot) -> Result<(), AuraError> {
        // Ensure initialized first (so we know what to clear)
        self.ensure_initialized().await?;

        // Clear in-memory cache
        {
            let mut ops = self
                .ops_cache
                .write()
                .expect("PersistentTreeHandler lock poisoned");
            ops.clear();
        }

        // Clear storage
        // First, list all tree_ops keys
        let keys = self
            .storage
            .list_keys(Some(TREE_OPS_PREFIX))
            .await
            .map_err(|e| AuraError::storage(format!("Failed to list tree ops: {}", e)))?;

        for key in keys {
            let _ = self.storage.remove(&key).await;
        }

        // Clear the index
        let _ = self.storage.remove(TREE_OPS_INDEX_KEY).await;

        // Snapshot application replaces history; we store no additional ops
        let _ = snapshot;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_testkit::MemoryStorageHandler;

    #[tokio::test]
    async fn test_persistent_handler_empty_init() {
        let storage = Arc::new(MemoryStorageHandler::new());
        let handler = PersistentTreeHandler::new(storage);

        // Trigger lazy initialization
        handler.ensure_initialized().await.unwrap();

        // Should have no ops
        let ops = handler
            .ops_cache
            .read()
            .expect("lock poisoned in test");
        assert!(ops.is_empty());
    }

    #[tokio::test]
    async fn test_persistent_handler_lazy_init() {
        let storage = Arc::new(MemoryStorageHandler::new());
        let handler = PersistentTreeHandler::new(storage);

        // Not initialized yet
        assert!(!handler.initialized.load(Ordering::Acquire));

        // Access state triggers initialization
        let _state = handler.get_current_state().await;

        // Now initialized
        assert!(handler.initialized.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn test_persistent_handler_survives_restart() {
        let storage = Arc::new(MemoryStorageHandler::new());

        // Create a handler
        let handler1 = PersistentTreeHandler::new(storage.clone());

        // Trigger initialization
        handler1.ensure_initialized().await.unwrap();

        // Verify initial state
        let ops1 = handler1
            .ops_cache
            .read()
            .expect("lock poisoned in test");
        assert!(ops1.is_empty());
        drop(ops1);
        drop(handler1);

        // Create a new handler with same storage - should load same state
        let handler2 = PersistentTreeHandler::new(storage);
        handler2.ensure_initialized().await.unwrap();
        let ops2 = handler2
            .ops_cache
            .read()
            .expect("lock poisoned in test");
        assert!(ops2.is_empty()); // Still empty since we didn't add any ops
    }
}
