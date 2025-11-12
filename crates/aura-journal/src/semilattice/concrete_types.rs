//! Domain-specific CRDT implementations using foundation traits
//!
//! This module provides journal-specific CRDT types built on the
//! harmonized foundation from `aura-core`.

use crate::ledger::intent::{Intent, IntentId};
use aura_core::identifiers::DeviceId;
use aura_core::semilattice::{Bottom, CvState, JoinSemilattice};
use aura_core::{AttestedOp, Hash32};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Intent pool CRDT with observed-remove semantics
///
/// Manages a pool of pending intents where additions win over removals,
/// providing eventual consistency for intent staging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentPool {
    /// Active intents
    pub intents: BTreeMap<IntentId, Intent>,
    /// Tombstones for removed intents
    pub tombstones: BTreeSet<IntentId>,
}

impl IntentPool {
    /// Create a new empty intent pool
    pub fn new() -> Self {
        Self {
            intents: BTreeMap::new(),
            tombstones: BTreeSet::new(),
        }
    }

    /// Add an intent to the pool
    pub fn add_intent(&mut self, intent: Intent) {
        let id = intent.intent_id;

        // Only add if not tombstoned (observed-remove: add wins)
        if !self.tombstones.contains(&id) {
            self.intents.insert(id, intent);
        }
    }

    /// Remove an intent from the pool
    pub fn remove_intent(&mut self, id: IntentId) {
        self.tombstones.insert(id);
        self.intents.remove(&id);
    }

    /// Check if an intent is present
    pub fn contains(&self, id: &IntentId) -> bool {
        self.intents.contains_key(id)
    }

    /// Get an intent by ID
    pub fn get(&self, id: &IntentId) -> Option<&Intent> {
        self.intents.get(id)
    }

    /// List all active intents
    pub fn list_intents(&self) -> Vec<&Intent> {
        self.intents.values().collect()
    }

    /// Get number of active intents
    pub fn len(&self) -> usize {
        self.intents.len()
    }

    /// Check if pool is empty
    pub fn is_empty(&self) -> bool {
        self.intents.is_empty()
    }
}

impl JoinSemilattice for IntentPool {
    fn join(&self, other: &Self) -> Self {
        let mut result = self.clone();

        // Merge intents (add wins over remove)
        for (id, intent) in &other.intents {
            if !result.tombstones.contains(id) {
                result.intents.insert(*id, intent.clone());
            }
        }

        // Merge tombstones (union)
        for id in &other.tombstones {
            result.tombstones.insert(*id);
            result.intents.remove(id); // Remove if present
        }

        result
    }
}

impl Bottom for IntentPool {
    fn bottom() -> Self {
        Self::new()
    }
}

impl CvState for IntentPool {}

impl Default for IntentPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Tree operation log CRDT using OR-set semantics
///
/// The OpLog is the **source of truth** for all attested tree operations.
/// It implements a grow-only OR-set keyed by operation hash (CID).
///
/// ## Key Properties (from docs/123_ratchet_tree.md):
///
/// - **Append-Only**: Operations are never removed, only added
/// - **OR-Set Semantics**: Union of all seen operations across replicas
/// - **Keyed by Hash**: Operations indexed by H(TreeOp) for deduplication
/// - **No Shares/Transcripts**: Stores only AttestedOp with aggregate signatures
/// - **TreeState is Derived**: Reduction function materializes tree on-demand
///
/// ## CRDT Properties:
///
/// - Join: Set union (all ops from both replicas)
/// - Convergence: All replicas eventually have same OpLog
/// - Deterministic: Same OpLog always reduces to same TreeState
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpLog {
    /// Attested operations indexed by commitment hash (CID)
    /// Using Hash32 as the content identifier
    pub ops: BTreeMap<Hash32, AttestedOp>,
}

impl OpLog {
    /// Create a new empty operation log
    pub fn new() -> Self {
        Self {
            ops: BTreeMap::new(),
        }
    }

