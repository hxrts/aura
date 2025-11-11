//! Phase 6 Tests: Authorization Bridge Functionality
//!
//! Tests for the bridge functionality that connects authentication (identity verification)
//! with authorization (capability evaluation) without mixing concerns.

use aura_core::{AccountId, DeviceId, GuardianId};
use aura_crypto::Ed25519SigningKey;
use aura_protocol::authorization_bridge::{
    evaluate_authorization, AuthorizationContext, AuthorizationRequest, AuthorizedEvent,
    PermissionGrant,
};
use aura_verify::{IdentityProof, KeyMaterial, VerifiedIdentity};
use aura_wot::{CapabilitySet, LeafRole, TreeAuthzContext, TreeOp, TreeOpKind};
use std::collections::BTreeSet;

/// Test that authorization bridge correctly connects identity with capability evaluation
#[tokio::test]
async fn test_authorization_bridge_integration() {
    // Create test setup
    let account_id = AccountId::from_bytes([1u8; 32]);
    let device_id = DeviceId::from_bytes([2u8; 32]);

    // Create verified identity (result of authentication)
    let verified_identity = VerifiedIdentity::Device(device_id);

    // Create tree operation
    let tree_op = TreeOp {
        parent_epoch: 1,
        parent_commitment: [0u8; 32],
        op: TreeOpKind::AddLeaf {
            leaf_id: 1,
            role: LeafRole::Device,
            under: 0,
        },
        version: 1,
    };

    // Create authorization context
    let required_capabilities = CapabilitySet::from_permissions(&["tree:write", "tree:propose"]);
    let tree_context = TreeAuthzContext::new(account_id, 1);
    let authz_context = AuthorizationContext::new(account_id, required_capabilities, tree_context);

    // Create authorization request
    let authz_request = AuthorizationRequest {
        verified_identity,
        operation: tree_op,
        context: authz_context,
        additional_signers: BTreeSet::new(),
        guardian_signers: BTreeSet::new(),
    };

    // Evaluate authorization through bridge
    let result = evaluate_authorization(authz_request).unwrap();

    assert!(
        result.authorized,
        "Device with proper capabilities should be authorized"
    );
    assert!(result.denial_reason.is_none());
}

/// Test authorization bridge with insufficient capabilities
#[tokio::test]
async fn test_authorization_bridge_insufficient_capabilities() {
    let account_id = AccountId::from_bytes([3u8; 32]);
    let device_id = DeviceId::from_bytes([4u8; 32]);

    // Create verified identity
    let verified_identity = VerifiedIdentity::Device(device_id);

    // Create tree operation requiring write capabilities
    let tree_op = TreeOp {
        parent_epoch: 1,
        parent_commitment: [0u8; 32],
        op: TreeOpKind::AddLeaf {
            leaf_id: 1,
            role: LeafRole::Device,
            under: 0,
        },
        version: 1,
    };

    // Create authorization context with insufficient capabilities
    let insufficient_capabilities = CapabilitySet::from_permissions(&["tree:read"]);
    let tree_context = TreeAuthzContext::new(account_id, 1);
    let authz_context =
        AuthorizationContext::new(account_id, insufficient_capabilities, tree_context);

    // Create authorization request
    let authz_request = AuthorizationRequest {
        verified_identity,
        operation: tree_op,
        context: authz_context,
        additional_signers: BTreeSet::new(),
        guardian_signers: BTreeSet::new(),
    };

    // Evaluate authorization - should fail
    let result = evaluate_authorization(authz_request).unwrap();

    assert!(
        !result.authorized,
        "Device with insufficient capabilities should not be authorized"
    );
    assert!(result.denial_reason.is_some());
}

/// Test authorization bridge with guardian identity
#[tokio::test]
async fn test_authorization_bridge_guardian_operations() {
    let account_id = AccountId::from_bytes([5u8; 32]);
    let guardian_id = GuardianId::from_bytes([6u8; 32]);

    // Create verified guardian identity
    let verified_identity = VerifiedIdentity::Guardian(guardian_id);

    // Create guardian tree operation
    let tree_op = TreeOp {
        parent_epoch: 1,
        parent_commitment: [0u8; 32],
        op: TreeOpKind::AddLeaf {
            leaf_id: 2,
            role: LeafRole::Guardian,
            under: 0,
        },
        version: 1,
    };

    // Create authorization context with guardian capabilities
    let guardian_capabilities =
        CapabilitySet::from_permissions(&["guardian:manage", "tree:propose"]);
    let tree_context = TreeAuthzContext::new(account_id, 1);
    let authz_context = AuthorizationContext::new(account_id, guardian_capabilities, tree_context);

    // Create authorization request
    let authz_request = AuthorizationRequest {
        verified_identity,
        operation: tree_op,
        context: authz_context,
        additional_signers: BTreeSet::new(),
        guardian_signers: BTreeSet::new(),
    };

    // Evaluate authorization
    let result = evaluate_authorization(authz_request).unwrap();

    assert!(
        result.authorized,
        "Guardian with proper capabilities should be authorized"
    );
}

/// Test authorization context creation and management
#[test]
fn test_authorization_context() {
    let account_id = AccountId::from_bytes([7u8; 32]);
    let capabilities = CapabilitySet::from_permissions(&["storage:read", "storage:write"]);
    let tree_context = TreeAuthzContext::new(account_id, 2);

    let authz_context =
        AuthorizationContext::new(account_id, capabilities.clone(), tree_context.clone());

    assert_eq!(authz_context.account_id, account_id);
    assert_eq!(
        authz_context.required_capabilities.permissions(),
        capabilities.permissions()
    );
    assert_eq!(authz_context.tree_context.account_id(), account_id);
    assert_eq!(authz_context.tree_context.epoch(), 2);
}

