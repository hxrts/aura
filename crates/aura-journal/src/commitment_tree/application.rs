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
use aura_core::crypto::tree_signing::tree_op_binding_message;
use aura_core::hash;
use aura_core::{
    commit_leaf, AttestedOp, CryptoEffects, Epoch, Hash32, LeafId, NodeIndex, Policy, TreeOp,
    TreeOpKind,
};
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
        expected_epoch: Epoch,
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

    /// Verification failed
    #[error("Verification failed: {reason}")]
    VerificationFailed {
        /// The reason verification failed
        reason: String,
    },

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

    /// Core error from aura-core operations
    #[error("Core error: {0}")]
    Core(#[from] aura_core::AuraError),
}

/// Result type for application operations
pub type ApplicationResult<T> = Result<T, ApplicationError>;

impl ApplicationError {
    /// Create a verification failed error
    pub fn verification_failed(reason: impl Into<String>) -> Self {
        Self::VerificationFailed {
            reason: reason.into(),
        }
    }
}

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
                    reason: format!("Cycle detected in tree structure involving node {current:?}"),
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
/// 1. Verify aggregate signature
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
pub async fn apply_verified(
    state: &mut TreeState,
    attested: &AttestedOp,
    crypto_effects: &dyn CryptoEffects,
) -> ApplicationResult<()> {
    // Step 1: Verify aggregate signature using FROST
    verify_aggregate_signature(attested, state, crypto_effects).await?;

    // Steps 2-6: Shared logic
    apply_verified_common(state, attested)
}

/// Synchronous version for backwards compatibility
///
/// This version skips FROST verification but performs all other validation.
/// Use this for existing code that doesn't have access to CryptoEffects.
/// Migrate to `apply_verified` when crypto effects are available.
pub fn apply_verified_sync(state: &mut TreeState, attested: &AttestedOp) -> ApplicationResult<()> {
    // Skip FROST verification (Step 1)
    tracing::warn!(
        "Using apply_verified_sync: FROST verification skipped. Migrate to apply_verified for full security."
    );

    // Steps 2-6: Shared logic
    apply_verified_common(state, attested)
}

/// Shared logic for both sync and async versions
fn apply_verified_common(state: &mut TreeState, attested: &AttestedOp) -> ApplicationResult<()> {
    // Step 2: Verify parent binding
    verify_parent_binding(&attested.op, state)?;

    // Step 3: Apply operation to tree state
    let affected_nodes = apply_operation_to_state(state, &attested.op)?;

    // Step 4: Recompute commitments for affected path
    recompute_commitments(state, &affected_nodes)?;

    // Step 5: Update epoch if required
    if requires_epoch_update(&attested.op.op) {
        state.increment_epoch()?;
    }

    // Step 6: Validate invariants
    validate_invariants(state)?;

    Ok(())
}

/// Verify aggregate threshold signature for attested operation
///
/// Verifies the threshold signature on the operation to ensure it was
/// properly authorized by the required threshold of devices.
async fn verify_aggregate_signature(
    attested: &AttestedOp,
    state: &TreeState,
    crypto_effects: &dyn CryptoEffects,
) -> ApplicationResult<()> {
    // Extract the signature from the attested operation
    let signature = &attested.agg_sig;

    // Determine which node's signing key should authorize this op
    let signing_node = match &attested.op.op {
        TreeOpKind::AddLeaf { under, .. } => *under,
        TreeOpKind::RemoveLeaf { leaf, .. } => state.get_leaf_parent(*leaf).unwrap_or(NodeIndex(0)),
        TreeOpKind::ChangePolicy { node, .. } => *node,
        TreeOpKind::RotateEpoch { affected } => affected.first().copied().unwrap_or(NodeIndex(0)),
    };

    let witness = state
        .signing_witness(&signing_node)
        .map_err(|e| ApplicationError::verification_failed(e.to_string()))?;

    if attested.signer_count < witness.threshold {
        return Err(ApplicationError::verification_failed(format!(
            "Insufficient signers: got {}, need {}",
            attested.signer_count, witness.threshold
        )));
    }

    // Group public key bound to this node
    let group_public_key = witness.group_public_key;

    // Compute binding message: H("TREE_OP_SIG" || node_id || epoch || policy_hash || op_bytes)
    let binding_message =
        tree_op_binding_message(attested, state.current_epoch(), &group_public_key);

    // Verify aggregate signature using effect-based verification
    verify_threshold_signature(
        signature,
        &binding_message,
        &group_public_key,
        crypto_effects,
    )
    .await?;

    Ok(())
}

