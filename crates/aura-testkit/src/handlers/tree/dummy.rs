//! Dummy tree handler used for wiring tests and composite handlers.
//!
//! The real commitment tree logic lives in higher layers. For unit tests and
//! wiring checks we still need a handler with deterministic, stateful behavior
//! so callers can exercise the full trait surface without tripping over
//! placeholder failures.

use async_lock::Mutex;
use async_trait::async_trait;
use aura_core::effects::{Cut, Partial, ProposalId, Snapshot, TreeOperationEffects};
use aura_core::util::serialization::to_vec;
use aura_core::{
    AttestedOp, AuraError, DeviceId, Epoch, Hash32, LeafId, LeafNode, NodeIndex, Policy,
};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Default)]
struct DummyTreeState {
    current_state: Vec<u8>,
    current_commitment: Hash32,
    current_epoch: Epoch,
    snapshots: HashMap<ProposalId, Snapshot>,
}

impl DummyTreeState {
    fn rebuild_commitment(&mut self) {
        self.current_commitment = Hash32::from_bytes(&self.current_state);
    }
}

fn encode_value<T: serde::Serialize>(value: &T, what: &str) -> Result<Vec<u8>, AuraError> {
    to_vec(value).map_err(|error| {
        AuraError::internal(format!("DummyTreeHandler failed to encode {what}: {error}"))
    })
}

/// Deterministic in-memory implementation of [`TreeOperationEffects`].
#[derive(Debug, Clone, Default)]
pub struct DummyTreeHandler {
    state: Arc<Mutex<DummyTreeState>>,
}

impl DummyTreeHandler {
    /// Create a new dummy handler.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(DummyTreeState::default())),
        }
    }
}

#[async_trait]
impl TreeOperationEffects for DummyTreeHandler {
    async fn get_current_state(&self) -> Result<Vec<u8>, AuraError> {
        Ok(self.state.lock().await.current_state.clone())
    }

    async fn get_current_commitment(&self) -> Result<Hash32, AuraError> {
        Ok(self.state.lock().await.current_commitment)
    }

    async fn get_current_epoch(&self) -> Result<aura_core::Epoch, AuraError> {
        Ok(self.state.lock().await.current_epoch)
    }

    async fn apply_attested_op(&self, op: AttestedOp) -> Result<Hash32, AuraError> {
        let encoded = encode_value(&op, "attested operation")?;
        let mut state = self.state.lock().await;
        state.current_state = encoded;
        state.current_epoch = op.op.parent_epoch.next()?;
        state.rebuild_commitment();
        Ok(state.current_commitment)
    }

    async fn verify_aggregate_sig(
        &self,
        op: &AttestedOp,
        _state: &[u8],
    ) -> Result<bool, AuraError> {
        Ok(!op.agg_sig.is_empty() && op.signer_count > 0)
    }

    async fn add_leaf(&self, leaf: LeafNode, under: NodeIndex) -> Result<Vec<u8>, AuraError> {
        encode_value(
            &aura_core::tree::TreeOpKind::AddLeaf { leaf, under },
            "add leaf proposal",
        )
    }

    async fn remove_leaf(&self, leaf_id: LeafId, reason: u8) -> Result<Vec<u8>, AuraError> {
        encode_value(
            &aura_core::tree::TreeOpKind::RemoveLeaf {
                leaf: leaf_id,
                reason,
            },
            "remove leaf proposal",
        )
    }

    async fn change_policy(&self, node: NodeIndex, policy: Policy) -> Result<Vec<u8>, AuraError> {
        encode_value(
            &aura_core::tree::TreeOpKind::ChangePolicy {
                node,
                new_policy: policy,
            },
            "change policy proposal",
        )
    }

    async fn rotate_epoch(&self, affected: Vec<NodeIndex>) -> Result<Vec<u8>, AuraError> {
        encode_value(
            &aura_core::tree::TreeOpKind::RotateEpoch { affected },
            "rotate epoch proposal",
        )
    }

    async fn propose_snapshot(&self, cut: Cut) -> Result<ProposalId, AuraError> {
        let proposal_id = ProposalId::from_hash32(Hash32::from_bytes(cut.commitment.as_bytes()));
        let snapshot = Snapshot {
            cut: cut.clone(),
            tree_state: self.state.lock().await.current_state.clone(),
            aggregate_signature: Vec::new(),
        };
        self.state
            .lock()
            .await
            .snapshots
            .insert(proposal_id, snapshot);
        Ok(proposal_id)
    }

    async fn approve_snapshot(&self, proposal_id: ProposalId) -> Result<Partial, AuraError> {
        let state = self.state.lock().await;
        if !state.snapshots.contains_key(&proposal_id) {
            return Err(AuraError::not_found(format!(
                "DummyTreeHandler snapshot proposal not found: {proposal_id}"
            )));
        }
        Ok(Partial {
            signature_share: proposal_id.as_bytes().to_vec(),
            participant_id: DeviceId::new_from_entropy([0u8; 32]),
        })
    }

    async fn finalize_snapshot(&self, proposal_id: ProposalId) -> Result<Snapshot, AuraError> {
        self.state
            .lock()
            .await
            .snapshots
            .get(&proposal_id)
            .cloned()
            .ok_or_else(|| {
                AuraError::not_found(format!(
                    "DummyTreeHandler snapshot proposal not found: {proposal_id}"
                ))
            })
    }

    async fn apply_snapshot(&self, snapshot: &Snapshot) -> Result<(), AuraError> {
        let mut state = self.state.lock().await;
        state.current_state = snapshot.tree_state.clone();
        state.current_epoch = snapshot.cut.epoch;
        state.current_commitment = snapshot.cut.commitment;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dummy_tree_handler_tracks_applied_state() {
        let handler = DummyTreeHandler::new();
        let cut = Cut {
            epoch: Epoch::new(7),
            commitment: Hash32::from_bytes(b"cut"),
            cid: Hash32::from_bytes(b"cid"),
        };
        let proposal = handler
            .propose_snapshot(cut.clone())
            .await
            .expect("snapshot proposal should succeed");
        let snapshot = handler
            .finalize_snapshot(proposal)
            .await
            .expect("snapshot finalization should succeed");
        handler
            .approve_snapshot(proposal)
            .await
            .expect("snapshot approval should succeed");
        handler
            .apply_snapshot(&snapshot)
            .await
            .expect("snapshot apply should succeed");

        assert_eq!(
            handler.get_current_epoch().await.expect("epoch"),
            Epoch::new(7)
        );
        assert_eq!(
            handler.get_current_commitment().await.expect("commitment"),
            cut.commitment
        );
    }
}
