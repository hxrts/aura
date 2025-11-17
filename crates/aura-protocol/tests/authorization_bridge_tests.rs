//! Phase 6 Tests: Authorization Bridge Functionality
//!
//! Tests for the bridge functionality that connects authentication (identity verification)
//! with authorization (capability evaluation) without mixing concerns.

use aura_core::{AccountId, DeviceId, GuardianId};
use aura_protocol::authorization_bridge::{
    AuthorizationContext, AuthorizationMetadata, AuthorizationRequest, AuthorizationService,
    AuthorizedEvent, PermissionGrant,
};
use aura_verify::Ed25519Signature;
use aura_verify::{IdentityProof, VerifiedIdentity};
use aura_wot::{CapabilitySet, LeafRole, TreeAuthzContext, TreeOp, TreeOpKind};
use std::collections::BTreeSet;

/// Test that authorization bridge correctly connects identity with capability evaluation
#[tokio::test]
async fn test_authorization_bridge_integration() {
    // Create test setup
    let account_id = AccountId::from_bytes([1u8; 32]);
    let device_id = DeviceId::from_bytes([2u8; 32]);

    // Create verified identity (result of authentication)
    let signature = Ed25519Signature::from_slice(&[0u8; 64]).unwrap();
    let verified_identity = VerifiedIdentity {
        proof: IdentityProof::Device {
            device_id,
            signature,
        },
        message_hash: [0u8; 32],
    };

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
    let required_capabilities =
        CapabilitySet::from_permissions(&["tree:read", "tree:propose", "tree:modify"]);
    let mut tree_context = TreeAuthzContext::new(account_id, 1);

    // Add a tree policy for node 0 that allows the operation
    let participants = std::collections::BTreeSet::from([device_id]);
    let threshold_config = aura_wot::ThresholdConfig::new(1, participants);
    let tree_policy = aura_wot::TreePolicy::new(
        aura_wot::NodeIndex(0),
        account_id,
        aura_wot::tree_policy::Policy::Any,
        threshold_config,
    )
    .with_required_capabilities(CapabilitySet::from_permissions(&[
        "tree:read",
        "tree:propose",
        "tree:modify",
    ]));

    tree_context.add_policy(0, tree_policy);

    let authz_context = AuthorizationContext::new(account_id, required_capabilities, tree_context);
    let service = AuthorizationService::new();

    // Create authorization request
    let authz_request = AuthorizationRequest {
        verified_identity,
        operation: tree_op,
        context: authz_context,
        additional_signers: BTreeSet::new(),
        guardian_signers: BTreeSet::new(),
        metadata: AuthorizationMetadata::default(),
    };

    // Evaluate authorization through bridge
    let result = service.authorize(authz_request).unwrap();

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
    let signature = Ed25519Signature::from_slice(&[1u8; 64]).unwrap();
    let verified_identity = VerifiedIdentity {
        proof: IdentityProof::Device {
            device_id,
            signature,
        },
        message_hash: [0u8; 32],
    };

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
    let mut tree_context = TreeAuthzContext::new(account_id, 1);

    // Add a tree policy for node 0 that requires full capabilities
    let participants = std::collections::BTreeSet::from([device_id]);
    let threshold_config = aura_wot::ThresholdConfig::new(1, participants);
    let tree_policy = aura_wot::TreePolicy::new(
        aura_wot::NodeIndex(0),
        account_id,
        aura_wot::tree_policy::Policy::Any,
        threshold_config,
    )
    .with_required_capabilities(CapabilitySet::from_permissions(&[
        "tree:read",
        "tree:propose",
        "tree:modify",
    ]));

    tree_context.add_policy(0, tree_policy);

    let authz_context =
        AuthorizationContext::new(account_id, insufficient_capabilities, tree_context);
    let service = AuthorizationService::new();

    // Create authorization request
    let authz_request = AuthorizationRequest {
        verified_identity,
        operation: tree_op,
        context: authz_context,
        additional_signers: BTreeSet::new(),
        guardian_signers: BTreeSet::new(),
        metadata: AuthorizationMetadata::default(),
    };

    // Evaluate authorization - should fail
    let result = service.authorize(authz_request).unwrap();

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
    let guardian_id = GuardianId::new();

    // Create verified guardian identity
    let signature = Ed25519Signature::from_slice(&[0u8; 64]).unwrap();
    let verified_identity = VerifiedIdentity {
        proof: IdentityProof::Guardian {
            guardian_id,
            signature,
        },
        message_hash: [0u8; 32],
    };

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
    let guardian_capabilities = CapabilitySet::from_permissions(&[
        "guardian:manage",
        "tree:propose",
        "tree:read",
        "tree:modify",
    ]);
    let mut tree_context = TreeAuthzContext::new(account_id, 1);

    // Add a tree policy for node 0 that allows guardian operations
    let participants = std::collections::BTreeSet::new(); // Guardians use different signing model
    let threshold_config = aura_wot::ThresholdConfig::new(1, participants);
    let tree_policy = aura_wot::TreePolicy::new(
        aura_wot::NodeIndex(0),
        account_id,
        aura_wot::tree_policy::Policy::Any,
        threshold_config,
    )
    .with_required_capabilities(CapabilitySet::from_permissions(&[
        "tree:read",
        "tree:propose",
        "tree:modify",
    ]));

    tree_context.add_policy(0, tree_policy);

    let authz_context = AuthorizationContext::new(account_id, guardian_capabilities, tree_context);
    let service = AuthorizationService::new();

    // Create authorization request
    let authz_request = AuthorizationRequest {
        verified_identity,
        operation: tree_op,
        context: authz_context,
        additional_signers: BTreeSet::new(),
        guardian_signers: BTreeSet::from([guardian_id]), // Add guardian as guardian signer
        metadata: AuthorizationMetadata::default(),
    };

    // Evaluate authorization
    let result = service.authorize(authz_request).unwrap();

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
    // Note: Capabilities comparison would be implementation-specific
    // For now, we just verify the context was created correctly
    assert_eq!(authz_context.tree_context.account_id, account_id);
    assert_eq!(authz_context.tree_context.current_epoch, 2);
}

