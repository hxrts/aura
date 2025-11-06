//! Journal CRDT Implementation
//!
//! Implements the JournalMap grow-only CRDT that combines:
//! - "op" namespace: Authoritative TreeOp records (one per epoch)
//! - "intent" namespace: Staging area with observed-remove semantics
//!
//! The journal provides eventual consistency across replicas while maintaining
//! authentication integrity through threshold signatures.

use super::intent::{Intent, IntentId, IntentStatus};
use super::tree_op::{Epoch, TreeOpRecord};
use super::journal_types::{JournalError, JournalStats};
use crate::tree::{Commitment, RatchetTree};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Journal CRDT ledger
///
/// Combines authoritative tree operations with a staging intent pool.
/// Uses join-semilattice semantics for convergence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalMap {
    /// "op" namespace: grow-only map of tree operations by epoch
    /// Key property: one TreeOp per epoch, conflicts resolved by commitment hash
    ops: BTreeMap<Epoch, TreeOpRecord>,

    /// "intent" namespace: observed-remove set of pending intents
    intents: BTreeMap<IntentId, Intent>,

    /// Tombstones for removed intents (observed-remove semantics)
    intent_tombstones: BTreeSet<IntentId>,

    /// Current tree state (cached, rebuilt from ops on demand)
    tree_cache: Option<RatchetTree>,
}

impl JournalMap {
    /// Create a new empty journal map
    pub fn new() -> Self {
        Self {
            ops: BTreeMap::new(),
            intents: BTreeMap::new(),
            intent_tombstones: BTreeSet::new(),
            tree_cache: None,
        }
    }

    /// Get the number of tree operations
    pub fn num_ops(&self) -> usize {
        self.ops.len()
    }

    /// Get the number of pending intents
    pub fn num_intents(&self) -> usize {
        self.intents.len()
    }

    /// Get a tree operation by epoch
    pub fn get_op(&self, epoch: Epoch) -> Option<&TreeOpRecord> {
        self.ops.get(&epoch)
    }

    /// Get all tree operations in epoch order
    pub fn ops_ordered(&self) -> Vec<&TreeOpRecord> {
        self.ops.values().collect()
    }

    /// Get the latest epoch
    pub fn latest_epoch(&self) -> Option<Epoch> {
        self.ops.keys().max().copied()
    }

    /// Get an intent by ID
    pub fn get_intent(&self, id: &IntentId) -> Option<&Intent> {
        self.intents.get(id)
    }

    /// Get intent status
    pub fn get_intent_status(&self, id: &IntentId) -> IntentStatus {
        if self.intent_tombstones.contains(id) {
            IntentStatus::Completed
        } else if self.intents.contains_key(id) {
            IntentStatus::Pending
        } else {
            IntentStatus::Failed
        }
    }

    /// List all pending intents
    pub fn list_pending_intents(&self) -> Vec<&Intent> {
        self.intents.values().collect()
    }

    /// Append a tree operation
    ///
    /// Merge rule: For each epoch, keep the op with the highest commitment hash.
    /// This ensures deterministic conflict resolution.
    pub fn append_tree_op(&mut self, op: TreeOpRecord) -> Result<(), JournalError> {
        let epoch = op.epoch;

        // Verify threshold signature
        if !op.verify_threshold() {
            return Err(JournalError::InvalidSignature {
                epoch,
                reason: "Threshold not met".to_string(),
            });
        }

        // Check if we already have an op for this epoch
        if let Some(existing) = self.ops.get(&epoch) {
            // Conflict resolution: keep the one with higher commitment hash
            if let (Some(existing_root), Some(new_root)) =
                (existing.root_commitment(), op.root_commitment())
            {
                if new_root > existing_root {
                    // New op wins
                    self.ops.insert(epoch, op);
                    self.invalidate_tree_cache();
                }
                // Otherwise keep existing
            }
        } else {
            // No conflict, just insert
            self.ops.insert(epoch, op);
            self.invalidate_tree_cache();
        }

        Ok(())
    }

