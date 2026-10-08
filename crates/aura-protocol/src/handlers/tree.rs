// Lock poisoning is fatal for this module - we prefer to panic than continue with corrupted state
#![allow(clippy::expect_used)]
// Uses std sync primitives for simple in-memory cache coordination.
#![allow(clippy::disallowed_types)]

//! Layer 4: Tree Handler Implementations
//!
//! Handlers for commitment tree operations.
//!
//! ## Handlers
//!
//! - **PersistentTreeHandler**: Production handler with storage persistence
//!
//! **Note**: Tree reduction and application logic lives in aura-journal (Layer 2),
//! enabling separation between domain CRDT operations and protocol-layer orchestration.
//!
//! ## Persistent Handler
//!
//! This handler persists the OpLog to storage, enabling tree state to survive
//! across restarts and supporting cross-device sync.
//!
//! ### Shared Storage
//!
//! Uses the same storage keys as `PersistentSyncHandler` via
//! `aura_journal::commitment_tree::storage`, ensuring both handlers operate on
//! the same source of truth.

use crate::effects::tree::{Cut, ProposalId, Snapshot, TreeEffects};
use async_trait::async_trait;
use aura_core::effects::storage::StorageEffects;
use aura_core::tree::{AttestedOp, LeafId, LeafNode, NodeIndex, Policy, TreeOpKind};
use aura_core::util::serialization::to_vec;
use aura_core::Epoch;
use aura_core::{hash, AuraError, Hash32};
use aura_journal::commitment_tree::reduce;
use aura_journal::commitment_tree::storage as tree_storage;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

const SNAPSHOT_SIGNATURE_DOMAIN: &[u8] = b"AURA_TREE_SNAPSHOT_V1";

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
    mutation_gate: async_lock::Mutex<()>,
}

/// Exclusive current-tree decision custody. It excludes local tree writers,
/// but does not authenticate the tree or establish remote freshness.
/// ```compile_fail
/// use aura_protocol::handlers::tree::TreeDecisionLease;
/// let _ = TreeDecisionLease { guard: () };
/// ```
pub struct TreeDecisionLease<'a> {
    owner: &'a PersistentTreeHandler,
    _guard: async_lock::MutexGuard<'a, ()>,
}

impl TreeDecisionLease<'_> {
    /// Storage comparison only: runtime callers must supply authenticated history.
    /// A held lease may publish its exact extension without reacquiring the gate.
    /// Divergence is rejected; any later history is returned intact for validation.
    pub async fn install_authenticated_extension(
        &self,
        admitted_prefix_len: usize,
        history: &[AttestedOp],
    ) -> Result<Vec<AttestedOp>, AuraError> {
        #[derive(Debug, thiserror::Error)]
        #[error("current tree is not an exact extension of the admitted enrollment history")]
        struct DivergentAuthenticatedExtension;
        self.owner.ensure_initialized().await?;
        let current = self.owner.export_ops().await?;
        if admitted_prefix_len == 0
            || history.len() <= admitted_prefix_len
            || current.len() < admitted_prefix_len
        {
            return Err(AuraError::PermissionDenied {
                message: "authenticated tree installation lacks complete original baseline".into(),
                source: Some(Arc::new(DivergentAuthenticatedExtension)),
            });
        }
        for (existing, expected) in current.iter().zip(history) {
            if tree_storage::op_hash(existing)? != tree_storage::op_hash(expected)? {
                return Err(AuraError::PermissionDenied {
                    message: "authenticated tree extension diverges from current evidence".into(),
                    source: Some(Arc::new(DivergentAuthenticatedExtension)),
                });
            }
        }
        if current.len() >= history.len() {
            return Ok(current);
        }
        self.owner.persist_candidate(history).await?;
        Ok(history.to_vec())
    }
}