/// Test permission grant structure
#[test]
fn test_permission_grant() {
    // Test authorized grant
    let authorized_grant = PermissionGrant::authorized();
    assert!(authorized_grant.authorized);
    assert!(authorized_grant.denial_reason.is_none());

    // Test denied grant
    let denied_grant = PermissionGrant::denied("Insufficient privileges");
    assert!(!denied_grant.authorized);
    assert_eq!(
        denied_grant.denial_reason,
        Some("Insufficient privileges".to_string())
    );
}

/// Test that bridge maintains separation of concerns
#[tokio::test]
async fn test_bridge_separation_of_concerns() {
    // The bridge should not perform identity verification itself
    // It should only consume already-verified identities

    let account_id = AccountId::from_bytes([8u8; 32]);
    let device_id = DeviceId::from_bytes([9u8; 32]);

    // Create verified identity (this would come from authentication layer)
    let verified_identity = VerifiedIdentity::Device(device_id);

    // Create authorization request
    let tree_op = TreeOp {
        parent_epoch: 1,
        parent_commitment: [0u8; 32],
        op: TreeOpKind::AddLeaf {
            leaf_id: 1,
            role: LeafRole::Device,
            under: 0,
        },
        version: 1,
    };

    let capabilities = CapabilitySet::from_permissions(&["tree:write", "tree:propose"]);
    let tree_context = TreeAuthzContext::new(account_id, 1);
    let authz_context = AuthorizationContext::new(account_id, capabilities, tree_context);

    let authz_request = AuthorizationRequest {
        verified_identity,
        operation: tree_op,
        context: authz_context,
        additional_signers: BTreeSet::new(),
        guardian_signers: BTreeSet::new(),
    };

    // Bridge should only perform capability evaluation, not identity verification
    let result = evaluate_authorization(authz_request).unwrap();

    // The fact that this succeeds means the bridge trusts the verified identity
    // and focuses only on capability evaluation
    assert!(result.authorized);
}

/// Test authorized event creation from bridge output
#[test]
fn test_authorized_event_creation() {
    let account_id = AccountId::from_bytes([10u8; 32]);
    let device_id = DeviceId::from_bytes([11u8; 32]);

    let verified_identity = VerifiedIdentity::Device(device_id);
    let permission_grant = PermissionGrant::authorized();

    // Create authorized event
    let authorized_event =
        AuthorizedEvent::new(verified_identity.clone(), permission_grant.clone());

    match authorized_event.verified_identity {
        VerifiedIdentity::Device(id) => assert_eq!(id, device_id),
        _ => panic!("Expected device identity"),
    }

    assert!(authorized_event.permission_grant.authorized);
}

/// Test threshold signature scenarios through bridge
#[tokio::test]
async fn test_bridge_threshold_operations() {
    let account_id = AccountId::from_bytes([12u8; 32]);

    // Create threshold-verified identity
    let verified_identity = VerifiedIdentity::Threshold {
        account_id,
        threshold: 2,
        signers: vec![
            DeviceId::from_bytes([13u8; 32]),
            DeviceId::from_bytes([14u8; 32]),
        ],
    };

    // Create tree operation requiring threshold authorization
    let tree_op = TreeOp {
        parent_epoch: 1,
        parent_commitment: [0u8; 32],
        op: TreeOpKind::RemoveLeaf { leaf_id: 1 },
        version: 1,
    };

    // Create authorization context with threshold capabilities
    let threshold_capabilities = CapabilitySet::from_permissions(&["tree:remove", "tree:propose"]);
    let tree_context = TreeAuthzContext::new(account_id, 1);
    let authz_context = AuthorizationContext::new(account_id, threshold_capabilities, tree_context);

    let authz_request = AuthorizationRequest {
        verified_identity,
        operation: tree_op,
        context: authz_context,
        additional_signers: BTreeSet::new(),
        guardian_signers: BTreeSet::new(),
    };

    // Evaluate authorization
    let result = evaluate_authorization(authz_request).unwrap();

    assert!(
        result.authorized,
        "Threshold identity with proper capabilities should be authorized"
    );
}

/// Test that bridge is stateless
#[tokio::test]
async fn test_bridge_stateless_operation() {
    let account_id = AccountId::from_bytes([15u8; 32]);
    let device_id = DeviceId::from_bytes([16u8; 32]);

    let verified_identity = VerifiedIdentity::Device(device_id);
    let tree_op = TreeOp {
        parent_epoch: 1,
        parent_commitment: [0u8; 32],
        op: TreeOpKind::AddLeaf {
            leaf_id: 1,
            role: LeafRole::Device,
            under: 0,
        },
        version: 1,
    };

    let capabilities = CapabilitySet::from_permissions(&["tree:write", "tree:propose"]);
    let tree_context = TreeAuthzContext::new(account_id, 1);
    let authz_context = AuthorizationContext::new(account_id, capabilities, tree_context);

    let authz_request = AuthorizationRequest {
        verified_identity,
        operation: tree_op,
        context: authz_context,
        additional_signers: BTreeSet::new(),
        guardian_signers: BTreeSet::new(),
    };

    // Multiple evaluations should be independent and identical
    let result1 = evaluate_authorization(authz_request.clone()).unwrap();
    let result2 = evaluate_authorization(authz_request).unwrap();

    assert_eq!(result1.authorized, result2.authorized);
    assert_eq!(result1.denial_reason, result2.denial_reason);
}