    /// Submit an intent to the pool
    ///
    /// Uses observed-remove set semantics: add always wins over remove.
    pub fn submit_intent(&mut self, intent: Intent) -> Result<IntentId, JournalError> {
        let id = intent.intent_id;

        // Check if tombstoned
        if self.intent_tombstones.contains(&id) {
            return Err(JournalError::IntentTombstoned(id));
        }

        self.intents.insert(id, intent);
        Ok(id)
    }

    /// Tombstone an intent (mark as completed)
    ///
    /// This implements the "remove" part of observed-remove set.
    pub fn tombstone_intent(&mut self, id: IntentId) -> Result<(), JournalError> {
        // Add to tombstones
        self.intent_tombstones.insert(id);

        // Remove from active intents
        self.intents.remove(&id);

        Ok(())
    }

    /// Merge another journal map into this one (join operation)
    ///
    /// This implements the CRDT join-semilattice merge.
    pub fn merge(&mut self, other: &JournalMap) {
        // Merge ops namespace
        for (epoch, op) in &other.ops {
            if let Some(existing) = self.ops.get(epoch) {
                // Resolve conflict by commitment hash
                if let (Some(existing_root), Some(other_root)) =
                    (existing.root_commitment(), op.root_commitment())
                {
                    if other_root > existing_root {
                        self.ops.insert(*epoch, op.clone());
                    }
                }
            } else {
                self.ops.insert(*epoch, op.clone());
            }
        }

        // Merge intents namespace (OR-Set semantics)
        // Add wins: if an intent is in other and not tombstoned here, add it
        for (id, intent) in &other.intents {
            if !self.intent_tombstones.contains(id) {
                self.intents.insert(*id, intent.clone());
            }
        }

        // Merge tombstones
        for id in &other.intent_tombstones {
            self.intent_tombstones.insert(*id);
            self.intents.remove(id); // Remove if present
        }

        self.invalidate_tree_cache();
    }

    /// Replay ops to reconstruct the ratchet tree
    ///
    /// Rebuilds the tree state by applying ops in epoch order.
    pub fn replay_to_tree(&self) -> Result<RatchetTree, JournalError> {
        let mut tree = RatchetTree::new();

        // Apply ops in epoch order
        for op_record in self.ops.values() {
            match &op_record.op {
                super::tree_op::TreeOp::AddLeaf {
                    leaf_node,
                    affected_path: _,
                } => {
                    tree.add_leaf(leaf_node.clone()).map_err(|e| {
                        JournalError::TreeOperationFailed {
                            epoch: op_record.epoch,
                            reason: e.to_string(),
                        }
                    })?;
                }
                super::tree_op::TreeOp::RemoveLeaf {
                    leaf_index,
                    affected_path: _,
                } => {
                    tree.remove_leaf(*leaf_index).map_err(|e| {
                        JournalError::TreeOperationFailed {
                            epoch: op_record.epoch,
                            reason: e.to_string(),
                        }
                    })?;
                }
                super::tree_op::TreeOp::RotatePath {
                    leaf_index,
                    affected_path: _,
                } => {
                    tree.rotate_path(*leaf_index).map_err(|e| {
                        JournalError::TreeOperationFailed {
                            epoch: op_record.epoch,
                            reason: e.to_string(),
                        }
                    })?;
                }
                super::tree_op::TreeOp::RefreshPolicy {
                    node_index,
                    new_policy,
                    affected_path: _,
                } => {
                    // Update branch policy
                    if let Some(branch) = tree.branches.get_mut(node_index) {
                        branch.policy = *new_policy;
                    }
                }
                super::tree_op::TreeOp::EpochBump { .. } => {
                    // Epoch bump doesn't change tree structure
                    tree.increment_epoch();
                }
                super::tree_op::TreeOp::RecoveryGrant { .. } => {
                    // Recovery grant doesn't change tree structure
                }
            }

            // Verify epoch matches
            if tree.epoch != op_record.epoch {
                // Sync epochs
                while tree.epoch < op_record.epoch {
                    tree.increment_epoch();
                }
            }
        }

        Ok(tree)
    }