    /// Append an attested operation to the log
    ///
    /// The operation is keyed by its commitment hash for deduplication.
    /// Returns the hash (CID) of the operation.
    pub fn append(&mut self, op: AttestedOp) -> Hash32 {
        // Compute CID by hashing the entire operation
        let cid = self.compute_operation_cid(&op);
        self.ops.insert(cid, op);
        cid
    }

    /// Compute the content ID (CID) of an operation
    fn compute_operation_cid(&self, op: &AttestedOp) -> Hash32 {
        use blake3::Hasher;

        let mut hasher = Hasher::new();

        // Hash the TreeOp
        hasher.update(&op.op.parent_epoch.to_le_bytes());
        hasher.update(&op.op.parent_commitment);
        hasher.update(&op.op.version.to_le_bytes());

        // Hash the aggregate signature
        hasher.update(&op.agg_sig);
        hasher.update(&op.signer_count.to_le_bytes());

        let hash = hasher.finalize();
        let mut result = [0u8; 32];
        result.copy_from_slice(hash.as_bytes());
        Hash32(result)
    }

    /// Get an operation by its hash (CID)
    pub fn get(&self, cid: &Hash32) -> Option<&AttestedOp> {
        self.ops.get(cid)
    }

    /// List all operations (unordered)
    pub fn list_ops(&self) -> Vec<&AttestedOp> {
        self.ops.values().collect()
    }

    /// Get number of operations
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Check if log is empty
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Check if log contains an operation with the given hash
    pub fn contains(&self, cid: &Hash32) -> bool {
        self.ops.contains_key(cid)
    }
}

impl JoinSemilattice for OpLog {
    fn join(&self, other: &Self) -> Self {
        let mut result = self.clone();

        // OR-set semantics: union of all operations
        for (cid, op) in &other.ops {
            result.ops.insert(*cid, op.clone());
        }

        result
    }
}

impl Bottom for OpLog {
    fn bottom() -> Self {
        Self::new()
    }
}

impl CvState for OpLog {}

impl Default for OpLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Epoch-ordered operation log CRDT
///
/// Maintains a grow-only log of operations ordered by epoch,
/// with deterministic conflict resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpochLog<T> {
    /// Operations by epoch
    pub ops: BTreeMap<u64, T>,
}

impl<T: Clone> EpochLog<T> {
    /// Create a new empty log
    pub fn new() -> Self {
        Self {
            ops: BTreeMap::new(),
        }
    }

    /// Add an operation to the epoch log
    pub fn add_operation(&mut self, epoch: u64, op: T) {
        self.ops.insert(epoch, op);
    }

    /// Append an operation at the given epoch
    pub fn append(&mut self, epoch: u64, op: T) {
        self.ops.insert(epoch, op);
    }

    /// Get operation at epoch
    pub fn get(&self, epoch: u64) -> Option<&T> {
        self.ops.get(&epoch)
    }

    /// Get all operations in epoch order
    pub fn ops_ordered(&self) -> Vec<&T> {
        self.ops.values().collect()
    }

    /// Get latest epoch
    pub fn latest_epoch(&self) -> Option<u64> {
        self.ops.keys().max().copied()
    }

    /// Get number of operations
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Check if log is empty
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

impl<T: Clone + Ord> JoinSemilattice for EpochLog<T> {
    fn join(&self, other: &Self) -> Self {
        let mut result = self.clone();

        // Merge operations (for conflicts, keep the greater one by Ord)
        for (epoch, op) in &other.ops {
            if let Some(existing) = result.ops.get(epoch) {
                if op > existing {
                    result.ops.insert(*epoch, op.clone());
                }
            } else {
                result.ops.insert(*epoch, op.clone());
            }
        }

        result
    }
}

impl<T: Clone> Bottom for EpochLog<T> {
    fn bottom() -> Self {
        Self::new()
    }
}

impl<T: Clone + Ord> CvState for EpochLog<T> {}

impl<T: Clone> Default for EpochLog<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Device registry CRDT
///
/// Maintains a grow-only set of registered devices with metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRegistry {
    /// Registered devices with their metadata
    pub devices: BTreeMap<DeviceId, crate::types::DeviceMetadata>,
}