impl PersistentTreeHandler {
    /// Hold the existing mutation gate over required reads and a decision.
    /// Read methods do not reacquire this gate. Calling a mutation while this
    /// lease is held would recursively wait and must be avoided.
    pub async fn lock_decision(&self) -> TreeDecisionLease<'_> {
        TreeDecisionLease {
            owner: self,
            _guard: self.mutation_gate.lock().await,
        }
    }
    /// Create a new persistent tree handler (synchronous, lazy loading).
    ///
    /// Operations are loaded from storage on first access to the tree state.
    pub fn new(storage: Arc<dyn StorageEffects>) -> Self {
        Self {
            storage,
            ops_cache: RwLock::new(Vec::new()),
            initialized: AtomicBool::new(false),
            mutation_gate: async_lock::Mutex::new(()),
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
            mutation_gate: async_lock::Mutex::new(()),
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
            .retrieve(tree_storage::TREE_OPS_INDEX_KEY)
            .await
            .map_err(|source| AuraError::Storage {
                message: "load canonical tree index".into(),
                source: Some(Arc::new(source)),
            })?;

        let op_hashes: Vec<[u8; 32]> = match index_bytes {
            Some(bytes) => tree_storage::deserialize_op_index(&bytes)?,
            None => Vec::new(), // No operations yet
        };

        // Load each operation by hash
        let mut ops = Vec::with_capacity(op_hashes.len());
        for op_hash in op_hashes {
            let key = tree_storage::op_key(op_hash);
            let op_bytes = storage
                .retrieve(&key)
                .await
                .map_err(|source| AuraError::Storage {
                    message: "load canonical tree operation".into(),
                    source: Some(Arc::new(source)),
                })?
                .ok_or_else(|| AuraError::storage(format!("Missing tree op: {key}")))?;

            let op: AttestedOp = tree_storage::deserialize_op(&op_bytes)?;
            if tree_storage::op_hash(&op)? != op_hash {
                return Err(AuraError::invalid(
                    "stored tree operation differs from canonical index digest",
                ));
            }
            ops.push(op);
        }

        Ok(ops)
    }

    /// Content blobs are written before the sole canonical index. Failure leaves
    /// the previous cache visible; an uncertain index outcome forces reread.
    async fn persist_candidate(&self, ops: &[AttestedOp]) -> Result<(), AuraError> {
        let mut hashes = Vec::with_capacity(ops.len());
        for op in ops {
            let digest = tree_storage::op_hash(op)?;
            let key = tree_storage::op_key(digest);
            self.storage
                .store(&key, tree_storage::serialize_op(op)?)
                .await
                .map_err(|source| AuraError::Storage {
                    message: "persist candidate tree operation".into(),
                    source: Some(Arc::new(source)),
                })?;
            hashes.push(digest);
        }
        let bytes = tree_storage::serialize_op_index(&hashes)?;
        if let Err(source) = self
            .storage
            .store(tree_storage::TREE_OPS_INDEX_KEY, bytes)
            .await
        {
            self.initialized.store(false, Ordering::Release);
            return Err(AuraError::Storage {
                message: "publish canonical tree index".into(),
                source: Some(Arc::new(source)),
            });
        }
        *self
            .ops_cache
            .write()
            .expect("PersistentTreeHandler lock poisoned") = ops.to_vec();
        Ok(())
    }
    /// Digest the exact ordered history for storage comparison, without granting authorization.
    pub fn ordered_ops_digest(ops: &[AttestedOp]) -> Result<[u8; 32], AuraError> {
        let bytes = to_vec(&ops).map_err(|source| AuraError::Serialization {
            message: "encode ordered tree operation digest".into(),
            source: Some(Arc::new(source)),
        })?;
        Ok(hash::hash(&bytes))
    }
    /// Storage comparison is not authorization. The caller supplies an
    /// independently authenticated baseline and its actual original snapshot.
    /// An existing complete baseline prefix retains any later local operations.
    pub async fn install_baseline_if_original(
        &self,
        original: [u8; 32],
        baseline: &[AttestedOp],
    ) -> Result<(), AuraError> {
        let _writer = self.mutation_gate.lock().await;
        self.ensure_initialized().await?;
        if baseline.is_empty() {
            return Err(AuraError::invalid(
                "authenticated enrollment baseline is empty",
            ));
        }
        let current = self.export_ops().await?;
        let mut prefix = current.len() >= baseline.len();
        if prefix {
            for (a, b) in current.iter().zip(baseline) {
                if tree_storage::op_hash(a)? != tree_storage::op_hash(b)? {
                    prefix = false;
                    break;
                }
            }
        }

        if prefix {
            return Ok(());
        }
        if Self::ordered_ops_digest(&current)? != original {
            return Err(AuraError::invalid("current tree is neither original installation state nor authenticated baseline prefix"));
        }
        self.persist_candidate(baseline).await
    }

    /// Export the current ordered OpLog.
    pub async fn export_ops(&self) -> Result<Vec<AttestedOp>, AuraError> {
        self.ensure_initialized().await?;
        let ops = self
            .ops_cache
            .read()
            .expect("PersistentTreeHandler lock poisoned")
            .clone();
        Ok(ops)
    }

    /// Persist an already verified operation batch, preserving existing order
    /// and appending only previously unseen operations.
    ///
    /// This storage primitive does not authenticate peer data. Runtime ingress
    /// must verify every operation against a trusted parent-epoch package and
    /// stage the complete transition before calling it.
    pub async fn import_ops(&self, imported_ops: &[AttestedOp]) -> Result<(), AuraError> {
        let _writer = self.mutation_gate.lock().await;
        self.ensure_initialized().await?;
        let mut candidate = self.export_ops().await?;
        let mut known = std::collections::BTreeSet::new();
        for op in &candidate {
            known.insert(tree_storage::op_hash(op)?);
        }
        let previous = candidate.len();
        for op in imported_ops {
            if known.insert(tree_storage::op_hash(op)?) {
                candidate.push(op.clone());
            }
        }
        if candidate.len() == previous {
            return Ok(());
        }
        self.persist_candidate(&candidate).await
    }

    /// Replace the local OpLog with an already authenticated baseline.
    ///
    /// Tree ops are stored by hash, not per authority, so a device that ran under
    /// a provisional authority before joining an account still holds that
    /// authority's ops. Callers must verify the account baseline and its
    /// ceremony-scoped trust anchor before discarding provisional history.
    pub async fn replace_ops(&self, ops: &[AttestedOp]) -> Result<(), AuraError> {
        let _writer = self.mutation_gate.lock().await;
        self.ensure_initialized().await?;
        self.persist_candidate(ops).await
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
        tree_storage::op_hash(op)
    }
}

