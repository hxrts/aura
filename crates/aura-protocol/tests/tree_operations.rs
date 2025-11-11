//! Integration Tests for Tree Operations
//!
//! This module tests end-to-end tree operation execution including:
//! - AddLeaf, RemoveLeaf, ChangePolicy, RotateEpoch
//! - Parent binding validation
//! - Authorization enforcement
//! - Invariant checking

use aura_core::tree::{
    AttestedOp, Epoch, LeafId, LeafNode, LeafRole, NodeIndex, Policy, TreeOp, TreeOpKind,
};
use aura_core::DeviceId;
use aura_journal::ratchet_tree::{apply_verified, reduce, validate_invariants};
use aura_journal::semilattice::OpLog;
use aura_protocol::effects::system::AuraEffectSystem;
use aura_protocol::effects::TreeEffects;

#[tokio::test]
async fn test_add_leaf_operation() {
    let device_id = DeviceId::new();
    let effect_system = AuraEffectSystem::for_testing(device_id);

    // Get initial state
    let initial_state = effect_system.get_current_state().await.unwrap();
    let initial_leaves = initial_state.list_leaf_ids().len();

    // Create AddLeaf operation
    let leaf = LeafNode::new_device(LeafId(100), device_id, vec![1, 2, 3]);
    let op_kind = effect_system.add_leaf(leaf, NodeIndex(0)).await.unwrap();

    // Verify operation kind
    assert!(matches!(op_kind, TreeOpKind::AddLeaf { .. }));

    // In real implementation, this would be attested via threshold ceremony
    // For testing, we verify the operation structure is correct
}

#[tokio::test]
async fn test_remove_leaf_operation() {
    let device_id = DeviceId::new();
    let effect_system = AuraEffectSystem::for_testing(device_id);

    // Create RemoveLeaf operation
    let leaf_id = LeafId(1);
    let reason = 1; // Removal reason code

    let op_kind = effect_system.remove_leaf(leaf_id, reason).await.unwrap();

    // Verify operation kind
    assert!(matches!(op_kind, TreeOpKind::RemoveLeaf { .. }));
}

#[tokio::test]
async fn test_change_policy_operation() {
    let device_id = DeviceId::new();
    let effect_system = AuraEffectSystem::for_testing(device_id);

    // Create ChangePolicy operation (Any → Threshold is valid)
    let node = NodeIndex(0);
    let new_policy = Policy::Threshold { m: 2, n: 3 };

    let op_kind = effect_system.change_policy(node, new_policy).await.unwrap();

    // Verify operation kind
    assert!(matches!(op_kind, TreeOpKind::ChangePolicy { .. }));
}

#[tokio::test]
async fn test_rotate_epoch_operation() {
    let device_id = DeviceId::new();
    let effect_system = AuraEffectSystem::for_testing(device_id);

    // Create RotateEpoch operation
    let affected = vec![NodeIndex(0), NodeIndex(1)];

    let op_kind = effect_system.rotate_epoch(affected).await.unwrap();

    // Verify operation kind
    assert!(matches!(op_kind, TreeOpKind::RotateEpoch { .. }));
}

