//! Stub tree handler for testing and composition
//!
//! A minimal implementation of TreeEffects that returns errors for all operations.
//! Used as a placeholder when tree operations are not needed or will be replaced later.

use async_trait::async_trait;
use aura_core::{
    tree::{AttestedOp, LeafId, LeafNode, NodeIndex, Policy, TreeOpKind},
    AuraError, Hash32,
};
use aura_journal::ratchet_tree::TreeState;

use crate::effects::tree::{Cut, Partial, ProposalId, Snapshot, TreeEffects};

/// Dummy tree handler that returns errors for all operations
///
/// This is a stub implementation used for:
/// - Testing handler composition without full tree support
/// - Placeholder in production handlers when tree operations are deferred
/// - Development iteration before full tree implementation
#[derive(Debug, Clone)]
pub struct DummyTreeHandler;

impl DummyTreeHandler {
    /// Create a new dummy tree handler
    pub fn new() -> Self {
        DummyTreeHandler
    }
}

impl Default for DummyTreeHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TreeEffects for DummyTreeHandler {
    async fn get_current_state(&self) -> Result<TreeState, AuraError> {
        Err(AuraError::Internal {
            message: "Tree operations not available in dummy handler".to_string(),
        })
    }

    async fn get_current_commitment(&self) -> Result<Hash32, AuraError> {
        Err(AuraError::Internal {
            message: "Tree operations not available in dummy handler".to_string(),
        })
    }

    async fn get_current_epoch(&self) -> Result<u64, AuraError> {
        Err(AuraError::Internal {
            message: "Tree operations not available in dummy handler".to_string(),
        })
    }

    async fn apply_attested_op(&self, _op: AttestedOp) -> Result<Hash32, AuraError> {
        Err(AuraError::Internal {
            message: "Tree operations not available in dummy handler".to_string(),
        })
    }

    async fn verify_aggregate_sig(
        &self,
        _op: &AttestedOp,
        _state: &TreeState,
    ) -> Result<bool, AuraError> {
        Err(AuraError::Internal {
            message: "Tree operations not available in dummy handler".to_string(),
        })
    }

    async fn add_leaf(&self, _leaf: LeafNode, _under: NodeIndex) -> Result<TreeOpKind, AuraError> {
        Err(AuraError::Internal {
            message: "Tree operations not available in dummy handler".to_string(),
        })
    }

    async fn remove_leaf(&self, _leaf_id: LeafId, _reason: u8) -> Result<TreeOpKind, AuraError> {
        Err(AuraError::Internal {
            message: "Tree operations not available in dummy handler".to_string(),
        })
    }

    async fn change_policy(
        &self,
        _node: NodeIndex,
        _new_policy: Policy,
    ) -> Result<TreeOpKind, AuraError> {
        Err(AuraError::Internal {
            message: "Tree operations not available in dummy handler".to_string(),
        })
    }

    async fn rotate_epoch(&self, _affected: Vec<NodeIndex>) -> Result<TreeOpKind, AuraError> {
        Err(AuraError::Internal {
            message: "Tree operations not available in dummy handler".to_string(),
        })
    }

    async fn propose_snapshot(&self, _cut: Cut) -> Result<ProposalId, AuraError> {
        Err(AuraError::Internal {
            message: "Tree operations not available in dummy handler".to_string(),
        })
    }

    async fn approve_snapshot(&self, _proposal_id: ProposalId) -> Result<Partial, AuraError> {
        Err(AuraError::Internal {
            message: "Tree operations not available in dummy handler".to_string(),
        })
    }

    async fn finalize_snapshot(&self, _proposal_id: ProposalId) -> Result<Snapshot, AuraError> {
        Err(AuraError::Internal {
            message: "Tree operations not available in dummy handler".to_string(),
        })
    }

    async fn apply_snapshot(&self, _snapshot: &Snapshot) -> Result<(), AuraError> {
        Err(AuraError::Internal {
            message: "Tree operations not available in dummy handler".to_string(),
        })
    }
}