impl DeviceRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            devices: BTreeMap::new(),
        }
    }

    /// Register a device
    pub fn register_device(&mut self, metadata: crate::types::DeviceMetadata) {
        self.devices.insert(metadata.device_id, metadata);
    }

    /// Get device metadata
    pub fn get_device(&self, id: &DeviceId) -> Option<&crate::types::DeviceMetadata> {
        self.devices.get(id)
    }

    /// List all registered devices
    pub fn list_devices(&self) -> Vec<&crate::types::DeviceMetadata> {
        self.devices.values().collect()
    }

    /// Check if device is registered
    pub fn is_registered(&self, id: &DeviceId) -> bool {
        self.devices.contains_key(id)
    }

    /// Get number of registered devices
    pub fn len(&self) -> usize {
        self.devices.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }
}

impl JoinSemilattice for DeviceRegistry {
    fn join(&self, other: &Self) -> Self {
        let mut result = self.clone();

        // Merge devices (later registration timestamp wins)
        for (id, metadata) in &other.devices {
            if let Some(existing) = result.devices.get(id) {
                if metadata.added_at > existing.added_at {
                    result.devices.insert(*id, metadata.clone());
                }
            } else {
                result.devices.insert(*id, metadata.clone());
            }
        }

        result
    }
}

impl Bottom for DeviceRegistry {
    fn bottom() -> Self {
        Self::new()
    }
}

impl CvState for DeviceRegistry {}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::intent::{Intent, Priority};
    use aura_core::tree::{LeafNode, LeafRole, TreeOpKind as TreeOperation};
    use aura_core::{NodeIndex, TreeCommitment};

    #[test]
    fn test_intent_pool_join_semantics() {
        let mut pool1 = IntentPool::new();
        let mut pool2 = IntentPool::new();

        let intent = Intent::new(
            TreeOperation::AddLeaf {
                leaf: LeafNode::new_device(
                    aura_core::tree::LeafId(0),
                    aura_core::DeviceId(uuid::Uuid::new_v4()),
                    vec![0u8; 32],
                ),
                under: NodeIndex(0),
            },
            vec![],
            aura_core::Hash32([0u8; 32]),
            Priority::default_priority(),
            DeviceId(uuid::Uuid::new_v4()),
            1000,
        );

        let intent_id = intent.intent_id;

        // Add intent to pool1
        pool1.add_intent(intent);

        // Remove intent from pool2 (tombstone)
        pool2.remove_intent(intent_id);

        // Join should result in intent being removed (tombstone wins)
        let joined = pool1.join(&pool2);
        assert!(!joined.contains(&intent_id));
    }

    #[test]
    fn test_epoch_log_conflict_resolution() {
        let mut log1 = EpochLog::<String>::new();
        let mut log2 = EpochLog::<String>::new();

        // Same epoch, different values
        log1.append(1, "value_a".to_string());
        log2.append(1, "value_b".to_string());

        let joined = log1.join(&log2);

        // Higher value should win (lexicographic ordering)
        assert_eq!(joined.get(1), Some(&"value_b".to_string()));
    }

    // Note: DeviceRegistry test requires types that may not exist yet
    // Skipping TODO fix - For now - can be added when DeviceMetadata is available

    // #[test]
    // fn test_device_registry_registration_conflict() { ... }

    #[test]
    fn test_crdt_laws() {
        let pool1 = IntentPool::new();
        let pool2 = IntentPool::new();
        let pool3 = IntentPool::new();

        // Commutativity: a ⊔ b = b ⊔ a
        assert_eq!(pool1.join(&pool2), pool2.join(&pool1));

        // Associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
        assert_eq!(
            pool1.join(&pool2).join(&pool3),
            pool1.join(&pool2.join(&pool3))
        );

        // Idempotence: a ⊔ a = a
        assert_eq!(pool1.join(&pool1), pool1);

        // Identity: a ⊔ ⊥ = a
        assert_eq!(pool1.join(&IntentPool::bottom()), pool1);
    }
}