    /// Get the cached tree or rebuild it
    pub fn get_tree(&mut self) -> Result<&RatchetTree, JournalError> {
        if self.tree_cache.is_none() {
            self.tree_cache = Some(self.replay_to_tree()?);
        }
        Ok(self.tree_cache.as_ref().unwrap())
    }

    /// Get the current root commitment from the latest op
    pub fn current_root_commitment(&self) -> Option<Commitment> {
        self.latest_epoch()
            .and_then(|epoch| self.ops.get(&epoch))
            .and_then(|op| op.root_commitment())
            .copied()
    }

    /// Invalidate the tree cache (call when ops change)
    fn invalidate_tree_cache(&mut self) {
        self.tree_cache = None;
    }

    /// Prune old intents that are stale
    pub fn prune_stale_intents(&mut self, current_commitment: &Commitment) {
        let stale_ids: Vec<IntentId> = self
            .intents
            .values()
            .filter(|intent| intent.is_stale(current_commitment))
            .map(|intent| intent.intent_id)
            .collect();

        for id in stale_ids {
            self.intents.remove(&id);
        }
    }

    /// Get statistics about the journal
    pub fn stats(&mut self) -> Result<JournalStats, JournalError> {
        // Collect stats before borrowing tree
        let num_ops = self.num_ops();
        let num_intents = self.num_intents();
        let num_tombstones = self.intent_tombstones.len();
        let latest_epoch = self.latest_epoch();

        // Get or rebuild tree to count devices/guardians
        let tree = self.get_tree()?;
        let num_devices = tree.num_leaves(); // TODO: Filter to only devices

        Ok(JournalStats {
            num_ops,
            num_intents,
            num_tombstones,
            latest_epoch,
            num_devices,
            num_guardians: 0, // TODO: Count guardians separately
        })
    }
}

impl Default for JournalMap {
    fn default() -> Self {
        Self::new()
    }
}

