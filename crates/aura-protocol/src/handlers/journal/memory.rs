//! Memory-based journal handler for testing
//!
//! This handler provides an in-memory implementation of JournalEffects
//! for testing and development. It stores the OpLog CRDT in memory and
//! computes TreeState on-demand via reduction.

use crate::effects::journal::{
    CapabilityId, CapabilityRef, Commitment, Epoch, Intent, IntentId, IntentStatus, JournalEffects,
    JournalMap, JournalStats, LeafIndex, RatchetTree, TreeOpRecord,
};
use aura_core::{AttestedOp, Hash32};
use aura_journal::{
    ratchet_tree::{reduce, TreeState},
    semilattice::OpLog,
};
use blake3::Hasher;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Memory-based journal handler for testing
///
/// Stores the OpLog CRDT in memory. TreeState is never stored directly -
/// it's always computed on-demand via the reduction function.
pub struct MemoryJournalHandler {
    /// OpLog CRDT storing all attested operations
    oplog: Arc<RwLock<OpLog>>,
}

impl MemoryJournalHandler {
    /// Create a new memory-based journal handler
    pub fn new() -> Self {
        Self {
            oplog: Arc::new(RwLock::new(OpLog::default())),
        }
    }
}

impl Default for MemoryJournalHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl JournalEffects for MemoryJournalHandler {
    async fn append_attested_tree_op(
        &self,
        op: AttestedOp,
    ) -> Result<Hash32, aura_core::AuraError> {
        let mut oplog = self.oplog.write().await;

        // Compute CID for the operation
        let cid = compute_op_cid(&op);

        // Append to OpLog (idempotent - won't duplicate if already present)
        oplog.append(op);

        Ok(cid)
    }

    async fn get_tree_state(&self) -> Result<TreeState, aura_core::AuraError> {
        let oplog = self.oplog.read().await;

        // Get all operations from OpLog
        let ops: Vec<&AttestedOp> = oplog.list_ops();

        // Convert to owned for reduction
        let owned_ops: Vec<AttestedOp> = ops.into_iter().cloned().collect();

        // Compute TreeState via reduction (never stored)
        reduce(&owned_ops)
            .map_err(|e| aura_core::AuraError::internal(&format!("Reduction failed: {}", e)))
    }

    async fn get_op_log(&self) -> Result<OpLog, aura_core::AuraError> {
        let oplog = self.oplog.read().await;
        Ok(oplog.clone())
    }

    async fn merge_op_log(&self, remote: OpLog) -> Result<(), aura_core::AuraError> {
        let mut oplog = self.oplog.write().await;

        // CRDT merge (union of operations) - simple approach TODO fix - For now
        // TODO: Implement proper CRDT join semantics
        for op in remote.list_ops() {
            oplog.append(op.clone());
        }

        Ok(())
    }

    async fn get_attested_op(
        &self,
        cid: &Hash32,
    ) -> Result<Option<AttestedOp>, aura_core::AuraError> {
        let oplog = self.oplog.read().await;
        Ok(oplog.get_operation(cid).cloned())
    }

    async fn list_attested_ops(&self) -> Result<Vec<AttestedOp>, aura_core::AuraError> {
        let oplog = self.oplog.read().await;
        Ok(oplog.to_operations_vec())
    }

    // Stub implementations for other JournalEffects methods
    async fn get_journal_state(&self) -> Result<JournalMap, aura_core::AuraError> {
        Ok(JournalMap(std::collections::HashMap::new()))
    }

    async fn get_current_tree(&self) -> Result<RatchetTree, aura_core::AuraError> {
        Ok(RatchetTree("test".to_string()))
    }

    async fn get_tree_at_epoch(&self, _epoch: Epoch) -> Result<RatchetTree, aura_core::AuraError> {
        Ok(RatchetTree("test".to_string()))
    }

    async fn get_current_commitment(&self) -> Result<Commitment, aura_core::AuraError> {
        Ok(Commitment(vec![0u8; 32]))
    }

    async fn get_latest_epoch(&self) -> Result<Option<Epoch>, aura_core::AuraError> {
        Ok(Some(Epoch(0)))
    }

