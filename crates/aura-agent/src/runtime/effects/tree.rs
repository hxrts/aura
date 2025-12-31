use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::AuraError;
use aura_journal::commitment_tree::state::TreeState as JournalTreeState;

#[async_trait]
impl aura_protocol::effects::TreeEffects for AuraEffectSystem {
    async fn get_current_state(&self) -> Result<JournalTreeState, AuraError> {
        self.tree_handler.get_current_state().await
    }

    async fn get_current_commitment(&self) -> Result<aura_core::Hash32, AuraError> {
        self.tree_handler.get_current_commitment().await
    }

    async fn get_current_epoch(&self) -> Result<u64, AuraError> {
        self.tree_handler.get_current_epoch().await
    }

    async fn apply_attested_op(
        &self,
        op: aura_core::AttestedOp,
    ) -> Result<aura_core::Hash32, AuraError> {
        self.tree_handler.apply_attested_op(op).await
    }

    async fn verify_aggregate_sig(
        &self,
        op: &aura_core::AttestedOp,
        state: &JournalTreeState,
    ) -> Result<bool, AuraError> {
        self.tree_handler.verify_aggregate_sig(op, state).await
    }

    async fn add_leaf(
        &self,
        leaf: aura_core::LeafNode,
        under: aura_core::NodeIndex,
    ) -> Result<aura_core::TreeOpKind, AuraError> {
        self.tree_handler.add_leaf(leaf, under).await
    }

    async fn remove_leaf(
        &self,
        leaf_id: aura_core::LeafId,
        reason: u8,
    ) -> Result<aura_core::TreeOpKind, AuraError> {
        self.tree_handler.remove_leaf(leaf_id, reason).await
    }

    async fn change_policy(
        &self,
        node: aura_core::NodeIndex,
        policy: aura_core::Policy,
    ) -> Result<aura_core::TreeOpKind, AuraError> {
        self.tree_handler.change_policy(node, policy).await
    }

    async fn rotate_epoch(
        &self,
        affected: Vec<aura_core::NodeIndex>,
    ) -> Result<aura_core::TreeOpKind, AuraError> {
        self.tree_handler.rotate_epoch(affected).await
    }

    async fn propose_snapshot(
        &self,
        cut: aura_protocol::effects::tree::Cut,
    ) -> Result<aura_protocol::effects::tree::ProposalId, AuraError> {
        self.tree_handler.propose_snapshot(cut).await
    }

    async fn approve_snapshot(
        &self,
        proposal_id: aura_protocol::effects::tree::ProposalId,
    ) -> Result<aura_protocol::effects::tree::Partial, AuraError> {
        self.tree_handler.approve_snapshot(proposal_id).await
    }

    async fn finalize_snapshot(
        &self,
        proposal_id: aura_protocol::effects::tree::ProposalId,
    ) -> Result<aura_protocol::effects::tree::Snapshot, AuraError> {
        self.tree_handler.finalize_snapshot(proposal_id).await
    }

    async fn apply_snapshot(
        &self,
        snapshot: &aura_protocol::effects::tree::Snapshot,
    ) -> Result<(), AuraError> {
        self.tree_handler.apply_snapshot(snapshot).await
    }
}
