//! Memory-based tree handler for testing
//!
//! **DEPRECATED**: This handler implements TreeEffects by delegating to JournalEffects.
//!
//! **Migration**: Use `ChoreographicTreeEffectHandler` from the choreographic module instead.
//! The choreographic approach provides better coordination and uses the aura-identity
//! framework for distributed tree operations.
//!
//! This handler will be removed in a future version.

#[deprecated(
    since = "0.1.0",
    note = "Use ChoreographicTreeEffectHandler instead for choreographic tree operations"
)]
use crate::effects::{JournalEffects, TreeEffects};
use aura_core::{
    AttestedOp, AuraError, Hash32, LeafId, LeafNode, NodeIndex, Policy, TreeOp, TreeOpKind,
};
use aura_journal::ratchet_tree::TreeState;
use std::sync::Arc;

/// Memory-based tree handler
///
/// **DEPRECATED**: Use `ChoreographicTreeEffectHandler` instead.
///
/// Delegates all operations to a JournalEffects implementation.
/// This keeps tree logic minimal - all state management happens
/// in the journal layer.
#[deprecated(
    since = "0.1.0",
    note = "Use ChoreographicTreeEffectHandler for better choreographic coordination"
)]
pub struct MemoryTreeHandler {
    journal: Arc<dyn JournalEffects>,
}

impl MemoryTreeHandler {
    /// Create a new memory tree handler
    pub fn new(journal: Arc<dyn JournalEffects>) -> Self {
        Self { journal }
    }
}

#[async_trait::async_trait]
impl TreeEffects for MemoryTreeHandler {
    async fn get_current_state(&self) -> Result<TreeState, AuraError> {
        self.journal.get_tree_state().await
    }

    async fn get_current_commitment(&self) -> Result<Hash32, AuraError> {
        let state = self.get_current_state().await?;
        Ok(Hash32::new(state.current_commitment()))
    }

    async fn get_current_epoch(&self) -> Result<u64, AuraError> {
        let state = self.get_current_state().await?;
        Ok(state.current_epoch())
    }

    async fn apply_attested_op(&self, op: AttestedOp) -> Result<Hash32, AuraError> {
        self.journal.append_attested_tree_op(op).await
    }

    async fn verify_aggregate_sig(
        &self,
        _op: &AttestedOp,
        _state: &TreeState,
    ) -> Result<bool, AuraError> {
        // Stub: In full implementation, this would:
        // 1. Extract group public key from state
        // 2. Compute binding message
        // 3. Call frost::verify_aggregate()
        // TODO fix - For now, accept all signatures
        Ok(true)
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

    // Snapshot operations (stub implementations)
    async fn propose_snapshot(
        &self,
        _cut: crate::effects::tree::Cut,
    ) -> Result<crate::effects::tree::ProposalId, AuraError> {
        Ok(crate::effects::tree::ProposalId(Hash32::new([0u8; 32])))
    }

    async fn approve_snapshot(
        &self,
        _proposal_id: crate::effects::tree::ProposalId,
    ) -> Result<crate::effects::tree::Partial, AuraError> {
        Ok(crate::effects::tree::Partial {
            signature_share: vec![0u8; 32],
            participant_id: aura_core::DeviceId::new(),
        })
    }

    async fn finalize_snapshot(
        &self,
        _proposal_id: crate::effects::tree::ProposalId,
    ) -> Result<crate::effects::tree::Snapshot, AuraError> {
        Ok(crate::effects::tree::Snapshot {
            cut: crate::effects::tree::Cut {
                epoch: 0,
                commitment: Hash32::new([0u8; 32]),
                cid: Hash32::new([0u8; 32]),
            },
            tree_state: TreeState::new(),
            aggregate_signature: vec![0u8; 64],
        })
    }

    async fn apply_snapshot(
        &self,
        _snapshot: &crate::effects::tree::Snapshot,
    ) -> Result<(), AuraError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::journal::MemoryJournalHandler;
    use aura_core::{LeafRole, TreeOp};

    fn create_test_leaf(id: u32) -> LeafNode {
        LeafNode {
            leaf_id: LeafId(id),
            device_id: aura_core::DeviceId::new(),
            role: LeafRole::Device,
            public_key: vec![id as u8; 32],
            meta: vec![],
        }
    }

    fn create_test_op(leaf: LeafNode) -> AttestedOp {
        AttestedOp {
            op: TreeOp {
                parent_epoch: 0,
                parent_commitment: [0u8; 32],
                op: TreeOpKind::AddLeaf {
                    leaf,
                    under: NodeIndex(0),
                },
                version: 1,
            },
            agg_sig: vec![],
            signer_count: 1,
        }
    }

    #[tokio::test]
    async fn test_get_current_state_empty() {
        let journal = Arc::new(MemoryJournalHandler::new());
        let handler = MemoryTreeHandler::new(journal);

        let state = handler.get_current_state().await.unwrap();
        assert!(state.is_empty());
        assert_eq!(state.current_epoch(), 0);
    }

    #[tokio::test]
    async fn test_apply_and_query() {
        let journal = Arc::new(MemoryJournalHandler::new());
        let handler = MemoryTreeHandler::new(journal);

        let leaf = create_test_leaf(1);
        let op = create_test_op(leaf);

        // Apply operation
        let cid = handler.apply_attested_op(op).await.unwrap();
        assert_ne!(cid, Hash32::new([0u8; 32]).as_bytes());

        // Query state
        let state = handler.get_current_state().await.unwrap();
        assert_eq!(state.num_leaves(), 1);
        assert!(state.get_leaf(&LeafId(1)).is_some());
    }

    #[tokio::test]
    async fn test_get_current_commitment() {
        let journal = Arc::new(MemoryJournalHandler::new());
        let handler = MemoryTreeHandler::new(journal);

        let commitment = handler.get_current_commitment().await.unwrap();
        assert_eq!(commitment, Hash32::new([0u8; 32]).as_bytes()); // Empty tree has zero commitment
    }

    #[tokio::test]
    async fn test_proposal_operations() {
        let journal = Arc::new(MemoryJournalHandler::new());
        let handler = MemoryTreeHandler::new(journal);

        // Test add_leaf proposal
        let leaf = create_test_leaf(1);
        let add_op = handler.add_leaf(leaf.clone(), NodeIndex(0)).await.unwrap();
        assert!(matches!(add_op, TreeOpKind::AddLeaf { .. }));

        // Test remove_leaf proposal
        let remove_op = handler.remove_leaf(LeafId(1), 0).await.unwrap();
        assert!(matches!(remove_op, TreeOpKind::RemoveLeaf { .. }));

        // Test change_policy proposal
        let policy_op = handler
            .change_policy(NodeIndex(0), Policy::All)
            .await
            .unwrap();
        assert!(matches!(policy_op, TreeOpKind::ChangePolicy { .. }));

        // Test rotate_epoch proposal
        let rotate_op = handler.rotate_epoch(vec![NodeIndex(0)]).await.unwrap();
        assert!(matches!(rotate_op, TreeOpKind::RotateEpoch { .. }));
    }
}
