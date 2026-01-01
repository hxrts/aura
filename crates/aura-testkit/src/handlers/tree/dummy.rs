//! Dummy tree handler used for wiring tests and composite handlers.
//!
//! The real commitment tree logic lives in higher layers. For unit tests and
//! wiring checks we only need a handler that satisfies the trait signatures
//! without performing any work.

use async_trait::async_trait;
use aura_core::{AttestedOp, AuraError, Hash32, LeafId, LeafNode, NodeIndex, Policy};

use aura_core::effects::{Cut, Partial, ProposalId, Snapshot, TreeOperationEffects};

fn not_implemented(method: &str) -> AuraError {
    AuraError::internal(format!("DummyTreeHandler::{method} is not implemented"))
}

/// No-op implementation of [`TreeOperationEffects`].
#[derive(Debug, Clone, Default)]
pub struct DummyTreeHandler;

impl DummyTreeHandler {
    /// Create a new dummy handler.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TreeOperationEffects for DummyTreeHandler {
    async fn get_current_state(&self) -> Result<Vec<u8>, AuraError> {
        Err(not_implemented("get_current_state"))
    }

    async fn get_current_commitment(&self) -> Result<Hash32, AuraError> {
        Err(not_implemented("get_current_commitment"))
    }

    async fn get_current_epoch(&self) -> Result<aura_core::Epoch, AuraError> {
        Err(not_implemented("get_current_epoch"))
    }

    async fn apply_attested_op(&self, _op: AttestedOp) -> Result<Hash32, AuraError> {
        Err(not_implemented("apply_attested_op"))
    }

    async fn verify_aggregate_sig(
        &self,
        _op: &AttestedOp,
        _state: &[u8],
    ) -> Result<bool, AuraError> {
        Err(not_implemented("verify_aggregate_sig"))
    }

    async fn add_leaf(&self, _leaf: LeafNode, _under: NodeIndex) -> Result<Vec<u8>, AuraError> {
        Err(not_implemented("add_leaf"))
    }

    async fn remove_leaf(&self, _leaf_id: LeafId, _reason: u8) -> Result<Vec<u8>, AuraError> {
        Err(not_implemented("remove_leaf"))
    }

    async fn change_policy(&self, _node: NodeIndex, _policy: Policy) -> Result<Vec<u8>, AuraError> {
        Err(not_implemented("change_policy"))
    }

    async fn rotate_epoch(&self, _affected: Vec<NodeIndex>) -> Result<Vec<u8>, AuraError> {
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