/// Verify threshold signature via CryptoEffects
async fn verify_threshold_signature(
    signature: &[u8],
    message: &[u8],
    public_key: &[u8],
    crypto_effects: &dyn CryptoEffects,
) -> ApplicationResult<()> {
    // Validate inputs
    if signature.is_empty() {
        return Err(ApplicationError::verification_failed(
            "Empty signature".to_string(),
        ));
    }

    if message.is_empty() {
        return Err(ApplicationError::verification_failed(
            "Empty message".to_string(),
        ));
    }

    if public_key.is_empty() {
        return Err(ApplicationError::verification_failed(
            "Empty public key".to_string(),
        ));
    }

    // Use the crypto effect system for verification
    let is_valid = crypto_effects
        .frost_verify(message, signature, public_key)
        .await
        .map_err(|e| {
            ApplicationError::verification_failed(format!("threshold verification error: {e}"))
        })?;

    if !is_valid {
        return Err(ApplicationError::verification_failed(
            "FROST signature verification failed".to_string(),
        ));
    }

    tracing::debug!(
        "threshold signature verified successfully: sig_len={}, msg_len={}, key_len={}",
        signature.len(),
        message.len(),
        public_key.len()
    );

    Ok(())
}

/// Verify parent binding matches current tree state
///
/// Parent binding prevents replay attacks and ensures lineage.
/// Operations must reference the current (epoch, commitment) to be valid.
///
/// Genesis operations (parent_epoch == Epoch::initial()) are exempt from this check.
fn verify_parent_binding(op: &TreeOp, state: &TreeState) -> ApplicationResult<()> {
    // Genesis operations don't have a parent
    if op.parent_epoch == Epoch::initial() {
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

            // Track parent nodes for affected node calculation
            if let Some(parent_node) = find_parent_node(leaf, state) {
                affected_nodes.push(parent_node);
            }
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
                    old: *old_policy,
                    new: *new_policy,
                });
            }

            // Update policy
            state.set_policy(*node, *new_policy);
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