/// Test permission grant structure
#[test]
fn test_permission_grant() {
    // Test authorized grant
    let device_id = DeviceId::from_bytes([99u8; 32]);
    let signature = Ed25519Signature::from_slice(&[99u8; 64]).unwrap();
    let identity = VerifiedIdentity {
        proof: IdentityProof::Device {
            device_id,
            signature,
        },
        message_hash: [0u8; 32],
    };
    let capabilities = CapabilitySet::from_permissions(&["test"]);
    let authorized_grant = PermissionGrant::granted(capabilities, identity.clone());
    assert!(authorized_grant.authorized);
    assert!(authorized_grant.denial_reason.is_none());

    // Test denied grant
    let denied_grant =
        PermissionGrant::denied("Insufficient privileges".to_string(), identity.clone());
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
    let signature = Ed25519Signature::from_slice(&[2u8; 64]).unwrap();
    let verified_identity = VerifiedIdentity {
        proof: IdentityProof::Device {
            device_id,
            signature,
        },
        message_hash: [0u8; 32],
    };

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

    let capabilities =
        CapabilitySet::from_permissions(&["tree:read", "tree:propose", "tree:modify"]);
    let mut tree_context = TreeAuthzContext::new(account_id, 1);

    // Add a tree policy for node 0
    let participants = std::collections::BTreeSet::from([device_id]);
    let threshold_config = aura_wot::ThresholdConfig::new(1, participants);
    let tree_policy = aura_wot::TreePolicy::new(
        aura_wot::NodeIndex(0),
        account_id,
        aura_wot::tree_policy::Policy::Any,
        threshold_config,
    )
    .with_required_capabilities(CapabilitySet::from_permissions(&[
        "tree:read",
        "tree:propose",
        "tree:modify",
    ]));

    tree_context.add_policy(0, tree_policy);

    let authz_context = AuthorizationContext::new(account_id, capabilities, tree_context);
    let service = AuthorizationService::new();

    let authz_request = AuthorizationRequest {
        verified_identity,
        operation: tree_op,
        context: authz_context,
        additional_signers: BTreeSet::from([device_id]), // Add the device as a signer
        guardian_signers: BTreeSet::new(),
        metadata: AuthorizationMetadata::default(),
    };

    // Bridge should only perform capability evaluation, not identity verification
    let result = service.authorize(authz_request).unwrap();

    // The fact that this succeeds means the bridge trusts the verified identity
    // and focuses only on capability evaluation
    assert!(result.authorized);
}

/// Test authorized event creation from bridge output
#[test]
fn test_authorized_event_creation() {
    let _account_id = AccountId::from_bytes([10u8; 32]);
    let device_id = DeviceId::from_bytes([11u8; 32]);

    let signature = Ed25519Signature::from_slice(&[3u8; 64]).unwrap();
    let verified_identity = VerifiedIdentity {
        proof: IdentityProof::Device {
            device_id,
            signature,
        },
        message_hash: [0u8; 32],
    };
    let capabilities = CapabilitySet::from_permissions(&["test"]);
    let permission_grant = PermissionGrant::granted(capabilities, verified_identity.clone());

    // Create a mock tree operation for the authorized event
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

    // Create authorized event
    let authorized_event =
        AuthorizedEvent::new(verified_identity.clone(), permission_grant.clone(), tree_op);

    match &authorized_event.identity_proof.proof {
        IdentityProof::Device { device_id: id, .. } => assert_eq!(id, &device_id),
        _ => panic!("Expected device identity"),
    }

    assert!(authorized_event.permission_grant.authorized);
}

