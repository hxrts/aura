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
    Hash32,
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
    pub fn operations_in_epoch_range(&self, start_epoch: u64, end_epoch: u64) -> Vec<&AttestedOp> {
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
        parent_epoch: u64,
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
    pub fn compact_before_epoch(&mut self, min_epoch: u64) -> usize {
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

    /// Get all operations as a vector of references
    /// Alias for get_all_operations() for compatibility
    pub fn list_ops(&self) -> Vec<&AttestedOp> {
        self.get_all_operations()
    }

    /// Add an operation to the log
    /// Alias for add_operation() for compatibility
    pub fn append(&mut self, op: AttestedOp) -> Hash32 {
        self.add_operation(op)
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
    h.update(&op.op.parent_epoch.to_le_bytes());
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

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{LeafId, LeafNode, NodeIndex, TreeOp, TreeOpKind};

    fn create_test_operation(leaf_id: u32, parent_epoch: u64) -> AttestedOp {
        AttestedOp {
            op: TreeOp {
                parent_epoch,
                parent_commitment: [0u8; 32],
                op: TreeOpKind::AddLeaf {
                    leaf: LeafNode::new_device(
                        LeafId(leaf_id),
                        aura_core::DeviceId(uuid::Uuid::from_bytes([5u8; 16])),
                        vec![leaf_id as u8; 32],
                    ),
                    under: NodeIndex(0),
                },
                version: 1,
            },
            agg_sig: vec![42u8; 64],
            signer_count: 3,
        }
    }

    #[test]
    fn test_oplog_creation() {
        let log = OpLog::new();
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
        assert_eq!(log.version(), 0);
    }

    #[test]
    fn test_add_operation() {
        let mut log = OpLog::new();
        let op = create_test_operation(1, 0);

        let cid = log.add_operation(op.clone());
        assert_eq!(log.len(), 1);
        assert_eq!(log.version(), 1);
        assert!(log.contains_operation(&cid));
        assert_eq!(log.get_operation(&cid), Some(&op));
    }

    #[test]
    fn test_duplicate_operations() {
        let mut log = OpLog::new();
        let op = create_test_operation(1, 0);

        let cid1 = log.add_operation(op.clone());
        let cid2 = log.add_operation(op.clone());

        assert_eq!(cid1, cid2);
        assert_eq!(log.len(), 1); // Should not add duplicate
        assert_eq!(log.version(), 1); // Version should not increment for duplicate
    }

    #[test]
    fn test_operation_ordering() {
        let mut log = OpLog::new();
        let op1 = create_test_operation(1, 0);
        let op2 = create_test_operation(2, 0);
        let op3 = create_test_operation(3, 0);

        log.add_operation(op3.clone());
        log.add_operation(op1.clone());
        log.add_operation(op2.clone());

        let operations = log.get_all_operations();
        assert_eq!(operations.len(), 3);

        // Operations should be in deterministic order (sorted by CID)
        let cids: Vec<_> = operations
            .iter()
            .map(|op| compute_operation_cid(op))
            .collect();
        let mut sorted_cids = cids.clone();
        sorted_cids.sort();
        assert_eq!(cids, sorted_cids);
    }

    #[test]
    fn test_join_semilattice_properties() {
        let mut log1 = OpLog::new();
        let mut log2 = OpLog::new();

        let op1 = create_test_operation(1, 0);
        let op2 = create_test_operation(2, 0);
        let op3 = create_test_operation(3, 0);

        log1.add_operation(op1.clone());
        log1.add_operation(op2.clone());

        log2.add_operation(op2.clone());
        log2.add_operation(op3.clone());

        // Test join operation
        let joined = log1.join(&log2);
        assert_eq!(joined.len(), 3); // Should have all unique operations

        // Test commutativity: log1 ⊔ log2 = log2 ⊔ log1
        let joined_reverse = log2.join(&log1);
        assert_eq!(joined.get_all_cids(), joined_reverse.get_all_cids());

        // Test idempotency: log ⊔ log = log
        let self_joined = log1.join(&log1);
        assert_eq!(self_joined.get_all_cids(), log1.get_all_cids());
    }

    #[test]
    fn test_merge_with() {
        let mut log1 = OpLog::new();
        let mut log2 = OpLog::new();

        let op1 = create_test_operation(1, 0);
        let op2 = create_test_operation(2, 0);

        log1.add_operation(op1.clone());
        log2.add_operation(op2.clone());

        let initial_version = log1.version();
        log1.merge_with(&log2);

        assert_eq!(log1.len(), 2);
        assert!(log1.version() > initial_version);
        assert!(log1.contains_operation(&compute_operation_cid(&op1)));
        assert!(log1.contains_operation(&compute_operation_cid(&op2)));
    }

    #[test]
    fn test_filtering_and_queries() {
        let mut log = OpLog::new();
        let op1 = create_test_operation(1, 0);
        let op2 = create_test_operation(2, 5);
        let op3 = create_test_operation(3, 10);

        log.add_operation(op1);
        log.add_operation(op2);
        log.add_operation(op3);

        // Test epoch range filtering
        let early_ops = log.operations_in_epoch_range(0, 5);
        assert_eq!(early_ops.len(), 2);

        // Test latest operation
        let latest = log.latest_operation();
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().op.parent_epoch, 10);

        // Test filtering by predicate
        let epoch_zero_ops = log.filter(|op| op.op.parent_epoch == 0);
        assert_eq!(epoch_zero_ops.len(), 1);
    }

    #[test]
    fn test_partial_ordering() {
        let mut log1 = OpLog::new();
        let mut log2 = OpLog::new();

        let op1 = create_test_operation(1, 0);
        let op2 = create_test_operation(2, 0);

        log1.add_operation(op1.clone());

        log2.add_operation(op1.clone());
        log2.add_operation(op2.clone());

        // log1 should be less than log2 (subset relationship)
        assert!(log1 < log2);
        assert!(log2 > log1);

        // Self comparison should be equal
        assert_eq!(log1.partial_cmp(&log1), Some(std::cmp::Ordering::Equal));
    }

    #[test]
    fn test_summary_and_sync() {
        let mut log1 = OpLog::new();
        let mut log2 = OpLog::new();

        let op1 = create_test_operation(1, 0);
        let op2 = create_test_operation(2, 0);

        log1.add_operation(op1.clone());
        log2.add_operation(op2.clone());

        let summary1 = log1.create_summary();
        let summary2 = log2.create_summary();

        // Check missing operations
        assert!(!log1.contains_all_from_summary(&summary2));
        assert!(!log2.contains_all_from_summary(&summary1));

        let missing_in_log1 = summary1.missing_cids(&summary2);
        assert_eq!(missing_in_log1.len(), 1);
    }

    #[test]
    fn test_compaction() {
        let mut log = OpLog::new();

        let op1 = create_test_operation(1, 0);
        let op2 = create_test_operation(2, 5);
        let op3 = create_test_operation(3, 10);

        log.add_operation(op1);
        log.add_operation(op2);
        log.add_operation(op3);

        assert_eq!(log.len(), 3);

        // Compact operations before epoch 8
        let removed = log.compact_before_epoch(8);
        assert_eq!(removed, 2); // Should remove op1 and op2
        assert_eq!(log.len(), 1); // Only op3 should remain
    }

    #[test]
    fn test_cid_determinism() {
        let op1 = create_test_operation(1, 0);
        let op2 = create_test_operation(1, 0); // Identical

        let cid1 = compute_operation_cid(&op1);
        let cid2 = compute_operation_cid(&op2);

        assert_eq!(cid1, cid2); // Same operation should have same CID

        let op3 = create_test_operation(2, 0); // Different leaf_id
        let cid3 = compute_operation_cid(&op3);

        assert_ne!(cid1, cid3); // Different operations should have different CIDs
    }
}