    async fn append_tree_op(&self, _op: TreeOpRecord) -> Result<(), aura_core::AuraError> {
        Ok(())
    }

    async fn get_tree_op(
        &self,
        _epoch: Epoch,
    ) -> Result<Option<TreeOpRecord>, aura_core::AuraError> {
        Ok(None)
    }

    async fn list_tree_ops(&self) -> Result<Vec<TreeOpRecord>, aura_core::AuraError> {
        Ok(vec![])
    }

    async fn submit_intent(&self, _intent: Intent) -> Result<IntentId, aura_core::AuraError> {
        Ok(IntentId("test".to_string()))
    }

    async fn get_intent(
        &self,
        _intent_id: IntentId,
    ) -> Result<Option<Intent>, aura_core::AuraError> {
        Ok(None)
    }

    async fn get_intent_status(
        &self,
        _intent_id: IntentId,
    ) -> Result<IntentStatus, aura_core::AuraError> {
        Ok(IntentStatus("pending".to_string()))
    }

    async fn list_pending_intents(&self) -> Result<Vec<Intent>, aura_core::AuraError> {
        Ok(vec![])
    }

    async fn tombstone_intent(&self, _intent_id: IntentId) -> Result<(), aura_core::AuraError> {
        Ok(())
    }

    async fn prune_stale_intents(
        &self,
        _current_commitment: Commitment,
    ) -> Result<usize, aura_core::AuraError> {
        Ok(0)
    }

    async fn validate_capability(
        &self,
        _capability: &CapabilityRef,
    ) -> Result<bool, aura_core::AuraError> {
        Ok(true)
    }

    async fn is_capability_revoked(
        &self,
        _capability_id: &CapabilityId,
    ) -> Result<bool, aura_core::AuraError> {
        Ok(false)
    }

    async fn list_capabilities_in_op(
        &self,
        _epoch: Epoch,
    ) -> Result<Vec<CapabilityRef>, aura_core::AuraError> {
        Ok(vec![])
    }

    async fn merge_journal_state(&self, _other: JournalMap) -> Result<(), aura_core::AuraError> {
        Ok(())
    }

    async fn get_journal_stats(&self) -> Result<JournalStats, aura_core::AuraError> {
        Ok(JournalStats {
            entry_count: 0,
            total_size: 0,
        })
    }

    async fn is_device_member(
        &self,
        _device_id: aura_core::identifiers::DeviceId,
    ) -> Result<bool, aura_core::AuraError> {
        Ok(true)
    }

    async fn get_device_leaf_index(
        &self,
        _device_id: aura_core::identifiers::DeviceId,
    ) -> Result<Option<LeafIndex>, aura_core::AuraError> {
        Ok(None)
    }

    async fn list_devices(
        &self,
    ) -> Result<Vec<aura_core::identifiers::DeviceId>, aura_core::AuraError> {
        Ok(vec![])
    }

    async fn list_guardians(
        &self,
    ) -> Result<Vec<aura_core::identifiers::GuardianId>, aura_core::AuraError> {
        Ok(vec![])
    }
}