/// Test threshold signature scenarios through bridge
/// TODO: Fix threshold identity support - requires account context integration
#[tokio::test]
#[ignore = "Threshold identity requires additional account context - infrastructure incomplete"]
async fn test_bridge_threshold_operations() {
    let account_id = AccountId::from_bytes([12u8; 32]);

    // Create threshold-verified identity
    let threshold_sig = aura_verify::ThresholdSig {
        signature: Ed25519Signature::from_slice(&[4u8; 64]).unwrap(),
        signers: vec![0u8, 1u8], // Device indices that signed
        signature_shares: vec![vec![0u8; 32], vec![1u8; 32]], // Mock signature shares
    };
    let verified_identity = VerifiedIdentity {
        proof: IdentityProof::Threshold(threshold_sig),
        message_hash: [0u8; 32],
    };

    // Create tree operation requiring threshold authorization
    let tree_op = TreeOp {
        parent_epoch: 1,
        parent_commitment: [0u8; 32],
        op: TreeOpKind::RemoveLeaf {
            leaf_id: 0,
            reason: 0,
        },
        version: 1,
    };

    // Create authorization context with threshold capabilities
    let threshold_capabilities =
        CapabilitySet::from_permissions(&["tree:read", "tree:propose", "tree:modify"]);
    let mut tree_context = TreeAuthzContext::new(account_id, 1);

    // Add a tree policy for node 0 for threshold operations
    let participants = std::collections::BTreeSet::new(); // Threshold operations handle signers separately
    let threshold_config = aura_wot::ThresholdConfig::new(1, participants);
    let tree_policy = aura_wot::TreePolicy::new(
        aura_wot::NodeIndex(0),
        account_id,
        aura_wot::tree_policy::Policy::Any,
        threshold_config,
    )
    .with_required_capabilities(CapabilitySet::from_permissions(&[
        "tree:read",
        "tree:propose",
        "tree:modify",
    ]));

    tree_context.add_policy(0, tree_policy);

    let authz_context = AuthorizationContext::new(account_id, threshold_capabilities, tree_context);
    let service = AuthorizationService::new();

    let authz_request = AuthorizationRequest {
        verified_identity,
        operation: tree_op,
        context: authz_context,
        additional_signers: BTreeSet::new(),
        guardian_signers: BTreeSet::new(),
        metadata: AuthorizationMetadata::default(),
    };

    // Evaluate authorization
    println!("About to evaluate authorization for threshold test...");
    let result = match service.authorize(authz_request) {
        Ok(grant) => {
            println!("Authorization successful: {:?}", grant);
            grant
        }
        Err(e) => {
            println!("Authorization failed with error: {:?}", e);
            panic!("Authorization failed: {}", e);
        }
    };

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

    let signature = Ed25519Signature::from_slice(&[5u8; 64]).unwrap();
    let verified_identity = VerifiedIdentity {
        proof: IdentityProof::Device {
            device_id,
            signature,
        },
        message_hash: [0u8; 32],
    };
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

    let capabilities =
        CapabilitySet::from_permissions(&["tree:read", "tree:propose", "tree:modify"]);
    let mut tree_context = TreeAuthzContext::new(account_id, 1);

    // Add a tree policy for node 0
    let participants = std::collections::BTreeSet::from([device_id]);
    let threshold_config = aura_wot::ThresholdConfig::new(1, participants);
    let tree_policy = aura_wot::TreePolicy::new(
        aura_wot::NodeIndex(0),
        account_id,
        aura_wot::tree_policy::Policy::Any,
        threshold_config,
    )
    .with_required_capabilities(CapabilitySet::from_permissions(&[
        "tree:read",
        "tree:propose",
        "tree:modify",
    ]));

    tree_context.add_policy(0, tree_policy);

    let authz_context = AuthorizationContext::new(account_id, capabilities, tree_context);
    let service = AuthorizationService::new();

    let authz_request = AuthorizationRequest {
        verified_identity,
        operation: tree_op,
        context: authz_context,
        additional_signers: BTreeSet::from([device_id]), // Add the device as a signer
        guardian_signers: BTreeSet::new(),
        metadata: AuthorizationMetadata::default(),
    };

    // Multiple evaluations should be independent and identical
    let result1 = service.authorize(authz_request.clone()).unwrap();
    let result2 = service.authorize(authz_request).unwrap();

    assert_eq!(result1.authorized, result2.authorized);
    assert_eq!(result1.denial_reason, result2.denial_reason);
}