#[derive(Serialize)]
struct SnapshotTranscript {
    proposal_id: ProposalId,
    cut: Cut,
    snapshot_epoch: Epoch,
    snapshot_commitment: Hash32,
    root_node: NodeIndex,
    root_policy: Policy,
    root_child_count: u32,
    required_signers: u16,
    participants: Vec<LeafId>,
}

fn snapshot_root_node(
    state: &aura_journal::commitment_tree::TreeState,
) -> Result<NodeIndex, AuraError> {
    state
        .root_node()
        .ok_or_else(|| AuraError::invalid("Snapshot tree state is missing a root branch"))
}

fn snapshot_transcript(
    proposal_id: ProposalId,
    snapshot: &Snapshot,
) -> Result<SnapshotTranscript, AuraError> {
    if snapshot.cut.epoch != snapshot.tree_state.epoch {
        return Err(AuraError::invalid(format!(
            "Snapshot epoch mismatch: cut={} state={}",
            u64::from(snapshot.cut.epoch),
            u64::from(snapshot.tree_state.epoch)
        )));
    }

    if snapshot.cut.commitment != Hash32(snapshot.tree_state.root_commitment) {
        return Err(AuraError::invalid(
            "Snapshot cut commitment does not match tree state root commitment",
        ));
    }

    let root_node = snapshot_root_node(&snapshot.tree_state)?;
    let root_policy = snapshot
        .tree_state
        .get_policy(&root_node)
        .copied()
        .ok_or_else(|| AuraError::invalid("Snapshot tree state is missing a root policy"))?;
    let root_child_count = u32::try_from(snapshot.tree_state.get_children(root_node).len())
        .map_err(|_| AuraError::invalid("Snapshot root child count exceeds u32"))?;
    let required_signers = root_policy
        .required_signers(
            usize::try_from(root_child_count)
                .map_err(|_| AuraError::invalid("Snapshot root child count exceeds usize"))?,
        )
        .map_err(|error| {
            AuraError::invalid(format!("Invalid root snapshot threshold policy: {error}"))
        })?;

    Ok(SnapshotTranscript {
        proposal_id,
        cut: snapshot.cut.clone(),
        snapshot_epoch: snapshot.tree_state.epoch,
        snapshot_commitment: Hash32(snapshot.tree_state.root_commitment),
        root_node,
        root_policy,
        root_child_count,
        required_signers,
        participants: snapshot.tree_state.list_leaf_ids(),
    })
}

