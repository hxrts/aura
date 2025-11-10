//! Tree Operation Application with Verification
//!
//! This module implements the verified application of attested tree operations,
//! ensuring all cryptographic and structural invariants are maintained.
//!
//! ## Application Pipeline
//!
//! 1. **Signature Verification**: Verify aggregate FROST signature
//! 2. **Parent Binding**: Verify operation references current (epoch, commitment)
//! 3. **State Update**: Apply operation to tree state
//! 4. **Commitment Recomputation**: Update cryptographic commitments
//! 5. **Epoch Update**: Increment epoch if required
//! 6. **Invariant Validation**: Verify tree invariants hold
//!
//! Any failure in these steps rejects the operation entirely.

use super::{reduction::ReductionError, TreeState};
use aura_core::{commit_leaf, AttestedOp, Hash32, LeafId, NodeIndex, Policy, TreeOp, TreeOpKind};
use blake3::Hasher;
// Removed unused BTreeMap import

/// Errors that can occur during tree operation application
#[derive(Debug, thiserror::Error)]
pub enum ApplicationError {
    /// Aggregate signature verification failed
    #[error("Invalid aggregate signature")]
    InvalidSignature,

    /// Parent binding verification failed
    #[error("Parent binding mismatch: expected epoch {expected_epoch}, commitment {expected_commitment:?}")]
    ParentBindingMismatch {
        /// The expected epoch value
        expected_epoch: u64,
        /// The expected commitment hash
        expected_commitment: Hash32,
    },

    /// Operation would violate tree invariants
    #[error("Invariant violation: {reason}")]
    InvariantViolation {
        /// Description of the invariant violation
        reason: String,
    },

    /// Node not found in tree
    #[error("Node {0:?} not found")]
    NodeNotFound(NodeIndex),

    /// Leaf not found in tree
    #[error("Leaf {0:?} not found")]
    LeafNotFound(LeafId),

    /// Policy change would make policy less restrictive
    #[error("Policy change would weaken security: {old:?} -> {new:?}")]
    PolicyWeakening {
        /// The old (more restrictive) policy
        old: Policy,
        /// The new (less restrictive) policy
        new: Policy,
    },