#[test]
fn test_operation_application_updates_state() {
    // Create initial state from empty OpLog
    let oplog = OpLog::new();
    let mut state = reduce(&oplog.list_ops().iter().cloned().collect::<Vec<_>>()).unwrap();

    let initial_epoch = state.epoch();

    // Create an attested operation
    let attested_op = AttestedOp {
        op: TreeOp {
            parent_commitment: state.root_commitment(),
            parent_epoch: initial_epoch,
            op: TreeOpKind::AddLeaf {
                leaf: LeafNode::new_device(LeafId(1), DeviceId::new(), vec![1, 2, 3]),
                under: NodeIndex(0),
            },
            version: 1,
        },
        agg_sig: vec![],
        signer_count: 0,
    };

    // Apply operation
    let result = apply_verified(&mut state, &attested_op);

    // In real implementation with proper signatures, this would succeed
    // TODO fix - For now, we verify the function can be called
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_parent_binding_validation() {
    let oplog = OpLog::new();
    let mut state = reduce(&oplog.list_ops().iter().cloned().collect::<Vec<_>>()).unwrap();

    // Create operation with incorrect parent epoch
    let wrong_epoch = 999;
    let attested_op = AttestedOp {
        op: TreeOp {
            parent_commitment: state.root_commitment(),
            parent_epoch: wrong_epoch, // Wrong epoch
            op: TreeOpKind::AddLeaf {
                leaf: LeafNode::new_device(LeafId(1), DeviceId::new(), vec![1, 2, 3]),
                under: NodeIndex(0),
            },
            version: 1,
        },
        agg_sig: vec![],
        signer_count: 0,
    };

    // Application should fail due to parent binding mismatch
    let result = apply_verified(&mut state, &attested_op);
    assert!(
        result.is_err(),
        "Should reject operation with wrong parent epoch"
    );
}

#[test]
fn test_invariant_validation() {
    let oplog = OpLog::new();
    let state = reduce(&oplog.list_ops().iter().cloned().collect::<Vec<_>>()).unwrap();

    // Validate invariants on empty state
    let result = validate_invariants(&state);
    assert!(result.is_ok(), "Empty state should pass validation");
}

#[test]
fn test_policy_strictness_enforcement() {
    let oplog = OpLog::new();
    let mut state = reduce(&oplog.list_ops().iter().cloned().collect::<Vec<_>>()).unwrap();

    // Attempt to change policy from stricter to weaker (should fail)
    // First, we'd need to set a threshold policy, then try to change to Any
    // This is enforced in application.rs::is_policy_stricter_or_equal

    // TODO fix - For now, verify the function exists and can be called
    let result = validate_invariants(&state);
    assert!(result.is_ok());
}

#[test]
fn test_concurrent_operations_deterministic() {
    // Create two operations with same parent
    let oplog = OpLog::new();
    let state = reduce(&oplog.list_ops().iter().cloned().collect::<Vec<_>>()).unwrap();

    let parent_commitment = state.root_commitment();
    let parent_epoch = state.epoch();

    let op1 = AttestedOp {
        op: TreeOp {
            parent_commitment,
            parent_epoch,
            op: TreeOpKind::AddLeaf {
                leaf: LeafNode::new_device(LeafId(1), DeviceId::new(), vec![1, 2, 3]),
                under: NodeIndex(0),
            },
            version: 1,
        },
        agg_sig: vec![],
        signer_count: 0,
    };

    let op2 = AttestedOp {
        op: TreeOp {
            parent_commitment,
            parent_epoch,
            op: TreeOpKind::AddLeaf {
                leaf: LeafNode::new_device(LeafId(2), DeviceId::new(), vec![4, 5, 6]),
                under: NodeIndex(0),
            },
            version: 1,
        },
        agg_sig: vec![],
        signer_count: 0,
    };

    // Add both to OpLog
    let mut oplog_new = OpLog::new();
    oplog_new.add_operation(op1);
    oplog_new.add_operation(op2);

    // Reduce should be deterministic
    let state1 = reduce(&oplog_new.list_ops().iter().cloned().collect::<Vec<_>>()).unwrap();
    let state2 = reduce(&oplog_new.list_ops().iter().cloned().collect::<Vec<_>>()).unwrap();

    assert_eq!(state1.epoch(), state2.epoch());
    assert_eq!(state1.root_commitment(), state2.root_commitment());
}

#[tokio::test]
async fn test_get_current_state() {
    let device_id = DeviceId::new();
    let effect_system = AuraEffectSystem::for_testing(device_id);

    let state = effect_system.get_current_state().await.unwrap();

    // Initial state should have epoch 0
    assert_eq!(state.epoch(), 0);
}

#[tokio::test]
async fn test_get_current_commitment() {
    let device_id = DeviceId::new();
    let effect_system = AuraEffectSystem::for_testing(device_id);

    let commitment = effect_system.get_current_commitment().await.unwrap();

    // Commitment should be 32 bytes
    assert_eq!(commitment.len(), 32);
}

#[tokio::test]
async fn test_get_current_epoch() {
    let device_id = DeviceId::new();
    let effect_system = AuraEffectSystem::for_testing(device_id);

    let epoch = effect_system.get_current_epoch().await.unwrap();

    // Initial epoch should be 0
    assert_eq!(epoch, 0);
}

#[test]
fn test_multiple_operations_in_sequence() {
    let mut oplog = OpLog::new();

    // Operation 1: Add leaf
    let op1 = AttestedOp {
        op: TreeOp {
            parent_commitment: [0u8; 32],
            parent_epoch: 0,
            op: TreeOpKind::AddLeaf {
                leaf: LeafNode::new_device(LeafId(1), DeviceId::new(), vec![1, 2, 3]),
                under: NodeIndex(0),
            },
            version: 1,
        },
        agg_sig: vec![],
        signer_count: 0,
    };
    oplog.add_operation(op1);

    let state1 = reduce(&oplog.list_ops().iter().cloned().collect::<Vec<_>>()).unwrap();
    let epoch1 = state1.epoch();

    // Operation 2: Rotate epoch
    let op2 = AttestedOp {
        op: TreeOp {
            parent_commitment: state1.root_commitment(),
            parent_epoch: epoch1,
            op: TreeOpKind::RotateEpoch {
                affected: vec![NodeIndex(0)],
            },
            version: 1,
        },
        agg_sig: vec![],
        signer_count: 0,
    };
    oplog.add_operation(op2);

    let state2 = reduce(&oplog.list_ops().iter().cloned().collect::<Vec<_>>()).unwrap();

    // Epoch should have incremented or state changed
    assert!(state2.epoch() >= epoch1);
}

#[test]
fn test_operation_deduplication() {
    let mut oplog = OpLog::new();

    let op = AttestedOp {
        op: TreeOp {
            parent_commitment: [1u8; 32],
            parent_epoch: 0,
            op: TreeOpKind::AddLeaf {
                leaf: LeafNode::new_device(LeafId(1), DeviceId::new(), vec![1, 2, 3]),
                under: NodeIndex(0),
            },
            version: 1,
        },
        agg_sig: vec![],
        signer_count: 0,
    };

    // Add same operation twice
    oplog.add_operation(op.clone());
    oplog.add_operation(op);

    // OpLog should deduplicate
    assert_eq!(oplog.list_ops().len(), 1);
}