/// Compute a content-addressed identifier (CID) for an operation
///
/// Uses BLAKE3 to hash the serialized operation. This produces a
/// deterministic identifier that can be used to reference operations.
fn compute_op_cid(op: &AttestedOp) -> Hash32 {
    let mut hasher = Hasher::new();

    // Hash operation fields
    hasher.update(&op.op.parent_epoch.to_le_bytes());
    hasher.update(&op.op.parent_commitment);
    hasher.update(&op.op.version.to_le_bytes());

    // Hash operation kind (TODO fix - Simplified - could use full serialization)
    match &op.op.op {
        aura_core::TreeOpKind::AddLeaf { leaf, under } => {
            hasher.update(b"AddLeaf");
            hasher.update(&leaf.leaf_id.0.to_le_bytes());
            hasher.update(&under.0.to_le_bytes());
        }
        aura_core::TreeOpKind::RemoveLeaf { leaf, reason } => {
            hasher.update(b"RemoveLeaf");
            hasher.update(&leaf.0.to_le_bytes());
            hasher.update(&[*reason]);
        }
        aura_core::TreeOpKind::ChangePolicy { node, new_policy } => {
            hasher.update(b"ChangePolicy");
            hasher.update(&node.0.to_le_bytes());
            hasher.update(&aura_core::policy_hash(new_policy));
        }
        aura_core::TreeOpKind::RotateEpoch { affected } => {
            hasher.update(b"RotateEpoch");
            for node in affected {
                hasher.update(&node.0.to_le_bytes());
            }
        }
    }

    // Hash signature and count
    hasher.update(&op.agg_sig);
    hasher.update(&op.signer_count.to_le_bytes());

    let hash = hasher.finalize();
    let mut result = [0u8; 32];
    result.copy_from_slice(hash.as_bytes());
    aura_core::Hash32(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{LeafId, LeafNode, LeafRole, NodeIndex, TreeOp, TreeOpKind};

    fn create_test_op(leaf_id: u32) -> AttestedOp {
        AttestedOp {
            op: TreeOp {
                parent_epoch: 0,
                parent_commitment: [0u8; 32],
                op: TreeOpKind::AddLeaf {
                    leaf: LeafNode {
                        leaf_id: LeafId(leaf_id),
                        role: LeafRole::Device,
                        public_key: vec![leaf_id as u8; 32],
                        meta: vec![],
                        device_id: DeviceId::from_bytes([leaf_id as u8; 32]),
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
    async fn test_append_and_retrieve() {
        let handler = MemoryJournalHandler::new();
        let op = create_test_op(1);

        // Append operation
        let cid = handler.append_attested_tree_op(op.clone()).await.unwrap();

        // Retrieve by CID
        let retrieved = handler.get_attested_op(&cid).await.unwrap();
        assert_eq!(retrieved, Some(op));
    }

    #[tokio::test]
    async fn test_list_operations() {
        let handler = MemoryJournalHandler::new();

        // Append multiple operations
        handler
            .append_attested_tree_op(create_test_op(1))
            .await
            .unwrap();
        handler
            .append_attested_tree_op(create_test_op(2))
            .await
            .unwrap();
        handler
            .append_attested_tree_op(create_test_op(3))
            .await
            .unwrap();

        // List all operations
        let ops = handler.list_attested_ops().await.unwrap();
        assert_eq!(ops.len(), 3);
    }

    #[tokio::test]
    async fn test_tree_state_computation() {
        let handler = MemoryJournalHandler::new();

        // Initially empty
        let state = handler.get_tree_state().await.unwrap();
        assert!(state.is_empty());

        // Add operations
        handler
            .append_attested_tree_op(create_test_op(1))
            .await
            .unwrap();
        handler
            .append_attested_tree_op(create_test_op(2))
            .await
            .unwrap();

        // TreeState should reflect operations
        let state = handler.get_tree_state().await.unwrap();
        assert_eq!(state.num_leaves(), 2);
    }

    #[tokio::test]
    async fn test_oplog_merge() {
        let handler1 = MemoryJournalHandler::new();
        let handler2 = MemoryJournalHandler::new();

        // Add different operations to each handler
        handler1
            .append_attested_tree_op(create_test_op(1))
            .await
            .unwrap();
        handler2
            .append_attested_tree_op(create_test_op(2))
            .await
            .unwrap();

        // Merge handler2's oplog into handler1
        let oplog2 = handler2.get_op_log().await.unwrap();
        handler1.merge_op_log(oplog2).await.unwrap();

        // Handler1 should have both operations
        let ops = handler1.list_attested_ops().await.unwrap();
        assert_eq!(ops.len(), 2);
    }

    #[test]
    fn test_cid_deterministic() {
        let op = create_test_op(1);
        let cid1 = compute_op_cid(&op);
        let cid2 = compute_op_cid(&op);
        assert_eq!(cid1, cid2);
    }

    #[test]
    fn test_cid_different_ops() {
        let op1 = create_test_op(1);
        let op2 = create_test_op(2);
        let cid1 = compute_op_cid(&op1);
        let cid2 = compute_op_cid(&op2);
        assert_ne!(cid1, cid2);
    }
}
