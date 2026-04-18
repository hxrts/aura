//! OpLog CRDT - OR-Set for Attested Tree Operations
//!
//! This module implements the OpLog CRDT that serves as the single source of truth
//! for all tree operations. The OpLog is an OR-set (Observed-Remove Set) that stores
//! AttestedOp instances with deterministic conflict resolution.
//!
//! ## Key Properties (from docs/123_commitment_tree.md):
//!
//! - **OR-set semantics**: Operations are added but never removed
//! - **Join-semilattice**: Supports merge operations with associativity/commutativity
//! - **Content addressing**: Operations identified by Hash32 CID
//! - **Deterministic ordering**: For reduction reproducibility

use aura_core::hash;
use aura_core::semilattice::JoinSemilattice;
use aura_core::{
    tree::{AttestedOp, TreeHash32},
    Epoch, Hash32,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Summary of an OpLog for efficient synchronization
///
/// Contains essential information needed for anti-entropy protocols
/// without transmitting the full operation set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpLogSummary {
    /// Version counter of the OpLog
    pub version: u64,
    /// Total number of operations in the log
    pub operation_count: u64,
    /// Set of all operation CIDs in the log
    pub cids: BTreeSet<Hash32>,
}

impl OpLogSummary {
    /// Check if this summary indicates an empty log
    pub fn is_empty(&self) -> bool {
        self.operation_count == 0
    }

    /// Get CIDs that are in other but not in this summary
    pub fn missing_cids(&self, other: &OpLogSummary) -> BTreeSet<Hash32> {
        other.cids.difference(&self.cids).copied().collect()
    }

    /// Get CIDs that are in this summary but not in other
    pub fn extra_cids(&self, other: &OpLogSummary) -> BTreeSet<Hash32> {
        self.cids.difference(&other.cids).copied().collect()
    }
}

/// OpLog CRDT implementing OR-set semantics for attested tree operations
///
/// The OpLog is the **authoritative source** of all tree operations. TreeState
/// is derived from OpLog through the reduction function and never stored directly.
///
/// ## Properties:
///
/// - **Append-only**: Operations are never removed, only added
/// - **Content-addressed**: Each operation has a unique CID (Hash32)
/// - **Join-semilattice**: Supports deterministic merge operations
/// - **Ordered**: Iteration order is deterministic (by CID for tie-breaking)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpLog {
    /// Operations stored by their content identifier (CID)
    operations: BTreeMap<Hash32, AttestedOp>,
    /// Version counter for conflict resolution
    version: u64,
}

impl OpLog {
    /// Create a new empty OpLog
    pub fn new() -> Self {
        Self {
            operations: BTreeMap::new(),
            version: 0,
        }
    }

    /// Add an attested operation to the log
    ///
    /// Returns the CID (content identifier) of the operation for future reference.
    /// If the operation already exists, returns the existing CID.
    pub fn add_operation(&mut self, op: AttestedOp) -> Hash32 {
        let cid = compute_operation_cid(&op);

        if let std::collections::btree_map::Entry::Vacant(e) = self.operations.entry(cid) {
            e.insert(op);
            self.version += 1;
        }

        cid
    }

    /// Get an operation by its CID
    pub fn get_operation(&self, cid: &Hash32) -> Option<&AttestedOp> {
        self.operations.get(cid)
    }

    /// Check if an operation exists in the log
    pub fn contains_operation(&self, cid: &Hash32) -> bool {
        self.operations.contains_key(cid)
    }

    /// Get all operations in deterministic order
    ///
    /// Operations are returned sorted by CID to ensure deterministic ordering
    /// across all replicas for the reduction algorithm.
    pub fn get_all_operations(&self) -> Vec<&AttestedOp> {
        self.operations.values().collect()
    }

    /// Get operations as a vector (owned)
    pub fn to_operations_vec(&self) -> Vec<AttestedOp> {
        self.operations.values().cloned().collect()
    }

    /// Get all CIDs in the log
    pub fn get_all_cids(&self) -> BTreeSet<Hash32> {
        self.operations.keys().copied().collect()
    }

    /// Get the number of operations in the log
    pub fn len(&self) -> usize {
        self.operations.len()
    }

    /// Check if the log is empty
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Get the current version of the log
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Iterate over operations in deterministic order
    pub fn iter(&self) -> impl Iterator<Item = (&Hash32, &AttestedOp)> {
        self.operations.iter()
    }

    /// Filter operations by a predicate
    pub fn filter<F>(&self, predicate: F) -> Vec<&AttestedOp>
    where
        F: Fn(&AttestedOp) -> bool,
    {
        self.operations
            .values()
            .filter(|op| predicate(op))
            .collect()
    }

    /// Get operations within a specific epoch range
    pub fn operations_in_epoch_range(
        &self,
        start_epoch: Epoch,
        end_epoch: Epoch,
    ) -> Vec<&AttestedOp> {
        self.operations
            .values()
            .filter(|op| op.op.parent_epoch >= start_epoch && op.op.parent_epoch <= end_epoch)
            .collect()
    }

    /// Get the latest operation by parent epoch
    pub fn latest_operation(&self) -> Option<&AttestedOp> {
        self.operations.values().max_by_key(|op| op.op.parent_epoch)
    }

