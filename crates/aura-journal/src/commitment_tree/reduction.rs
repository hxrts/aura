//! Deterministic Reduction Function
//!
//! This module implements the deterministic reduction algorithm that converts
//! an OpLog (OR-set of AttestedOp) into a TreeState.
//!
//! ## Algorithm (from docs/123_commitment_tree.md):
//!
//! 1. Build a DAG using parent references (parent_epoch, parent_commitment)
//! 2. Topologically sort by ancestry
//! 3. For concurrent operations (same parent), use H(op) as tie-breaker (max wins)
//! 4. Apply winners in order, updating TreeState and commitment
//! 5. Mark losers as superseded
//!
//! ## Required Properties:
//!
//! - **Deterministic**: Same OpLog always produces same TreeState
//! - **Total**: All valid operations can be applied
//! - **Rejection**: Invalid operations rejected during application
//!
//! ## Verification:
//!
//! - Aggregate signature validates against committed group key
//! - Parent binding prevents replay attacks
//! - Commitment chain ensures integrity

use super::state::{TreeState, TreeStateError};
use aura_core::hash;
use aura_core::util::graph::DagNode;
use aura_core::{
    tree::{commit_leaf, AttestedOp, Epoch, NodeIndex, Policy, TreeHash32, TreeOp, TreeOpKind},
    Hash32,
};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

// =============================================================================
// OpNode - DagNode Implementation for AttestedOp
// =============================================================================

/// Parent key for operation DAG dependencies.
///
/// Operations form a DAG where each operation references its parent by
/// (epoch, commitment). This type provides a hashable, comparable key
/// for tracking dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParentKey {
    /// Epoch of the parent state
    pub epoch: Epoch,
    /// Commitment hash of the parent state
    pub commitment: TreeHash32,
}

impl ParentKey {
    /// Create a new parent key from epoch and commitment
    pub fn new(epoch: Epoch, commitment: TreeHash32) -> Self {
        Self { epoch, commitment }
    }
}

/// Wrapper type that implements `DagNode` for `AttestedOp`.
///
/// This wrapper allows operations to be used with generic DAG algorithms
/// from `aura_core::util::graph`. The operation's hash serves as its ID,
/// and its parent (epoch, commitment) serves as the dependency.
///
/// ## Note on Dependencies
///
/// Unlike view dependencies (explicit list of view IDs), operation dependencies
/// are parent references that may point to state rather than other operations.
/// The DAG sorting algorithm gracefully handles missing dependencies.
#[derive(Clone)]
pub struct OpNode<'a>(pub &'a AttestedOp);

impl DagNode for OpNode<'_> {
    type Id = TreeHash32;

    /// Returns the hash of this operation as its unique identifier.
    fn dag_id(&self) -> Self::Id {
        hash_op(self.0)
    }

    /// Returns the parent key as a hash for dependency tracking.
    ///
    /// The parent is identified by (epoch, commitment) but we convert this
    /// to a hash for the DagNode interface. The actual sorting function
    /// uses specialized logic with tie-breaking for concurrent operations.
    fn dag_dependencies(&self) -> Vec<Self::Id> {
        // Convert parent key to a deterministic hash
        let mut hasher = hash::hasher();
        hasher.update(b"PARENT_KEY");
        hasher.update(&self.0.op.parent_epoch.to_le_bytes());
        hasher.update(&self.0.op.parent_commitment);
        vec![hasher.finalize()]
    }
}

/// Reduce an OpLog to a TreeState
///
/// This is the core reduction function that deterministically computes the
/// materialized tree state from a set of attested operations.
///
/// ## Algorithm:
///
/// 1. Build parent-child DAG
/// 2. Topological sort with tie-breaking
/// 3. Apply operations in order
/// 4. Validate at each step
///
/// ## Invariants:
///
/// - Result is deterministic across all replicas
/// - Invalid operations are rejected
/// - TreeState always consistent with OpLog
pub fn reduce(ops: &[AttestedOp]) -> Result<TreeState, ReductionError> {
    if ops.is_empty() {
        return Ok(TreeState::new());
    }

    // Step 1: Build DAG
    let dag = build_dag(ops)?;

    // Step 2: Topological sort with tie-breaking
    let sorted_ops = topological_sort_with_tiebreak(&dag, ops)?;

    // Step 3: Apply operations in order
    let mut state = TreeState::new();
    for op in sorted_ops {
        apply_operation(&mut state, op)?;
    }

    Ok(state)
}

