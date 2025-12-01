//! Dummy tree handler used for wiring tests and composite handlers.
//!
//! The real commitment tree logic lives in higher layers. For unit tests and
//! Minimal handler used for contexts where full tree logic is unnecessary (tests/sim).
//! signatures without performing any work.

use async_trait::async_trait;
use aura_core::{AttestedOp, AuraError, Hash32, LeafId, LeafNode, NodeIndex, Policy, TreeOpKind};
use aura_journal::commitment_tree::TreeState;

use crate::effects::tree::{Cut, Partial, ProposalId, Snapshot, TreeEffects};

fn not_implemented(method: &str) -> AuraError {
    AuraError::internal(format!("DummyTreeHandler::{method} is not implemented"))
}

/// No-op implementation of [`TreeEffects`].
#[derive(Debug, Clone, Default)]
pub struct DummyTreeHandler;

impl DummyTreeHandler {
    /// Create a new dummy handler.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TreeEffects for DummyTreeHandler {
    async fn get_current_state(&self) -> Result<TreeState, AuraError> {
        Err(not_implemented("get_current_state"))
    }

    async fn get_current_commitment(&self) -> Result<Hash32, AuraError> {
        Err(not_implemented("get_current_commitment"))
    }

    async fn get_current_epoch(&self) -> Result<u64, AuraError> {
        Err(not_implemented("get_current_epoch"))
    }

    async fn apply_attested_op(&self, _op: AttestedOp) -> Result<Hash32, AuraError> {
        Err(not_implemented("apply_attested_op"))
    }

    async fn verify_aggregate_sig(
        &self,
        _op: &AttestedOp,
        _state: &TreeState,
    ) -> Result<bool, AuraError> {
        Err(not_implemented("verify_aggregate_sig"))
    }

    async fn add_leaf(&self, _leaf: LeafNode, _under: NodeIndex) -> Result<TreeOpKind, AuraError> {
        Err(not_implemented("add_leaf"))
    }

    async fn remove_leaf(&self, _leaf_id: LeafId, _reason: u8) -> Result<TreeOpKind, AuraError> {
        Err(not_implemented("remove_leaf"))
    }

    async fn change_policy(
        &self,
        _node: NodeIndex,
        _policy: Policy,
    ) -> Result<TreeOpKind, AuraError> {
        Err(not_implemented("change_policy"))
    }

    async fn rotate_epoch(&self, _affected: Vec<NodeIndex>) -> Result<TreeOpKind, AuraError> {
        Err(not_implemented("rotate_epoch"))
    }

    async fn propose_snapshot(&self, _cut: Cut) -> Result<ProposalId, AuraError> {
        Err(not_implemented("propose_snapshot"))
    }

    async fn approve_snapshot(&self, _proposal_id: ProposalId) -> Result<Partial, AuraError> {
        Err(not_implemented("approve_snapshot"))
    }

    async fn finalize_snapshot(&self, _proposal_id: ProposalId) -> Result<Snapshot, AuraError> {
        Err(not_implemented("finalize_snapshot"))
    }

    async fn apply_snapshot(&self, _snapshot: &Snapshot) -> Result<(), AuraError> {
        Err(not_implemented("apply_snapshot"))
    }
}