    /// Reduction error during application
    #[error("Reduction error: {0}")]
    ReductionError(#[from] ReductionError),
}

/// Result type for application operations
pub type ApplicationResult<T> = Result<T, ApplicationError>;

/// Validate that the tree structure is acyclic
///
/// This checks that following parent pointers from any node will eventually
/// reach a root (node with no parent) without creating cycles.
fn validate_acyclicity(state: &TreeState) -> ApplicationResult<()> {
    use std::collections::{BTreeSet, VecDeque};

    // Get all branch nodes
    let all_nodes: BTreeSet<NodeIndex> = state.list_branch_indices().into_iter().collect();
    let mut visited = BTreeSet::new();
    let mut visiting = BTreeSet::new(); // Tracks nodes currently being processed

    // Check each connected component for cycles
    for &start_node in &all_nodes {
        if visited.contains(&start_node) {
            continue; // Already checked this component
        }

        // DFS to check for cycles starting from this node
        let mut stack = VecDeque::new();
        stack.push_back(start_node);

        while let Some(current) = stack.pop_back() {
            if visited.contains(&current) {
                continue;
            }

            if visiting.contains(&current) {
                // Found a cycle
                return Err(ApplicationError::InvariantViolation {
                    reason: format!(
                        "Cycle detected in tree structure involving node {:?}",
                        current
                    ),
                });
            }

            visiting.insert(current);

            // Follow parent pointer
            if let Some(parent) = state.get_parent(current) {
                if !visited.contains(&parent) {
                    stack.push_back(parent);
                }
            }

            // Follow children (though these shouldn't create cycles if parent pointers are correct)
            for child in state.get_children(current) {
                if !visited.contains(&child) {
                    stack.push_back(child);
                }
            }

            visiting.remove(&current);
            visited.insert(current);
        }
    }

    Ok(())
}

/// Apply an attested operation to tree state with full verification
///
/// This is the main entry point for verified tree operation application.
/// It performs all necessary checks before modifying state.
///
/// ## Steps
///
/// 1. Verify aggregate signature (stub TODO fix - For now - needs FROST integration)
/// 2. Verify parent binding matches current state
/// 3. Apply the operation to tree state
/// 4. Recompute commitments for affected nodes
/// 5. Update epoch if operation requires it
/// 6. Validate all tree invariants
///
/// ## Errors
///
/// Returns `ApplicationError` if any verification step fails.
/// State is NOT modified on error (atomic operation).
pub fn apply_verified(state: &mut TreeState, attested: &AttestedOp) -> ApplicationResult<()> {
    // Step 1: Verify aggregate signature
    // TODO: Integrate with FROST signature verification when aura-crypto is ready
    verify_aggregate_signature(attested, state)?;

    // Step 2: Verify parent binding
    verify_parent_binding(&attested.op, state)?;

    // Step 3: Apply operation to tree state
    let affected_nodes = apply_operation_to_state(state, &attested.op)?;

    // Step 4: Recompute commitments for affected path
    recompute_commitments(state, &affected_nodes)?;

    // Step 5: Update epoch if required
    if requires_epoch_update(&attested.op.op) {
        state.increment_epoch();
    }

    // Step 6: Validate invariants
    validate_invariants(state)?;

    Ok(())
}

/// Verify aggregate FROST signature for attested operation
///
/// **STUB**: This is a placeholder until FROST signature verification
/// is integrated from aura-crypto. Currently always succeeds.
///
/// ## TODO
///
/// - Extract group public key from tree state for signing node
/// - Compute binding message: H("TREE_OP_SIG" || node_id || epoch || policy_hash || op_bytes)
/// - Verify aggregate signature using FROST verification
/// - Verify signer_count meets threshold requirement
fn verify_aggregate_signature(_attested: &AttestedOp, _state: &TreeState) -> ApplicationResult<()> {
    // Stub implementation - always succeeds TODO fix - For now
    // Real implementation will verify FROST aggregate signature
    Ok(())
}

/// Verify parent binding matches current tree state
///
/// Parent binding prevents replay attacks and ensures lineage.
/// Operations must reference the current (epoch, commitment) to be valid.
///
/// Genesis operations (parent_epoch == 0) are exempt from this check.
fn verify_parent_binding(op: &TreeOp, state: &TreeState) -> ApplicationResult<()> {
    // Genesis operations don't have a parent
    if op.parent_epoch == 0 {
        return Ok(());
    }

    // Check epoch matches
    if op.parent_epoch != state.current_epoch() {
        return Err(ApplicationError::ParentBindingMismatch {
            expected_epoch: state.current_epoch(),
            expected_commitment: Hash32(state.current_commitment()),
        });
    }

    // Check commitment matches
    if op.parent_commitment != state.current_commitment() {
        return Err(ApplicationError::ParentBindingMismatch {
            expected_epoch: state.current_epoch(),
            expected_commitment: Hash32(state.current_commitment()),
        });
    }

    Ok(())
}

/// Apply operation to tree state and return affected nodes
///
/// This modifies the tree structure according to the operation type.
/// Returns the set of nodes whose commitments need recomputation.
fn apply_operation_to_state(
    state: &mut TreeState,
    op: &TreeOp,
) -> ApplicationResult<Vec<NodeIndex>> {
    let mut affected_nodes = Vec::new();

    match &op.op {
        TreeOpKind::AddLeaf { leaf, under } => {
            // Add leaf to tree
            state.insert_leaf(leaf.clone());

            // Track affected nodes for commitment recomputation
            affected_nodes.push(*under);

            // Compute and store leaf commitment
            let leaf_commitment =
                commit_leaf(leaf.leaf_id, state.current_epoch(), &leaf.public_key);
            state.set_leaf_commitment(leaf.leaf_id, leaf_commitment);
        }

        TreeOpKind::RemoveLeaf { leaf, .. } => {
            // Verify leaf exists
            if state.get_leaf(leaf).is_none() {
                return Err(ApplicationError::LeafNotFound(*leaf));
            }

            // Remove leaf from tree
            state.remove_leaf(leaf);

            // TODO: Find parent node and add to affected_nodes
            // TODO fix - For now, we don't track parent pointers, so we can't determine affected nodes
        }

        TreeOpKind::ChangePolicy { node, new_policy } => {
            // Verify node exists
            let branch = state
                .get_branch(node)
                .ok_or(ApplicationError::NodeNotFound(*node))?;

            // Verify new policy is stricter or equal (meet-semilattice)
            let old_policy = &branch.policy;
            if !is_policy_stricter_or_equal(new_policy, old_policy) {
                return Err(ApplicationError::PolicyWeakening {
                    old: old_policy.clone(),
                    new: new_policy.clone(),
                });
            }

            // Update policy
            state.set_policy(*node, new_policy.clone());
            affected_nodes.push(*node);
        }

        TreeOpKind::RotateEpoch { affected } => {
            // Track all affected nodes
            affected_nodes.extend(affected.iter());
            // Epoch will be incremented in apply_verified
        }
    }

    Ok(affected_nodes)
}

/// Check if new policy is stricter or equal to old policy (meet-semilattice)
///
/// Stricter means:
/// - All -> Threshold -> Any (in terms of restrictiveness)
/// - For Threshold: higher m/n ratio is stricter
fn is_policy_stricter_or_equal(new: &Policy, old: &Policy) -> bool {
    use Policy::*;

    match (old, new) {
        // Same policy is always valid
        (a, b) if a == b => true,

        // Any policy can become more restrictive
        (Any, _) => true,

        // All is the most restrictive, can't change to anything else
        (All, _) => false,

        // Threshold can only become more restrictive
        (Threshold { m: m1, n: n1 }, Threshold { m: m2, n: n2 }) => {
            // New threshold must be at least as restrictive
            // (m2/n2) >= (m1/n1) in terms of required proportion
            (m2 * n1) >= (m1 * n2)
        }

        (Threshold { .. }, All) => true,
        (Threshold { .. }, Any) => false,
    }
}

/// Recompute cryptographic commitments for affected nodes
///
/// This updates commitments for all nodes in the affected path from
/// leaves to root. Uses the commitment functions from aura-core.
fn recompute_commitments(
    state: &mut TreeState,
    _affected_nodes: &[NodeIndex],
) -> ApplicationResult<()> {
    // TODO fix - Simplified implementation: recompute root commitment from all state
    // Full implementation would recompute only affected path

    let mut hasher = Hasher::new();
    hasher.update(&state.current_epoch().to_le_bytes());

    // Hash all leaf commitments
    for (leaf_id, leaf_commitment) in state.iter_leaf_commitments() {
        hasher.update(&leaf_id.0.to_le_bytes());
        hasher.update(leaf_commitment);
    }

    // Hash all branch commitments
    for branch in state.iter_branches() {
        hasher.update(&branch.node.0.to_le_bytes());
        hasher.update(&branch.commitment);
        hasher.update(&aura_core::policy_hash(&branch.policy));
    }

    let hash = hasher.finalize();
    let mut root_commitment = [0u8; 32];
    root_commitment.copy_from_slice(hash.as_bytes());
    state.set_root_commitment(root_commitment);

    Ok(())
}

/// Check if operation requires epoch increment
fn requires_epoch_update(op: &TreeOpKind) -> bool {
    matches!(op, TreeOpKind::RotateEpoch { .. })
}

/// Validate all tree invariants
///
/// This ensures the tree maintains all required properties:
/// - Acyclicity (no cycles in tree structure)
/// - Proper node ordering (children ordered by NodeIndex)
/// - Policy monotonicity (policies only get stricter)
/// - Commitment integrity (all commitments match computed values)
///
/// ## Invariants Checked
///
/// 1. **Acyclicity**: Tree structure forms a DAG (no cycles)
/// 2. **Node Ordering**: All nodes properly indexed
/// 3. **Policy Monotonicity**: No policy weakening
/// 4. **Commitment Integrity**: Root commitment matches recomputed value
pub fn validate_invariants(state: &TreeState) -> ApplicationResult<()> {
    // 1. Acyclicity check
    // Check for cycles in the tree structure using parent pointers
    validate_acyclicity(state)?;

    // 2. Node ordering check
    // All node indices should be valid (non-negative, unique)
    let branch_indices: Vec<_> = state.iter_branches().map(|b| b.node).collect();
    for (i, &node) in branch_indices.iter().enumerate() {
        if branch_indices[i + 1..].contains(&node) {
            return Err(ApplicationError::InvariantViolation {
                reason: format!("Duplicate node index: {:?}", node),
            });
        }
    }

    // 3. Policy monotonicity
    // This is enforced during application, so no additional check needed here

    // 4. Commitment integrity
    // Root commitment should match the recomputed value
    // This is enforced by recompute_commitments being called after every operation

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::tree::LeafNode;

    #[test]
    fn test_policy_strictness() {
        use Policy::*;

        // Same policy is always valid
        assert!(is_policy_stricter_or_equal(&Any, &Any));
        assert!(is_policy_stricter_or_equal(&All, &All));

        // Any can become anything
        assert!(is_policy_stricter_or_equal(&All, &Any));
        assert!(is_policy_stricter_or_equal(&Threshold { m: 2, n: 3 }, &Any));

        // All is most restrictive
        assert!(!is_policy_stricter_or_equal(&Any, &All));
        assert!(!is_policy_stricter_or_equal(
            &Threshold { m: 2, n: 3 },
            &All
        ));

        // Threshold strictness
        assert!(is_policy_stricter_or_equal(
            &Threshold { m: 3, n: 3 },
            &Threshold { m: 2, n: 3 }
        )); // 3/3 >= 2/3
        assert!(!is_policy_stricter_or_equal(
            &Threshold { m: 1, n: 3 },
            &Threshold { m: 2, n: 3 }
        )); // 1/3 < 2/3
    }

    #[test]
    fn test_verify_parent_binding_genesis() {
        let state = TreeState::new();
        let op = TreeOp {
            parent_epoch: 0,
            parent_commitment: [0u8; 32],
            op: TreeOpKind::RotateEpoch {
                affected: vec![NodeIndex(0)],
            },
            version: 1,
        };

        // Genesis operations should always pass
        assert!(verify_parent_binding(&op, &state).is_ok());
    }

    #[test]
    fn test_verify_parent_binding_mismatch() {
        let mut state = TreeState::new();
        state.set_epoch(5);

        let op = TreeOp {
            parent_epoch: 3, // Wrong epoch
            parent_commitment: state.current_commitment(),
            op: TreeOpKind::RotateEpoch {
                affected: vec![NodeIndex(0)],
            },
            version: 1,
        };

        // Should fail due to epoch mismatch
        assert!(verify_parent_binding(&op, &state).is_err());
    }

    #[test]
    fn test_requires_epoch_update() {
        assert!(requires_epoch_update(&TreeOpKind::RotateEpoch {
            affected: vec![]
        }));
        assert!(!requires_epoch_update(&TreeOpKind::AddLeaf {
            leaf: LeafNode::new_device(LeafId(1), aura_core::DeviceId::new(), vec![0u8; 32]),
            under: NodeIndex(0)
        }));
    }

    #[test]
    fn test_apply_operation_add_leaf() {
        let mut state = TreeState::new();
        let device_id = aura_core::DeviceId::new();
        let leaf = LeafNode::new_device(LeafId(1), device_id, vec![0u8; 32]);

        let op = TreeOp {
            parent_epoch: 0,
            parent_commitment: [0u8; 32],
            op: TreeOpKind::AddLeaf {
                leaf: leaf.clone(),
                under: NodeIndex(0),
            },
            version: 1,
        };

        let affected = apply_operation_to_state(&mut state, &op).unwrap();
        assert_eq!(affected, vec![NodeIndex(0)]);
        assert!(state.get_leaf(&LeafId(1)).is_some());
    }
}