fn snapshot_transcript_bytes(
    proposal_id: ProposalId,
    snapshot: &Snapshot,
) -> Result<Vec<u8>, AuraError> {
    let transcript = snapshot_transcript(proposal_id, snapshot)?;
    let mut bytes = SNAPSHOT_SIGNATURE_DOMAIN.to_vec();
    bytes.extend(to_vec(&transcript).map_err(|error| {
        AuraError::serialization(format!("serialize snapshot transcript: {error}"))
    })?);
    Ok(bytes)
}

fn verify_snapshot_signature(snapshot: &Snapshot) -> Result<(), AuraError> {
    use aura_core::crypto::tree_signing::frost_verify_aggregate;
    use frost_ed25519::VerifyingKey;

    let root_node = snapshot_root_node(&snapshot.tree_state)?;
    let signing_key = snapshot
        .tree_state
        .get_signing_key(&root_node)
        .ok_or_else(|| AuraError::invalid("Snapshot tree state is missing a root signing key"))?;
    let transcript = snapshot_transcript_bytes(snapshot.proposal_id, snapshot)?;
    let verifying_key = VerifyingKey::deserialize(*signing_key.group_key()).map_err(|error| {
        AuraError::crypto(format!("Invalid snapshot group public key: {error}"))
    })?;

    frost_verify_aggregate(&verifying_key, &transcript, &snapshot.aggregate_signature)
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl TreeEffects for PersistentTreeHandler {
    async fn list_attested_ops(&self) -> Result<Vec<AttestedOp>, AuraError> {
        self.export_ops().await
    }

    async fn get_current_state(
        &self,
    ) -> Result<aura_journal::commitment_tree::TreeState, AuraError> {
        self.reduce_state().await
    }

    async fn get_current_commitment(&self) -> Result<Hash32, AuraError> {
        let state = self.reduce_state().await?;
        Ok(Hash32(state.root_commitment))
    }

    async fn get_current_epoch(&self) -> Result<Epoch, AuraError> {
        let state = self.reduce_state().await?;
        Ok(state.epoch)
    }

    async fn apply_attested_op(&self, op: AttestedOp) -> Result<Hash32, AuraError> {
        let _writer = self.mutation_gate.lock().await;
        self.ensure_initialized().await?;
        let digest = Self::op_hash(&op)?;
        let mut candidate = self.export_ops().await?;
        let mut already = false;
        for existing in &candidate {
            if Self::op_hash(existing)? == digest {
                already = true;
                break;
            }
        }
        if !already {
            candidate.push(op);
            self.persist_candidate(&candidate).await?;
        }
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
        let threshold = policy.required_signers(child_count).map_err(|e| {
            AuraError::invalid(format!(
                "Invalid policy for branch {} (child_count={}): {e}",
                node.0, child_count
            ))
        })?;

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
        let bytes = aura_core::util::serialization::to_vec(&cut)
            .map_err(|e| AuraError::internal(format!("serialize cut: {e}")))?;
        Ok(ProposalId::new(hash::hash(&bytes)))
    }

    async fn apply_snapshot(&self, snapshot: &Snapshot) -> Result<(), AuraError> {
        let _writer = self.mutation_gate.lock().await;
        self.ensure_initialized().await?;
        verify_snapshot_signature(snapshot)?;
        self.persist_candidate(&[]).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::crypto::{CryptoExtendedEffects, KeyGenerationMethod};
    use aura_core::effects::storage::{StorageCoreEffects, StorageError, StorageExtendedEffects};
    use aura_core::tree::{BranchNode, BranchSigningKey, LeafId, TreeOp, TreeOpKind};
    use aura_effects::crypto::RealCryptoHandler;
    use std::collections::HashMap;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn current_decision_lease_excludes_actual_replacement() {
        let handler = PersistentTreeHandler::new(Arc::new(TestStorage::default()));
        let lease = handler.lock_decision().await;
        assert!(handler.export_ops().await.expect("owned read").is_empty());
        let replacement = handler.replace_ops(&[]);
        tokio::pin!(replacement);
        assert!(futures::poll!(&mut replacement).is_pending());
        drop(lease);
        replacement
            .await
            .expect("writer progresses after decision release");
    }

    #[derive(Debug, Default)]
    struct TestStorage {
        data: RwLock<HashMap<String, Vec<u8>>>,
        fail_index_before: AtomicBool,
        fail_index_after: AtomicBool,
    }

    #[async_trait]
    impl StorageCoreEffects for TestStorage {
        async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
            let fault = || StorageError::BackendFailure {
                operation: "fault canonical tree index".into(),
                source: AuraError::Storage {
                    message: "injected publication boundary".into(),
                    source: Some(Arc::new(std::io::Error::other("canonical index fault"))),
                },
            };
            if key == tree_storage::TREE_OPS_INDEX_KEY
                && self.fail_index_before.swap(false, Ordering::AcqRel)
            {
                return Err(fault());
            }
            self.data.write().await.insert(key.to_string(), value);
            if key == tree_storage::TREE_OPS_INDEX_KEY
                && self.fail_index_after.swap(false, Ordering::AcqRel)
            {
                return Err(fault());
            }

            Ok(())
        }

        async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
            Ok(self.data.read().await.get(key).cloned())
        }

        async fn remove(&self, key: &str) -> Result<bool, StorageError> {
            Ok(self.data.write().await.remove(key).is_some())
        }

        async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
            let data = self.data.read().await;
            Ok(match prefix {
                Some(prefix) => data
                    .keys()
                    .filter(|key| key.starts_with(prefix))
                    .cloned()
                    .collect(),
                None => data.keys().cloned().collect(),
            })
        }
    }

    #[async_trait]
    impl StorageExtendedEffects for TestStorage {}

    fn test_tree_state(group_public_key: [u8; 32]) -> aura_journal::commitment_tree::TreeState {
        let mut state = aura_journal::commitment_tree::TreeState::new();
        state.set_epoch(Epoch::new(7));
        state.set_root_commitment(hash::hash(b"snapshot-root"));
        state.add_branch_with_parent(
            BranchNode {
                node: NodeIndex(0),
                policy: Policy::threshold(2, 2).expect("valid threshold policy"),
                commitment: hash::hash(b"root-branch"),
            },
            None,
        );
        state.add_branch_with_parent(
            BranchNode {
                node: NodeIndex(1),
                policy: Policy::Any,
                commitment: hash::hash(b"child-branch"),
            },
            Some(NodeIndex(0)),
        );
        state.add_branch_with_parent(
            BranchNode {
                node: NodeIndex(2),
                policy: Policy::Any,
                commitment: hash::hash(b"child-branch-2"),
            },
            Some(NodeIndex(0)),
        );
        state.add_leaf_under(
            LeafNode::new_device(
                LeafId(1),
                aura_core::DeviceId::new_from_entropy([9u8; 32]),
                vec![7u8; 32],
            )
            .expect("leaf"),
            NodeIndex(1),
        );
        state.add_leaf_under(
            LeafNode::new_device(
                LeafId(2),
                aura_core::DeviceId::new_from_entropy([10u8; 32]),
                vec![8u8; 32],
            )
            .expect("leaf"),
            NodeIndex(2),
        );
        state.set_signing_key(
            NodeIndex(0),
            BranchSigningKey::new(group_public_key, Epoch::new(7)),
        );
        state
    }

    fn snapshot_without_signature(
        proposal_id: ProposalId,
        state: aura_journal::commitment_tree::TreeState,
    ) -> Snapshot {
        Snapshot {
            proposal_id,
            cut: Cut {
                epoch: state.epoch,
                commitment: Hash32(state.root_commitment),
                cid: Hash32(hash::hash(b"snapshot-cut")),
            },
            tree_state: state,
            aggregate_signature: Vec::new(),
        }
    }

    async fn signed_snapshot() -> Snapshot {
        let crypto = RealCryptoHandler::for_simulation_seed([11u8; 32]);
        let keys = crypto
            .generate_signing_keys_with(KeyGenerationMethod::DealerBased, 2, 2)
            .await
            .expect("signing keys");
        let public_key_package =
            frost_ed25519::keys::PublicKeyPackage::deserialize(&keys.public_key_package)
                .expect("public key package");
        let group_public_key = public_key_package.verifying_key().serialize();
        let proposal_id = ProposalId::new(hash::hash(b"signed-snapshot-proposal"));
        let mut snapshot =
            snapshot_without_signature(proposal_id, test_tree_state(group_public_key));
        let nonces_1 = crypto
            .frost_generate_nonces(&keys.key_packages[0])
            .await
            .expect("nonces");
        let nonces_2 = crypto
            .frost_generate_nonces(&keys.key_packages[1])
            .await
            .expect("nonces");
        let message = snapshot_transcript_bytes(snapshot.proposal_id, &snapshot)
            .expect("snapshot transcript");
        let signing_package = crypto
            .frost_create_public_signing_package(
                &message,
                &[nonces_1.public_commitment(), nonces_2.public_commitment()],
                &keys.public_key_package,
                2,
            )
            .await
            .expect("signing package");
        let log = aura_core::crypto::tree_signing::ProcessFrostNonceRetirement::default();
        let mut shares = Vec::new();
        for (key_package, nonces) in keys.key_packages.iter().zip([nonces_1, nonces_2]) {
            let nonces = nonces.retire(&log).await.expect("retire nonces");
            shares.push(
                crypto
                    .frost_sign_share_for_message(
                        &signing_package,
                        key_package,
                        nonces,
                        &message,
                        &keys.public_key_package,
                        2,
                    )
                    .await
                    .expect("sign share"),
            );
        }
        snapshot.aggregate_signature = crypto
            .frost_aggregate_signatures(&signing_package, &shares)
            .await
            .expect("aggregate signature");
        snapshot
    }

    #[tokio::test]
    async fn apply_snapshot_rejects_placeholder_proposal_id_bytes() {
        let handler = PersistentTreeHandler::new(Arc::new(TestStorage::default()));
        let mut snapshot = signed_snapshot().await;
        snapshot.aggregate_signature = snapshot.proposal_id.as_bytes().to_vec();

        let error = handler
            .apply_snapshot(&snapshot)
            .await
            .expect_err("proposal-id bytes are not a valid snapshot signature");
        assert!(error.to_string().contains("Invalid signature length"));
    }

    fn remove_leaf_op(leaf: u32) -> AttestedOp {
        AttestedOp {
            op: TreeOp {
                parent_epoch: Epoch::initial(),
                parent_commitment: [0u8; 32],
                op: TreeOpKind::RemoveLeaf {
                    leaf: LeafId(leaf),
                    reason: 0,
                },
                version: 1,
            },
            agg_sig: vec![],
            signer_count: 1,
        }
    }

    // Regression (work/8.md task 32): a joining device's provisional ops are
    // dropped, not merged, when it adopts the account's tree, including after
    // a reload from storage.
    #[tokio::test]
    async fn replace_ops_drops_provisional_history() {
        let storage = Arc::new(TestStorage::default());
        let handler = PersistentTreeHandler::new(storage.clone());
        handler.import_ops(&[remove_leaf_op(1)]).await.unwrap();

        handler.replace_ops(&[remove_leaf_op(2)]).await.unwrap();

        assert_eq!(handler.export_ops().await.unwrap(), vec![remove_leaf_op(2)]);
        let reloaded = PersistentTreeHandler::new(storage);
        assert_eq!(
            reloaded.export_ops().await.unwrap(),
            vec![remove_leaf_op(2)]
        );
    }

    #[tokio::test]
    async fn failed_tree_replacement_keeps_complete_original_before_index() -> Result<(), AuraError>
    {
        let storage = Arc::new(TestStorage::default());
        let handler = PersistentTreeHandler::new(storage.clone());
        let old = vec![remove_leaf_op(1)];
        handler.import_ops(&old).await?;
        storage.fail_index_before.store(true, Ordering::Release);
        assert!(handler.replace_ops(&[remove_leaf_op(2)]).await.is_err());
        assert_eq!(handler.export_ops().await?, old);
        assert_eq!(PersistentTreeHandler::new(storage).export_ops().await?, old);
        Ok(())
    }
    #[tokio::test]
    async fn uncertain_index_outcome_reloads_complete_canonical_publication(
    ) -> Result<(), AuraError> {
        let storage = Arc::new(TestStorage::default());
        let handler = PersistentTreeHandler::new(storage.clone());
        handler.import_ops(&[remove_leaf_op(1)]).await?;
        storage.fail_index_after.store(true, Ordering::Release);
        let replacement = vec![remove_leaf_op(2)];
        assert!(handler.replace_ops(&replacement).await.is_err());
        assert_eq!(handler.export_ops().await?, replacement);
        assert_eq!(
            PersistentTreeHandler::new(storage).export_ops().await?,
            replacement
        );
        Ok(())
    }
    #[tokio::test]
    async fn original_baseline_replay_preserves_later_operations() -> Result<(), AuraError> {
        let storage = Arc::new(TestStorage::default());
        let handler = PersistentTreeHandler::new(storage);
        handler.import_ops(&[remove_leaf_op(1)]).await?;
        let original = PersistentTreeHandler::ordered_ops_digest(&handler.export_ops().await?)?;
        let baseline = vec![remove_leaf_op(2)];
        handler
            .install_baseline_if_original(original, &baseline)
            .await?;
        handler.import_ops(&[remove_leaf_op(3)]).await?;
        handler
            .install_baseline_if_original(original, &baseline)
            .await?;
        assert_eq!(
            handler.export_ops().await?,
            vec![remove_leaf_op(2), remove_leaf_op(3)]
        );
        assert!(handler
            .install_baseline_if_original(original, &[remove_leaf_op(4)])
            .await
            .is_err());
        assert_eq!(
            handler.export_ops().await?,
            vec![remove_leaf_op(2), remove_leaf_op(3)]
        );
        Ok(())
    }

    #[tokio::test]
    async fn apply_snapshot_accepts_valid_signed_snapshot() {
        let handler = PersistentTreeHandler::new(Arc::new(TestStorage::default()));
        let snapshot = signed_snapshot().await;

        handler
            .apply_snapshot(&snapshot)
            .await
            .expect("valid signed snapshot should apply");
    }
    /// Tests the held storage owner only; crypto provenance is supplied by the
    /// runtime's sealed committed transition, never by these unsigned fixtures.
    #[tokio::test]
    async fn held_extension_preserves_later_evidence_and_rejects_divergence(
    ) -> Result<(), AuraError> {
        let storage = Arc::new(TestStorage::default());
        let handler = PersistentTreeHandler::new(storage.clone());
        let baseline = remove_leaf_op(1);
        let committed = vec![baseline.clone(), remove_leaf_op(2)];
        handler.import_ops(&[baseline]).await?;
        {
            let lease = handler.lock_decision().await;
            assert_eq!(
                lease.install_authenticated_extension(1, &committed).await?,
                committed
            );
            assert_eq!(
                lease.install_authenticated_extension(1, &committed).await?,
                committed
            );
        }
        handler.import_ops(&[remove_leaf_op(3)]).await?;
        let expected = handler.export_ops().await?;
        {
            let lease = handler.lock_decision().await;
            assert_eq!(
                lease.install_authenticated_extension(1, &committed).await?,
                expected
            );
            assert!(lease
                .install_authenticated_extension(1, &[remove_leaf_op(1), remove_leaf_op(4)])
                .await
                .is_err());
        }
        assert_eq!(handler.export_ops().await?, expected);
        let reloaded = PersistentTreeHandler::new(storage);
        assert_eq!(reloaded.export_ops().await?, expected);
        Ok(())
    }
}