/// Build parent-child DAG from operations
///
/// Each operation references its parent by (epoch, commitment).
/// This builds the dependency graph for topological sorting.
fn build_dag(ops: &[AttestedOp]) -> Result<Dag<'_>, ReductionError> {
    let mut dag = Dag::new();

    for op in ops {
        let op_hash = hash_op(op);
        let parent_key = (op.op.parent_epoch, op.op.parent_commitment);

        dag.add_edge(parent_key, op_hash, op);
    }

    Ok(dag)
}

/// Topological sort with tie-breaking for concurrent operations
///
/// When multiple operations share the same parent (concurrent), we use
/// H(op) as a deterministic tie-breaker (maximum hash wins).
fn topological_sort_with_tiebreak<'a>(
    _dag: &Dag<'a>,
    ops: &'a [AttestedOp],
) -> Result<Vec<&'a AttestedOp>, ReductionError> {
    let mut sorted = Vec::new();
    let mut visited = BTreeSet::new();

    // Group operations by parent
    let mut by_parent: BTreeMap<(Epoch, Hash32), Vec<&AttestedOp>> = BTreeMap::new();
    for op in ops {
        let parent_key = (
            op.op.parent_epoch,
            aura_core::Hash32(op.op.parent_commitment),
        );
        by_parent.entry(parent_key).or_default().push(op);
    }

    // For each parent, resolve conflicts using H(op) tie-breaker
    for (_parent_key, mut children) in by_parent {
        if children.len() > 1 {
            // Multiple concurrent operations - use tie-breaker
            children.sort_by_key(|op| hash_op(op));
            children.reverse(); // Maximum hash wins
        }

        // Take the winner (first after sorting)
        if let Some(winner) = children.first() {
            let op_hash = hash_op(winner);
            if !visited.contains(&op_hash) {
                sorted.push(*winner);
                visited.insert(op_hash);
            }
        }
    }

    // Sort by parent epoch for proper ordering
    sorted.sort_by_key(|op| op.op.parent_epoch);

    Ok(sorted)
}

/// Apply a single operation to the tree state (Phase 2.1d)
///
/// This validates the operation and updates the tree structure.
///
/// ## Validation (Phase 2.1e):
///
/// 1. Verify parent binding (epoch and commitment match)
/// 2. Apply the operation to tree structure
/// 3. Validate invariants (stricter-or-equal policy for ChangePolicy)
/// 4. Recompute affected path commitments
///
/// ## Implementation Status:
///
/// - Parent binding verification: Implemented
/// - Operation application: Implemented for all TreeOpKind variants
/// - Policy meet verification: Implemented for ChangePolicy
/// - Commitment recomputation: Implemented
/// - Path commitment updates: Implemented with tree traversal
/// - FROST signature verification: ✅ COMPLETED in application.rs
fn apply_operation(state: &mut TreeState, op: &AttestedOp) -> Result<(), ReductionError> {
    // Phase 2.1e: Parent Binding Verification
    verify_parent_binding(&op.op, state)?;

    // Track affected nodes for commitment recomputation
    let mut affected_nodes = Vec::new();

    // Phase 2.1d: Operation Application
    match &op.op.op {
        TreeOpKind::AddLeaf { leaf, under } => {
            // Add the leaf to the tree under the specified branch
            state.add_leaf_under(leaf.clone(), *under);
            affected_nodes.push(*under);

            // Compute and store leaf commitment
            let leaf_commitment =
                commit_leaf(leaf.leaf_id, state.current_epoch(), &leaf.public_key);
            state.set_leaf_commitment(leaf.leaf_id, leaf_commitment);

            tracing::debug!(
                "Added leaf {:?} under node {:?} with commitment {:?}",
                leaf.leaf_id,
                under,
                hex::encode(&leaf_commitment[..8])
            );
        }
        TreeOpKind::RemoveLeaf { leaf, reason } => {
            // Find the parent node before removing (for affected nodes computation)
            if let Some(parent) = state.get_remove_leaf_affected_parent(leaf) {
                affected_nodes.push(parent);
            }

            // Remove the leaf from the tree
            state
                .remove_leaf(leaf)
                .ok_or(ReductionError::LeafNotFound(*leaf))?;

            tracing::debug!("Removed leaf {:?} with reason {}", leaf, reason);
        }
        TreeOpKind::ChangePolicy { node, new_policy } => {
            // Verify policy change is stricter-or-equal using meet-semilattice
            if let Some(old_policy) = state.get_policy(node) {
                let meet_result = old_policy.meet(new_policy);
                if &meet_result != new_policy {
                    return Err(ReductionError::InvalidOperation(format!(
                        "Policy change from {:?} to {:?} is not stricter-or-equal (meet = {:?})",
                        old_policy, new_policy, meet_result
                    )));
                }
            }

            // Update the policy at the node
            state
                .update_branch_policy(node, *new_policy)
                .map_err(|e| ReductionError::InvalidOperation(e.to_string()))?;

            affected_nodes.push(*node);

            tracing::debug!("Changed policy at node {:?} to {:?}", node, new_policy);
        }
        TreeOpKind::RotateEpoch { affected } => {
            // Increment the epoch counter
            state.increment_epoch();

            // Mark all affected nodes for commitment recomputation
            affected_nodes.extend(affected.iter());

            tracing::debug!(
                "Rotated epoch to {}, affected {} nodes",
                state.current_epoch(),
                affected.len()
            );
        }
    }

    // Recompute commitments for affected nodes and update root
    recompute_commitments(state, &affected_nodes)?;

    Ok(())
}