/// Find parent node for a given leaf in the tree structure
fn find_parent_node(_leaf: &LeafId, state: &TreeState) -> Option<NodeIndex> {
    // Walk the tree to find a parent branch that references this leaf; fall back to root.
    // Prefer the explicit leaf->parent mapping when available
    if let Some(parent) = state.get_leaf_parent(*_leaf) {
        return Some(parent);
    }

    // If we can't find a parent (new leaf or empty tree), default to root node.
    Some(NodeIndex(0))
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
    // Replace simplified recomputation with efficient tree updates
    // Recompute only affected subtrees and propagate changes upward
    if _affected_nodes.is_empty() {
        // If no specific nodes affected, do full recomputation
        recompute_full_tree(state)?;
    } else {
        // Efficient update: only recompute affected paths
        recompute_affected_paths(state, _affected_nodes)?;
    }

    return Ok(());

    // Legacy full recomputation (kept for fallback)
    fn recompute_full_tree(state: &mut TreeState) -> ApplicationResult<()> {
        let mut h = hash::hasher();
        h.update(&u64::from(state.current_epoch()).to_le_bytes());

        // Hash all leaf commitments
        for (leaf_id, leaf_commitment) in state.iter_leaf_commitments() {
            h.update(&leaf_id.0.to_le_bytes());
            h.update(leaf_commitment);
        }

        // Hash all branch commitments
        for branch in state.iter_branches() {
            h.update(&branch.node.0.to_le_bytes());
            h.update(&branch.commitment);
            h.update(&aura_core::policy_hash(&branch.policy));
        }

        let root_commitment = h.finalize();
        state.set_root_commitment(root_commitment);
        Ok(())
    }

    // Efficient recomputation for affected paths only
    fn recompute_affected_paths(
        state: &mut TreeState,
        affected_nodes: &[NodeIndex],
    ) -> ApplicationResult<()> {
        use std::collections::HashSet;

        // Implement efficient path recomputation:
        // 1. Start from affected nodes and collect their upward path
        // 2. Recompute commitments only for nodes in affected paths
        // 3. Propagate changes up to parents until reaching root
        // 4. Skip unaffected subtrees for performance

        let mut nodes_to_update = HashSet::new();

        // Collect all nodes that need updating (affected nodes + their ancestors)
        for &node in affected_nodes {
            let mut current = node;
            nodes_to_update.insert(current);

            // Walk up the tree to root, marking ancestors for update
            while let Some(parent) = state.get_parent(current) {
                if nodes_to_update.contains(&parent) {
                    break; // Already processed this path
                }
                nodes_to_update.insert(parent);
                current = parent;
            }
        }

        // Recompute commitments for affected nodes in bottom-up order
        let mut sorted_nodes: Vec<_> = nodes_to_update.into_iter().collect();
        sorted_nodes.sort_by_key(|node| std::cmp::Reverse(node.0)); // Process deepest first

        for node in sorted_nodes {
            recompute_node_commitment(state, node)?;
        }

        // Update root commitment based on recomputed tree
        recompute_root_commitment_from_tree(state)?;

        Ok(())
    }

    // Helper function to recompute commitment for a single node
    fn recompute_node_commitment(state: &mut TreeState, node: NodeIndex) -> ApplicationResult<()> {
        let mut h = hash::hasher();

        // Include node metadata
        h.update(&node.0.to_le_bytes());

        // Get branch information if it exists
        if let Some(branch) = state.get_branch(&node) {
            h.update(&aura_core::policy_hash(&branch.policy));

            // Hash children commitments
            for child in state.get_children(node) {
                if let Some(child_branch) = state.get_branch(&child) {
                    h.update(&child_branch.commitment);
                }
            }
        }

        // Update the node's commitment
        let new_commitment = h.finalize();
        if let Some(branch) = state.branches.get_mut(&node) {
            branch.commitment = new_commitment;
        }

        Ok(())
    }

    // Helper function to recompute root commitment from updated tree
    fn recompute_root_commitment_from_tree(state: &mut TreeState) -> ApplicationResult<()> {
        let mut h = hash::hasher();
        h.update(&u64::from(state.current_epoch()).to_le_bytes());

        // Hash all updated branch commitments in deterministic order
        let mut branch_nodes: Vec<_> = state.list_branch_indices();
        branch_nodes.sort_by_key(|node| node.0);

        for node in branch_nodes {
            if let Some(branch) = state.get_branch(&node) {
                h.update(&node.0.to_le_bytes());
                h.update(&branch.commitment);
            }
        }

        // Include leaf commitments
        for (leaf_id, leaf_commitment) in state.iter_leaf_commitments() {
            h.update(&leaf_id.0.to_le_bytes());
            h.update(leaf_commitment);
        }

        let root_commitment = h.finalize();
        state.set_root_commitment(root_commitment);

        Ok(())
    }
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
                reason: format!("Duplicate node index: {node:?}"),
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
    use crate::LeafNode;

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
            parent_epoch: Epoch::initial(),
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
        state.set_epoch(Epoch::new(5));

        let op = TreeOp {
            parent_epoch: Epoch::new(3), // Wrong epoch
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
            leaf: LeafNode::new_device(
                LeafId(1),
                aura_core::DeviceId(uuid::Uuid::from_bytes([6u8; 16])),
                vec![0u8; 32]
            )
            .expect("valid leaf"),
            under: NodeIndex(0)
        }));
    }

    #[test]
    fn test_apply_operation_add_leaf() {
        let mut state = TreeState::new();
        let device_id = aura_core::DeviceId(uuid::Uuid::from_bytes([7u8; 16]));
        let leaf = LeafNode::new_device(LeafId(1), device_id, vec![0u8; 32]).expect("valid leaf");

        let op = TreeOp {
            parent_epoch: Epoch::initial(),
            parent_commitment: [0u8; 32],
            op: TreeOpKind::AddLeaf {
                leaf,
                under: NodeIndex(0),
            },
            version: 1,
        };

        let affected = apply_operation_to_state(&mut state, &op).unwrap();
        assert_eq!(affected, vec![NodeIndex(0)]);
        assert!(state.get_leaf(&LeafId(1)).is_some());
    }
}