    /// Get operations that are children of a specific parent
    pub fn children_of_parent(
        &self,
        parent_epoch: Epoch,
        parent_commitment: TreeHash32,
    ) -> Vec<&AttestedOp> {
        self.operations
            .values()
            .filter(|op| {
                op.op.parent_epoch == parent_epoch && op.op.parent_commitment == parent_commitment
            })
            .collect()
    }

    /// Compact the log by removing operations before a specific epoch
    ///
    /// **WARNING**: This breaks the OR-set semantics and should only be used
    /// for garbage collection after creating a snapshot. The snapshot must
    /// preserve the join-semilattice properties.
    pub fn compact_before_epoch(&mut self, min_epoch: Epoch) -> usize {
        let initial_len = self.operations.len();

        self.operations
            .retain(|_, op| op.op.parent_epoch >= min_epoch);

        if self.operations.len() < initial_len {
            self.version += 1;
        }

        initial_len - self.operations.len()
    }

    /// Merge another OpLog into this one (for anti-entropy/sync)
    ///
    /// This implements the OR-set union operation. Operations present in either
    /// log will be present in the result.
    pub fn merge_with(&mut self, other: &OpLog) {
        let mut changed = false;

        for (cid, op) in &other.operations {
            if !self.operations.contains_key(cid) {
                self.operations.insert(*cid, op.clone());
                changed = true;
            }
        }

        if changed {
            self.version += 1;
        }
    }

    /// Get the difference between this log and another (operations we have that they don't)
    pub fn difference(&self, other: &OpLog) -> Vec<&AttestedOp> {
        self.operations
            .iter()
            .filter(|(cid, _)| !other.operations.contains_key(*cid))
            .map(|(_, op)| op)
            .collect()
    }

    /// Get operations missing from this log compared to another
    pub fn missing_from<'a>(&self, other: &'a OpLog) -> Vec<&'a AttestedOp> {
        other.difference(self)
    }

    /// Create a summary of the log for efficient synchronization
    pub fn create_summary(&self) -> OpLogSummary {
        OpLogSummary {
            version: self.version,
            operation_count: self.operations.len() as u64,
            cids: self.get_all_cids(),
        }
    }

    /// Check if this log contains all operations from a summary
    pub fn contains_all_from_summary(&self, summary: &OpLogSummary) -> bool {
        summary.cids.iter().all(|cid| self.contains_operation(cid))
    }
}

impl Default for OpLog {
    fn default() -> Self {
        Self::new()
    }
}

impl JoinSemilattice for OpLog {
    /// Join two OpLogs using OR-set union semantics
    ///
    /// The result contains all operations present in either log.
    /// This operation is:
    /// - Associative: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
    /// - Commutative: a ⊔ b = b ⊔ a
    /// - Idempotent: a ⊔ a = a
    fn join(&self, other: &Self) -> Self {
        let mut result = self.clone();
        result.merge_with(other);
        result
    }
}

impl aura_core::semilattice::Bottom for OpLog {
    /// The bottom element is an empty OpLog
    fn bottom() -> Self {
        Self::new()
    }
}

impl PartialOrd for OpLog {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // OpLog A ≤ OpLog B if A's operations are a subset of B's operations
        let my_cids = self.get_all_cids();
        let other_cids = other.get_all_cids();

        let self_subset = my_cids.is_subset(&other_cids);
        let other_subset = other_cids.is_subset(&my_cids);

        match (self_subset, other_subset) {
            (true, true) => Some(std::cmp::Ordering::Equal),
            (true, false) => Some(std::cmp::Ordering::Less),
            (false, true) => Some(std::cmp::Ordering::Greater),
            (false, false) => None, // Incomparable (neither is subset of the other)
        }
    }
}

// Duplicate OpLogSummary definition removed - using the one defined earlier in the file

/// Compute content identifier (CID) for an attested operation
///
/// Uses BLAKE3 to produce a deterministic hash that uniquely identifies
/// the operation. This is used for:
/// - Deduplication in the OpLog
/// - Tie-breaking in the reduction algorithm
/// - Content addressing for synchronization
fn compute_operation_cid(op: &AttestedOp) -> Hash32 {
    let mut h = hash::hasher();

    // Hash the TreeOp
    h.update(&u64::from(op.op.parent_epoch).to_le_bytes());
    h.update(&op.op.parent_commitment);
    h.update(&op.op.version.to_le_bytes());

    // Hash the operation kind
    match &op.op.op {
        aura_core::TreeOpKind::AddLeaf { leaf, under } => {
            h.update(b"AddLeaf");
            h.update(&leaf.leaf_id.0.to_le_bytes());
            h.update(&under.0.to_le_bytes());
            h.update(&leaf.public_key);
        }
        aura_core::TreeOpKind::RemoveLeaf { leaf, reason } => {
            h.update(b"RemoveLeaf");
            h.update(&leaf.0.to_le_bytes());
            h.update(&[*reason]);
        }
        aura_core::TreeOpKind::ChangePolicy { node, new_policy } => {
            h.update(b"ChangePolicy");
            h.update(&node.0.to_le_bytes());
            h.update(&aura_core::policy_hash(new_policy));
        }
        aura_core::TreeOpKind::RotateEpoch { affected } => {
            h.update(b"RotateEpoch");
            for node in affected {
                h.update(&node.0.to_le_bytes());
            }
        }
    }

    // Hash attestation information
    h.update(&op.signer_count.to_le_bytes());
    h.update(&op.agg_sig);

    Hash32(h.finalize())
}