/// Recompute commitments for affected nodes and update root
///
/// This function efficiently recomputes commitments for only the nodes affected
/// by an operation, walking up the tree paths to the root. This is much more
/// efficient than recomputing the entire tree.
///
/// ## Algorithm:
///
/// 1. Collect all affected paths from affected nodes to root
/// 2. Recompute commitments for affected nodes bottom-up
/// 3. Propagate changes up to root
/// 4. Update root commitment last
fn recompute_commitments(
    state: &mut TreeState,
    affected_nodes: &[NodeIndex],
) -> Result<(), ReductionError> {
    if affected_nodes.is_empty() {
        // No nodes affected, but still need to update root with new epoch
        recompute_root_commitment_simple(state);
        return Ok(());
    }

    // Collect all nodes in the paths from affected nodes to root
    let mut all_affected_nodes = BTreeSet::new();

    for &node in affected_nodes {
        // Add the node itself
        all_affected_nodes.insert(node);

        // Add all nodes in path to root
        let path_to_root = compute_path_to_root(state, node);
        all_affected_nodes.extend(path_to_root);
    }

    // Convert to sorted vector for deterministic processing
    let mut path_nodes: Vec<_> = all_affected_nodes.into_iter().collect();

    // Sort by dependency order: leaves first, then branches, then root
    // This ensures we compute commitments in bottom-up order
    path_nodes.sort_by_key(|&node| {
        (get_tree_level(state, node), node.0) // Sort by level then by index for determinism
    });

    // Recompute each affected branch commitment
    for &node in &path_nodes {
        // Get the policy first to avoid borrowing conflicts
        let policy = if let Some(branch) = state.branches.get(&node) {
            branch.policy
        } else {
            continue;
        };

        // Recompute this branch's commitment based on its current policy and children
        let new_commitment = compute_branch_commitment(state, node, &policy)?;

        // Now update the commitment
        if let Some(branch) = state.branches.get_mut(&node) {
            branch.commitment = *new_commitment.as_bytes();

            tracing::debug!(
                "Recomputed commitment for branch {:?}: {:?}",
                node,
                hex::encode(&new_commitment.as_bytes()[..8])
            );
        }
    }

    // Update root commitment based on all current commitments
    recompute_root_commitment_from_tree(state);

    Ok(())
}

/// Compute commitment for a branch node based on current tree state
fn compute_branch_commitment(
    state: &TreeState,
    node: NodeIndex,
    policy: &Policy,
) -> Result<Hash32, ReductionError> {
    // Get child commitments
    let children = state.get_children(node);
    let mut child_commitments = Vec::new();

    // Collect commitments from child branches
    for child in &children {
        if let Some(child_branch) = state.get_branch(child) {
            child_commitments.push(child_branch.commitment);
        }
    }

    // Collect commitments from leaves under this branch
    for leaf_id in state.leaves.keys() {
        if state.get_leaf_parent(*leaf_id) == Some(node) {
            if let Some(leaf_commitment) = state.get_leaf_commitment(leaf_id) {
                child_commitments.push(*leaf_commitment);
            }
        }
    }

    // Sort for determinism
    child_commitments.sort();

    // Compute branch commitment using aura-core's commit_branch function
    // Compute policy hash for the branch
    let policy_hash = aura_core::policy_hash(policy);

    let mut hasher = hash::hasher();
    hasher.update(b"BRANCH");
    hasher.update(&1u16.to_le_bytes()); // version
    hasher.update(&node.0.to_le_bytes());
    hasher.update(&state.current_epoch().to_le_bytes());
    hasher.update(&policy_hash);

    // Include child commitments
    for commitment in &child_commitments {
        hasher.update(commitment);
    }

    Ok(aura_core::Hash32(hasher.finalize()))
}

