//! Journal-specific CRDT using harmonized architecture
//!
//! This module refactors the `JournalMap` to use the harmonized CRDT foundation
//! from `aura-core`. The journal becomes a standard `CvState` that can
//! participate in choreographic synchronization protocols.

use crate::ledger::{
    intent::{Intent, IntentId, IntentStatus},
    journal_types::{JournalError, JournalStats},
};
use crate::ratchet_tree::TreeState as RatchetTree;
use aura_core::tree::{AttestedOp as TreeOpRecord, Epoch};
use aura_core::Hash32 as Commitment;

// Note: TreeOpRecord is now AttestedOp from aura_core::tree (imported via lib.rs)
// Using the real type instead of a stub

// Helper functions for AttestedOp to match legacy TreeOpRecord interface
fn get_epoch(attested_op: &TreeOpRecord) -> Epoch {
    attested_op.op.parent_epoch
}

fn get_root_commitment(attested_op: &TreeOpRecord) -> Option<Commitment> {
    Some(Commitment(attested_op.op.parent_commitment))
}

fn verify_threshold(attested_op: &TreeOpRecord) -> bool {
    // TODO: Implement proper FROST signature verification
    // TODO fix - For now, check that we have a signature and signer count > 0
    !attested_op.agg_sig.is_empty() && attested_op.signer_count > 0
}

use aura_core::semilattice::{Bottom, CvState, JoinSemilattice};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Journal CRDT using harmonized architecture
///
/// Implements `CvState` from the foundation layer, making it compatible
/// with choreographic CRDT protocols. Combines authoritative tree operations
/// with a staging intent pool using join-semilattice semantics.
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
    #[serde(skip)]
    tree_cache: Option<RatchetTree>,
}

impl JoinSemilattice for JournalMap {
    /// Join operation implementing CRDT merge semantics
    ///
    /// Merges two journal states according to CRDT laws:
    /// - Ops namespace: Keep highest commitment hash per epoch
    /// - Intents namespace: Observed-remove set semantics (add wins)
    /// - Tombstones: Union of all tombstones
    fn join(&self, other: &Self) -> Self {
        let mut result = self.clone();

        // Merge ops namespace
        for (epoch, op) in &other.ops {
            if let Some(existing) = result.ops.get(epoch) {
                // Resolve conflict by commitment hash
                if let (Some(existing_root), Some(other_root)) =
                    (get_root_commitment(existing), get_root_commitment(op))
                {
                    if other_root > existing_root {
                        result.ops.insert(*epoch, op.clone());
                    }
                }
            } else {
                result.ops.insert(*epoch, op.clone());
            }
        }

        // Merge intents namespace (OR-Set semantics)
        // Add wins: if an intent is in other and not tombstoned here, add it
        for (id, intent) in &other.intents {
            if !result.intent_tombstones.contains(id) {
                result.intents.insert(*id, intent.clone());
            }
        }

        // Merge tombstones (union)
        for id in &other.intent_tombstones {
            result.intent_tombstones.insert(*id);
            result.intents.remove(id); // Remove if present
        }

        // Invalidate tree cache since state changed
        result.tree_cache = None;

        result
    }
}

impl Bottom for JournalMap {
    /// Return the bottom element (empty journal)
    fn bottom() -> Self {
        Self {
            ops: BTreeMap::new(),
            intents: BTreeMap::new(),
            intent_tombstones: BTreeSet::new(),
            tree_cache: None,
        }
    }
}

impl CvState for JournalMap {}

impl JournalMap {
    /// Create a new empty journal map
    pub fn new() -> Self {
        Self::bottom()
    }

    // === Query Methods ===

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

    /// Get the current root commitment from the latest op
    pub fn current_root_commitment(&self) -> Option<Commitment> {
        self.latest_epoch()
            .and_then(|epoch| self.ops.get(&epoch))
            .and_then(get_root_commitment)
    }

    // === Mutation Methods ===

    /// Append a tree operation
    ///
    /// Note: This method modifies self in place for compatibility with existing code.
    /// In pure CRDT usage, operations would be applied via `join()` with new states.
    pub fn append_tree_op(&mut self, op: TreeOpRecord) -> Result<(), JournalError> {
        let epoch = get_epoch(&op);

        // Verify threshold signature
        if !verify_threshold(&op) {
            return Err(JournalError::InvalidSignature {
                epoch,
                reason: "Threshold not met".to_string(),
            });
        }

        // Create new state with this operation and join
        let mut new_state = Self::bottom();
        new_state.ops.insert(epoch, op);

        *self = self.join(&new_state);

        Ok(())
    }

    /// Submit an intent to the pool
    ///
    /// Note: This method modifies self in place for compatibility with existing code.
    pub fn submit_intent(&mut self, intent: Intent) -> Result<IntentId, JournalError> {
        let id = intent.intent_id;

        // Check if tombstoned
        if self.intent_tombstones.contains(&id) {
            return Err(JournalError::IntentTombstoned(id));
        }

        // Create new state with this intent and join
        let mut new_state = Self::bottom();
        new_state.intents.insert(id, intent);

        *self = self.join(&new_state);

        Ok(id)
    }

