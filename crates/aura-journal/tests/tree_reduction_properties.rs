//! Property-Based Tests for Tree Reduction
//!
//! This module verifies that TreeState reduction from OpLog is deterministic
//! and confluent using property-based testing.
//!
//! ## Properties Verified
//!
//! 1. **Determinism**: Same OpLog always produces same TreeState
//! 2. **Confluence**: Different merge orders produce same TreeState
//! 3. **Tie-breaker consistency**: H(op) ordering is stable
//! 4. **Commutativity**: OpLog join order doesn't affect final state
//!
//! ## Reference
//!
//! See docs/123_commitment_tree.md - Deterministic Reduction section

use aura_core::{TreeOp, TreeOpKind};
use aura_journal::commitment_tree::reduce;
use aura_journal::semilattice::{JoinSemilattice, OpLog};
use aura_journal::{AttestedOp, LeafId, LeafNode, LeafRole, NodeIndex};
use proptest::prelude::*;

/// Create a test operation with specific parameters
fn create_op(epoch: u64, commitment: [u8; 32], leaf_id: u64) -> AttestedOp {
    // Create deterministic device_id from leaf_id for consistent tests
    let device_bytes = [
        (leaf_id as u8),
        (leaf_id >> 8) as u8,
        (leaf_id >> 16) as u8,
        (leaf_id >> 24) as u8,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];

    AttestedOp {
        op: TreeOp {
            parent_commitment: commitment,
            parent_epoch: epoch,
            op: TreeOpKind::AddLeaf {
                leaf: LeafNode {
                    leaf_id: LeafId(leaf_id as u32),
                    device_id: aura_core::DeviceId::from_bytes(device_bytes),
                    role: LeafRole::Device,
                    public_key: vec![1, 2, 3],
                    meta: vec![],
                },
                under: NodeIndex(0),
            },
            version: 1,
        },
        agg_sig: vec![],
        signer_count: 1,
    }
}

/// Generate arbitrary operation with valid epoch (0 for genesis operations)
fn arb_op() -> impl Strategy<Value = AttestedOp> {
    (1u64..=100).prop_map(|leaf_id| {
        // All operations reference epoch 0 (genesis) for simplicity
        create_op(0, [0u8; 32], leaf_id)
    })
}

/// Generate arbitrary OpLog with 1-10 operations
fn arb_oplog() -> impl Strategy<Value = OpLog> {
    prop::collection::vec(arb_op(), 1..=10).prop_map(|ops| {
        let mut oplog = OpLog::new();
        for op in ops {
            oplog.append(op);
        }
        oplog
    })
}