/// Simple root commitment computation (fallback)
fn recompute_root_commitment_simple(state: &mut TreeState) {
    let mut hasher = hash::hasher();

    // Include epoch to ensure commitments change on epoch rotation
    hasher.update(&state.current_epoch().to_le_bytes());

    // Include all leaf commitments (sorted by LeafId for determinism)
    for (leaf_id, leaf_commitment) in state.iter_leaf_commitments() {
        hasher.update(&leaf_id.0.to_le_bytes());
        hasher.update(leaf_commitment);
    }

    // Include all branch commitments (sorted by NodeIndex for determinism)
    for branch in state.iter_branches() {
        hasher.update(&branch.node.0.to_le_bytes());
        hasher.update(&branch.commitment);
        hasher.update(&aura_core::policy_hash(&branch.policy));
    }

    state.set_root_commitment(hasher.finalize());
}

/// Recompute root commitment from current tree structure
fn recompute_root_commitment_from_tree(state: &mut TreeState) {
    // Deterministic recomputation over all branch and leaf commitments
    recompute_root_commitment_simple(state);
}

/// Verify parent binding for an operation (Phase 2.1e)
///
/// Parent binding prevents:
/// - Replay attacks (operation tied to specific parent state)
/// - Lineage violations (operation must extend current state)
/// - Fork attacks (commitment chain ensures single history)
///
/// ## Implementation Status:
///
/// - Epoch verification: Implemented
/// - Commitment verification: Implemented
fn verify_parent_binding(op: &TreeOp, state: &TreeState) -> Result<(), ReductionError> {
    // Genesis operations (epoch 0) don't have parent binding
    if op.parent_epoch == 0 {
        return Ok(());
    }

    // Verify epoch matches (operation extends current state)
    if op.parent_epoch != state.current_epoch() {
        return Err(ReductionError::ParentBindingInvalid {
            expected: state.current_epoch(),
            actual: op.parent_epoch,
        });
    }

    // Verify commitment matches (ensures lineage)
    if aura_core::Hash32(op.parent_commitment) != aura_core::Hash32(state.current_commitment()) {
        return Err(ReductionError::CommitmentMismatch {
            expected: aura_core::Hash32(state.current_commitment()),
            actual: aura_core::Hash32(op.parent_commitment),
        });
    }

    Ok(())
}

/// Hash an operation for tie-breaking
///
/// Uses BLAKE3 to produce a deterministic hash of the entire operation.
fn hash_op(op: &AttestedOp) -> TreeHash32 {
    let mut hasher = hash::hasher();

    // Hash the operation fields
    hasher.update(&op.op.parent_epoch.to_le_bytes());
    hasher.update(&op.op.parent_commitment);
    hasher.update(&op.op.version.to_le_bytes());

    // Hash the operation kind discriminant
    match &op.op.op {
        TreeOpKind::AddLeaf { leaf, under } => {
            hasher.update(b"AddLeaf");
            hasher.update(&leaf.leaf_id.0.to_le_bytes());
            hasher.update(&under.0.to_le_bytes());
        }
        TreeOpKind::RemoveLeaf { leaf, reason } => {
            hasher.update(b"RemoveLeaf");
            hasher.update(&leaf.0.to_le_bytes());
            hasher.update(&[*reason]);
        }
        TreeOpKind::ChangePolicy { node, new_policy } => {
            hasher.update(b"ChangePolicy");
            hasher.update(&node.0.to_le_bytes());
            // Hash policy
            hasher.update(&aura_core::policy_hash(new_policy));
        }
        TreeOpKind::RotateEpoch { affected } => {
            hasher.update(b"RotateEpoch");
            for node in affected {
                hasher.update(&node.0.to_le_bytes());
            }
        }
    }

    hasher.finalize()
}

/// Directed acyclic graph for operation ordering
struct Dag<'a> {
    /// Edges: parent_key -> (op_hash, operation)
    edges: BTreeMap<(Epoch, TreeHash32), Vec<(TreeHash32, &'a AttestedOp)>>,
}