    /// Tombstone an intent (mark as completed)
    ///
    /// Note: This method modifies self in place for compatibility with existing code.
    pub fn tombstone_intent(&mut self, id: IntentId) -> Result<(), JournalError> {
        // Create new state with this tombstone and join
        let mut new_state = Self::bottom();
        new_state.intent_tombstones.insert(id);

        *self = self.join(&new_state);

        Ok(())
    }

    /// Merge another journal map into this one (legacy method)
    ///
    /// This is kept for backwards compatibility. It delegates to the `join` method
    /// which implements the proper CRDT semantics.
    pub fn merge(&mut self, other: &JournalMap) {
        *self = self.join(other);
    }

    // === Tree Reconstruction ===

    /// Get the cached tree or rebuild it
    ///
    /// Note: Tree reconstruction should use reduce() from the ratchet_tree module
    pub fn get_tree(&mut self) -> Result<&RatchetTree, JournalError> {
        if self.tree_cache.is_none() {
            self.tree_cache = Some(RatchetTree::new());
        }
        #[allow(clippy::unwrap_used)]
        Ok(self.tree_cache.as_ref().unwrap())
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
            let _ = self.tombstone_intent(id); // Ignore errors for pruning
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

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::DeviceId;
    use aura_core::tree::{NodeIndex, TreeOp, TreeOpKind};

    fn create_test_op(epoch: Epoch) -> TreeOpRecord {
        TreeOpRecord {
            op: TreeOp {
                parent_epoch: epoch,
                parent_commitment: [0u8; 32],
                op: TreeOpKind::RotateEpoch {
                    affected: vec![NodeIndex(0)],
                },
                version: 1,
            },
            agg_sig: vec![0u8; 64],
            signer_count: 2,
        }
    }

    #[test]
    fn test_journal_map_implements_crdt_traits() {
        let journal1 = JournalMap::new();
        let journal2 = JournalMap::new();

        // Test JoinSemilattice
        let joined = journal1.join(&journal2);
        assert_eq!(joined, journal1); // Joining empty journals gives empty

        // Test Bottom
        let bottom = JournalMap::bottom();
        assert_eq!(bottom.num_ops(), 0);
        assert_eq!(bottom.num_intents(), 0);

        // Joining with bottom is identity
        assert_eq!(journal1.join(&bottom), journal1);
    }

    #[test]
    fn test_join_with_different_ops() {
        let mut journal1 = JournalMap::new();
        let mut journal2 = JournalMap::new();

        journal1.append_tree_op(create_test_op(1)).unwrap();
        journal2.append_tree_op(create_test_op(2)).unwrap();

        let joined = journal1.join(&journal2);

        assert_eq!(joined.num_ops(), 2);
        assert!(joined.get_op(1).is_some());
        assert!(joined.get_op(2).is_some());
    }

    #[test]
    fn test_join_commutative() {
        let mut journal1 = JournalMap::new();
        let mut journal2 = JournalMap::new();

        journal1.append_tree_op(create_test_op(1)).unwrap();
        journal2.append_tree_op(create_test_op(2)).unwrap();

        let join1 = journal1.join(&journal2);
        let join2 = journal2.join(&journal1);

        assert_eq!(join1, join2);
    }

    #[test]
    fn test_join_idempotent() {
        let mut journal1 = JournalMap::new();
        journal1.append_tree_op(create_test_op(1)).unwrap();

        let joined = journal1.join(&journal1);
        assert_eq!(joined, journal1);
    }

    #[test]
    fn test_join_associative() {
        let mut journal1 = JournalMap::new();
        let mut journal2 = JournalMap::new();
        let mut journal3 = JournalMap::new();

        journal1.append_tree_op(create_test_op(1)).unwrap();
        journal2.append_tree_op(create_test_op(2)).unwrap();
        journal3.append_tree_op(create_test_op(3)).unwrap();

        let join12_3 = journal1.join(&journal2).join(&journal3);
        let join1_23 = journal1.join(&journal2.join(&journal3));

        assert_eq!(join12_3, join1_23);
    }

    #[test]
    fn test_intent_observed_remove_semantics() {
        use crate::ledger::intent::{Intent, Priority};
        use aura_core::tree::{LeafNode, TreeOpKind};

        let mut journal1 = JournalMap::new();
        let mut journal2 = JournalMap::new();

        let intent = Intent::new(
            TreeOpKind::AddLeaf {
                leaf: LeafNode::new_device(
                    aura_core::tree::LeafId(0),
                    aura_core::DeviceId(uuid::Uuid::from_bytes([3u8; 16])),
                    vec![0u8; 32],
                ),
                under: NodeIndex(0),
            },
            vec![],
            aura_core::Hash32([0u8; 32]),
            Priority::default_priority(),
            DeviceId(uuid::Uuid::from_bytes([4u8; 16])),
            1000,
        );

        let intent_id = intent.intent_id;
        journal1.submit_intent(intent).unwrap();
        journal2.tombstone_intent(intent_id).unwrap();

        let joined = journal1.join(&journal2);

        // Tombstone should win - intent should be removed
        assert!(joined.get_intent(&intent_id).is_none());
        assert_eq!(
            joined.get_intent_status(&intent_id),
            IntentStatus::Completed
        );
    }
}