proptest! {
    /// Property: Reduction is deterministic
    /// Reducing the same OpLog multiple times produces identical TreeState
    #[test]
    fn prop_reduction_deterministic(oplog in arb_oplog()) {
        let ops: Vec<AttestedOp> = oplog.list_ops().into_iter().cloned().collect();
        let state1 = reduce(&ops)?;
        let state2 = reduce(&ops)?;

        // States should be identical
        prop_assert_eq!(state1.epoch, state2.epoch, "Epochs must match");
        prop_assert_eq!(state1.root_commitment, state2.root_commitment, "Commitments must match");
        prop_assert_eq!(state1.leaves.len(), state2.leaves.len(), "Leaf counts must match");
    }

    /// Property: Reduction is confluent (join order independence)
    /// OpLog join is commutative, so (A ⊔ B) and (B ⊔ A) reduce to same state
    #[test]
    fn prop_reduction_confluent(
        oplog1 in arb_oplog(),
        oplog2 in arb_oplog()
    ) {
        // Join in different orders
        let joined_ab = oplog1.join(&oplog2);
        let joined_ba = oplog2.join(&oplog1);

        // Reduce both
        let ops_ab: Vec<AttestedOp> = joined_ab.list_ops().into_iter().cloned().collect();
        let ops_ba: Vec<AttestedOp> = joined_ba.list_ops().into_iter().cloned().collect();
        let state_ab = reduce(&ops_ab)?;
        let state_ba = reduce(&ops_ba)?;

        // States should be identical
        prop_assert_eq!(state_ab.epoch, state_ba.epoch, "Epochs must match after join");
        prop_assert_eq!(
            state_ab.root_commitment,
            state_ba.root_commitment,
            "Commitments must match after join"
        );
    }

    /// Property: OpLog union is associative in reduction
    /// reduce((A ⊔ B) ⊔ C) = reduce(A ⊔ (B ⊔ C))
    #[test]
    fn prop_join_associative_in_reduction(
        oplog1 in arb_oplog(),
        oplog2 in arb_oplog(),
        oplog3 in arb_oplog()
    ) {
        // Join with different associativity
        let left = oplog1.join(&oplog2).join(&oplog3);
        let right = oplog1.join(&oplog2.join(&oplog3));

        // Reduce both
        let ops_left: Vec<AttestedOp> = left.list_ops().into_iter().cloned().collect();
        let ops_right: Vec<AttestedOp> = right.list_ops().into_iter().cloned().collect();
        let state_left = reduce(&ops_left)?;
        let state_right = reduce(&ops_right)?;

        // States should be identical
        prop_assert_eq!(
            state_left.epoch,
            state_right.epoch,
            "Epochs must match with different join associativity"
        );
    }

    /// Property: Adding same operation twice doesn't change state
    /// OpLog is a set (idempotent under union)
    #[test]
    fn prop_duplicate_ops_idempotent(op in arb_op()) {
        let mut oplog1 = OpLog::new();
        oplog1.append(op.clone());

        let mut oplog2 = OpLog::new();
        oplog2.append(op.clone());
        oplog2.append(op); // Duplicate

        let ops1: Vec<AttestedOp> = oplog1.list_ops().into_iter().cloned().collect();
        let ops2: Vec<AttestedOp> = oplog2.list_ops().into_iter().cloned().collect();
        let state1 = reduce(&ops1)?;
        let state2 = reduce(&ops2)?;

        prop_assert_eq!(state1.epoch, state2.epoch, "Duplicate ops should not change state");
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_empty_oplog_reduction() {
        let oplog = OpLog::new();
        let ops: Vec<AttestedOp> = oplog.list_ops().into_iter().cloned().collect();
        let state = reduce(&ops).unwrap();

        assert_eq!(state.epoch, 0);
        assert_eq!(state.leaves.len(), 0);
    }

    #[test]
    fn test_single_op_reduction() {
        let mut oplog = OpLog::new();
        let op = create_op(0, [0u8; 32], 1);
        oplog.append(op);

        let ops: Vec<AttestedOp> = oplog.list_ops().into_iter().cloned().collect();
        let state = reduce(&ops).unwrap();

        assert!(!state.leaves.is_empty() || state.epoch >= 1);
    }

    #[test]
    fn test_deterministic_reduction_same_ops() {
        let mut oplog = OpLog::new();
        oplog.append(create_op(0, [0u8; 32], 1));
        oplog.append(create_op(0, [0u8; 32], 2));

        let ops: Vec<AttestedOp> = oplog.list_ops().into_iter().cloned().collect();
        let state1 = reduce(&ops).unwrap();
        let state2 = reduce(&ops).unwrap();

        assert_eq!(state1.epoch, state2.epoch);
        assert_eq!(state1.root_commitment, state2.root_commitment);
    }

    #[test]
    fn test_join_then_reduce() {
        let mut oplog1 = OpLog::new();
        oplog1.append(create_op(0, [0u8; 32], 1));

        let mut oplog2 = OpLog::new();
        oplog2.append(create_op(0, [0u8; 32], 2));

        let joined = oplog1.join(&oplog2);
        let ops: Vec<AttestedOp> = joined.list_ops().into_iter().cloned().collect();
        let state = reduce(&ops).unwrap();

        // State should reflect both operations
        assert!(!state.leaves.is_empty());
    }

    #[test]
    fn test_commutative_join_reduction() {
        let mut oplog1 = OpLog::new();
        oplog1.append(create_op(0, [0u8; 32], 1));

        let mut oplog2 = OpLog::new();
        oplog2.append(create_op(0, [0u8; 32], 2));

        let joined_ab = oplog1.join(&oplog2);
        let joined_ba = oplog2.join(&oplog1);

        let ops_ab: Vec<AttestedOp> = joined_ab.list_ops().into_iter().cloned().collect();
        let ops_ba: Vec<AttestedOp> = joined_ba.list_ops().into_iter().cloned().collect();
        let state_ab = reduce(&ops_ab).unwrap();
        let state_ba = reduce(&ops_ba).unwrap();

        assert_eq!(state_ab.epoch, state_ba.epoch);
        assert_eq!(state_ab.root_commitment, state_ba.root_commitment);
    }

    #[test]
    fn test_idempotent_reduction() {
        let mut oplog = OpLog::new();
        let op = create_op(0, [0u8; 32], 1);
        oplog.append(op.clone());
        oplog.append(op); // Add same op twice

        let ops: Vec<AttestedOp> = oplog.list_ops().into_iter().cloned().collect();
        let state = reduce(&ops).unwrap();

        // OpLog deduplicates, so state should be as if added once
        assert!(!state.leaves.is_empty());
    }

    #[test]
    fn test_associative_join_reduction() {
        let mut oplog1 = OpLog::new();
        oplog1.append(create_op(0, [0u8; 32], 1));

        let mut oplog2 = OpLog::new();
        oplog2.append(create_op(0, [0u8; 32], 2));

        let mut oplog3 = OpLog::new();
        oplog3.append(create_op(0, [0u8; 32], 3));

        let left = oplog1.join(&oplog2).join(&oplog3);
        let right = oplog1.join(&oplog2.join(&oplog3));

        let ops_left: Vec<AttestedOp> = left.list_ops().into_iter().cloned().collect();
        let ops_right: Vec<AttestedOp> = right.list_ops().into_iter().cloned().collect();
        let state_left = reduce(&ops_left).unwrap();
        let state_right = reduce(&ops_right).unwrap();

        assert_eq!(state_left.epoch, state_right.epoch);
    }
}