// Note: JournalStats and JournalError are now defined in crate::effects::journal
// and imported above to avoid duplication

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::identifiers::DeviceId;
    use crate::ledger::tree_op::{ThresholdSignature, TreeOp};
    use crate::tree::{AffectedPath, LeafIndex, NodeIndex, TreeOperation};

    fn create_test_op(epoch: Epoch) -> TreeOpRecord {
        TreeOpRecord {
            epoch,
            op: TreeOp::EpochBump {
                reason: super::super::tree_op::EpochBumpReason::PeriodicRotation,
            },
            affected_indices: vec![],
            new_commitments: BTreeMap::new(),
            capability_refs: vec![],
            attestation: ThresholdSignature::new(vec![0u8; 64], vec![DeviceId::new(); 2], (2, 3)),
            authored_at: 1000,
            author: DeviceId::new(),
        }
    }

    #[test]
    fn test_journal_map_new() {
        let journal = JournalMap::new();
        assert_eq!(journal.num_ops(), 0);
        assert_eq!(journal.num_intents(), 0);
    }

    #[test]
    fn test_append_tree_op() {
        let mut journal = JournalMap::new();
        let op = create_test_op(1);

        let result = journal.append_tree_op(op);
        assert!(result.is_ok());
        assert_eq!(journal.num_ops(), 1);
        assert_eq!(journal.latest_epoch(), Some(1));
    }

    #[test]
    fn test_submit_intent() {
        let mut journal = JournalMap::new();

        let intent = Intent::new(
            TreeOperation::RotatePath {
                leaf_index: LeafIndex(0),
                affected_path: AffectedPath::new(),
            },
            vec![NodeIndex::new(0)],
            Commitment::default(),
            super::super::intent::Priority::default(),
            DeviceId::new(),
            1000,
        );

        let result = journal.submit_intent(intent);
        assert!(result.is_ok());
        assert_eq!(journal.num_intents(), 1);
    }

    #[test]
    fn test_tombstone_intent() {
        let mut journal = JournalMap::new();

        let intent = Intent::new(
            TreeOperation::RotatePath {
                leaf_index: LeafIndex(0),
                affected_path: AffectedPath::new(),
            },
            vec![],
            Commitment::default(),
            super::super::intent::Priority::default(),
            DeviceId::new(),
            1000,
        );

        let id = journal.submit_intent(intent).unwrap();
        assert_eq!(journal.num_intents(), 1);

        journal.tombstone_intent(id).unwrap();
        assert_eq!(journal.num_intents(), 0);
        assert_eq!(journal.get_intent_status(&id), IntentStatus::Completed);
    }

    #[test]
    fn test_merge_ops() {
        let mut journal1 = JournalMap::new();
        let mut journal2 = JournalMap::new();

        journal1.append_tree_op(create_test_op(1)).unwrap();
        journal2.append_tree_op(create_test_op(2)).unwrap();

        journal1.merge(&journal2);

        assert_eq!(journal1.num_ops(), 2);
        assert!(journal1.get_op(1).is_some());
        assert!(journal1.get_op(2).is_some());
    }

    #[test]
    fn test_merge_intents_or_set() {
        let mut journal1 = JournalMap::new();
        let mut journal2 = JournalMap::new();

        let intent = Intent::new(
            TreeOperation::RotatePath {
                leaf_index: LeafIndex(0),
                affected_path: AffectedPath::new(),
            },
            vec![],
            Commitment::default(),
            super::super::intent::Priority::default(),
            DeviceId::new(),
            1000,
        );

        let id = intent.intent_id;
        journal2.submit_intent(intent).unwrap();

        journal1.merge(&journal2);
        assert_eq!(journal1.num_intents(), 1);
        assert!(journal1.get_intent(&id).is_some());
    }

    #[test]
    fn test_merge_respects_tombstones() {
        let mut journal1 = JournalMap::new();
        let mut journal2 = JournalMap::new();

        let intent = Intent::new(
            TreeOperation::RotatePath {
                leaf_index: LeafIndex(0),
                affected_path: AffectedPath::new(),
            },
            vec![],
            Commitment::default(),
            super::super::intent::Priority::default(),
            DeviceId::new(),
            1000,
        );

        let id = intent.intent_id;
        journal2.submit_intent(intent).unwrap();

        // Tombstone in journal1
        journal1.intent_tombstones.insert(id);

        journal1.merge(&journal2);

        // Intent should not be added because it's tombstoned
        assert!(!journal1.intents.contains_key(&id));
    }

    #[test]
    fn test_prune_stale_intents() {
        let mut journal = JournalMap::new();

        let old_commitment = Commitment::new([1u8; 32]);
        let new_commitment = Commitment::new([2u8; 32]);

        let intent = Intent::new(
            TreeOperation::RotatePath {
                leaf_index: LeafIndex(0),
                affected_path: AffectedPath::new(),
            },
            vec![],
            old_commitment,
            super::super::intent::Priority::default(),
            DeviceId::new(),
            1000,
        );

        journal.submit_intent(intent).unwrap();
        assert_eq!(journal.num_intents(), 1);

        journal.prune_stale_intents(&new_commitment);
        assert_eq!(journal.num_intents(), 0);
    }

    #[test]
    fn test_stats() {
        let mut journal = JournalMap::new();

        journal.append_tree_op(create_test_op(1)).unwrap();

        let intent = Intent::new(
            TreeOperation::RotatePath {
                leaf_index: LeafIndex(0),
                affected_path: AffectedPath::new(),
            },
            vec![],
            Commitment::default(),
            super::super::intent::Priority::default(),
            DeviceId::new(),
            1000,
        );

        journal.submit_intent(intent).unwrap();

        let stats = journal.stats().expect("stats should be available");
        assert_eq!(stats.num_ops, 1);
        assert_eq!(stats.num_intents, 1);
        assert_eq!(stats.latest_epoch, Some(1));
    }
}
