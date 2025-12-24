// Lock poisoning is fatal for this module - we prefer to panic than continue with corrupted state
#![allow(clippy::expect_used)]

use crate::effects::tree::{Cut, Partial, ProposalId, Snapshot, TreeEffects};
use async_trait::async_trait;
use aura_core::hash;
use aura_core::tree::{AttestedOp, LeafId, LeafNode, NodeIndex, Policy, TreeOpKind};
use aura_core::{AuraError, Hash32};
use aura_journal::commitment_tree::reduce;
use bincode;
use std::sync::{Arc, RwLock};

/// In-memory commitment tree handler backed by an OpLog buffer.
///
/// This is the **current production implementation** used by `AuraEffectSystem`.
/// It provides deterministic, side-effect-free tree operations by reducing an
/// in-memory OpLog to derive current tree state.
///
/// The in-memory approach works well for single-device scenarios. A persistent
/// journal-backed handler will eventually replace this for cross-device sync.
#[derive(Clone)]
pub struct InMemoryTreeHandler {
    oplog: Arc<RwLock<Vec<AttestedOp>>>,
}

impl InMemoryTreeHandler {
    pub fn new(oplog: Arc<RwLock<Vec<AttestedOp>>>) -> Self {
        Self { oplog }
    }

    fn ops(&self) -> Arc<RwLock<Vec<AttestedOp>>> {
        self.oplog.clone()
    }

    fn reduce_state(&self) -> Result<aura_journal::commitment_tree::state::TreeState, AuraError> {
        let ops = self
            .oplog
            .read()
            .expect("InMemoryTreeHandler lock poisoned");
        reduce(&ops).map_err(|e| AuraError::internal(format!("tree reduce failed: {e}")))
    }
}

#[async_trait]
impl TreeEffects for InMemoryTreeHandler {
    async fn get_current_state(
        &self,
    ) -> Result<aura_journal::commitment_tree::TreeState, AuraError> {
        self.reduce_state()
    }

    async fn get_current_commitment(&self) -> Result<Hash32, AuraError> {
        let state = self.reduce_state()?;
        Ok(Hash32(state.root_commitment))
    }

    async fn get_current_epoch(&self) -> Result<u64, AuraError> {
        let state = self.reduce_state()?;
        Ok(state.epoch)
    }

    async fn apply_attested_op(&self, op: AttestedOp) -> Result<Hash32, AuraError> {
        let oplog = self.ops();
        {
            let mut ops = oplog.write().expect("InMemoryTreeHandler lock poisoned");
            // Deduplicate by hash to avoid re-application
            let op_hash =
                hash::hash(&bincode::serialize(&op).map_err(|e| {
                    AuraError::internal(format!("hash serialize attested op: {e}"))
                })?);
            let already = ops.iter().any(|existing| {
                hash::hash(&bincode::serialize(existing).unwrap_or_default()) == op_hash
            });
            if !already {
                ops.push(op);
            }
        }

        let state = self.reduce_state()?;
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
        let state = self.reduce_state()?;
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
        let mut ops = self
            .oplog
            .write()
            .expect("InMemoryTreeHandler lock poisoned");
        ops.clear();
        // Snapshot application replaces history; we store no additional ops for the in-memory handler.
        let _ = snapshot;
        Ok(())
    }
}