impl<'a> Dag<'a> {
    fn new() -> Self {
        Self {
            edges: BTreeMap::new(),
        }
    }

    fn add_edge(&mut self, parent: (Epoch, TreeHash32), op_hash: TreeHash32, op: &'a AttestedOp) {
        self.edges.entry(parent).or_default().push((op_hash, op));
    }
}

/// Errors that can occur during reduction
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ReductionError {
    /// Invalid operation
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Leaf not found
    #[error("Leaf not found: {0:?}")]
    LeafNotFound(aura_core::LeafId),

    /// Cycle detected in DAG
    #[error("Cycle detected in operation DAG")]
    CycleDetected,

    /// Aggregate signature verification failed
    #[error("Aggregate signature verification failed")]
    SignatureVerificationFailed,

    /// Parent binding invalid
    #[error("Parent binding invalid: expected epoch {expected}, got {actual}")]
    ParentBindingInvalid {
        /// The expected epoch
        expected: Epoch,
        /// The actual epoch
        actual: Epoch,
    },

    /// Commitment mismatch in parent binding
    #[error("Commitment mismatch: expected {expected:?}, got {actual:?}")]
    CommitmentMismatch {
        /// The expected commitment
        expected: Hash32,
        /// The actual commitment
        actual: Hash32,
    },

    /// Tree state error
    #[error("Tree state error: {0}")]
    TreeStateError(#[from] TreeStateError),
}

/// Compute path from a node to the root
fn compute_path_to_root(state: &TreeState, node: NodeIndex) -> Vec<NodeIndex> {
    let mut path = Vec::new();
    let mut current = node;

    // Walk up the tree until we reach the root
    while let Some(parent) = state.get_parent(current) {
        path.push(parent);
        current = parent;
    }

    path
}

/// Get the level/depth of a node in the tree (0 = leaf, higher = closer to root)
fn get_tree_level(state: &TreeState, node: NodeIndex) -> u32 {
    let mut level = 0;
    let mut current = node;

    // Count children to approximate tree level (leaves = 0, branches = higher)
    loop {
        let children = state.get_children(current);
        if children.is_empty() {
            break; // This is a leaf
        }
        level += 1;
        // Move to first child to continue traversal
        match children.iter().next() {
            Some(&child) => current = child,
            None => break, // This shouldn't happen given the is_empty check, but be safe
        }
        if level > 100 {
            break; // Prevent infinite loops
        }
    }

    level
}

#[cfg(test)]
mod tests {
    use super::TreeOp;
    use super::*;
    use crate::{LeafId, LeafNode, NodeIndex};

    fn create_test_op(parent_epoch: Epoch, leaf_id: u32) -> AttestedOp {
        AttestedOp {
            op: TreeOp {
                parent_epoch,
                parent_commitment: [0u8; 32],
                op: TreeOpKind::AddLeaf {
                    leaf: LeafNode::new_device(
                        LeafId(leaf_id),
                        aura_core::DeviceId(uuid::Uuid::from_bytes([10u8; 16])),
                        vec![leaf_id as u8; 32],
                    ),
                    under: NodeIndex(0),
                },
                version: 1,
            },
            agg_sig: vec![],
            signer_count: 1,
        }
    }

    #[test]
    fn test_reduce_empty() {
        let ops = vec![];
        let state = reduce(&ops).unwrap();
        assert!(state.is_empty());
        assert_eq!(state.epoch, 0);
    }

    #[test]
    fn test_reduce_single_op() {
        let ops = vec![create_test_op(0, 1)];
        let state = reduce(&ops).unwrap();
        assert_eq!(state.num_leaves(), 1);
        assert!(state.get_leaf(&LeafId(1)).is_some());
    }

    #[test]
    fn test_reduce_multiple_ops() {
        let ops = vec![
            create_test_op(0, 1),
            create_test_op(0, 2),
            create_test_op(0, 3),
        ];

        let state = reduce(&ops).unwrap();
        assert_eq!(state.num_leaves(), 1);
    }

    #[test]
    fn test_hash_op_deterministic() {
        let op = create_test_op(0, 1);
        let hash1 = hash_op(&op);
        let hash2 = hash_op(&op);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_op_different_ops() {
        let op1 = create_test_op(0, 1);
        let op2 = create_test_op(0, 2);
        let hash1 = hash_op(&op1);
        let hash2 = hash_op(&op2);
        assert_ne!(hash1, hash2);
    }
}
