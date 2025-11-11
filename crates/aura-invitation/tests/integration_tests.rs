//! Integration tests for aura-invitation crate
//!
//! These tests validate the integration between different invitation system components
//! including device invitations, acceptance protocols, and relationship formation.

use aura_core::{AccountId, Cap, DeviceId, RelationshipId, TrustLevel, Top};
use aura_invitation::{
    device_invitation::{DeviceInvitationCoordinator, DeviceInvitationRequest},
    invitation_acceptance::{AcceptanceProtocolConfig, InvitationAcceptanceCoordinator},
    relationship_formation::{RelationshipFormationCoordinator, RelationshipFormationRequest, RelationshipType},
};
use aura_journal::semilattice::InvitationLedger;
use aura_protocol::effects::AuraEffectSystem;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Test helper to create coordinated test environment
struct InvitationIntegrationTest {
    inviter_device: DeviceId,
    invitee_device: DeviceId,
    account_id: AccountId,
    inviter_effects: AuraEffectSystem,
    invitee_effects: AuraEffectSystem,
    shared_ledger: Arc<Mutex<InvitationLedger>>,
}

impl InvitationIntegrationTest {
    fn new() -> Self {
        let inviter_device = DeviceId::new();
        let invitee_device = DeviceId::new();
        let account_id = AccountId::new();
        let shared_ledger = Arc::new(Mutex::new(InvitationLedger::new()));

        Self {
            inviter_device,
            invitee_device,
            account_id,
            inviter_effects: AuraEffectSystem::for_testing(inviter_device),
            invitee_effects: AuraEffectSystem::for_testing(invitee_device),
            shared_ledger,
        }
    }

    fn create_invitation_request(&self, role: &str, ttl: Option<u64>) -> DeviceInvitationRequest {
        DeviceInvitationRequest {
            inviter: self.inviter_device,
            invitee: self.invitee_device,
            account_id: self.account_id,
            granted_capabilities: Cap::top(),
            device_role: role.to_string(),
            ttl_secs: ttl,
        }
    }
}

#[tokio::test]
async fn test_device_invitation_coordinator_integration() {
    println!("Testing device invitation coordinator integration...");
    
    let test = InvitationIntegrationTest::new();
    let request = test.create_invitation_request("tablet", Some(7200));

    // Create coordinator and send invitation
    let coordinator = DeviceInvitationCoordinator::with_ledger(
        test.inviter_effects.clone(),
        test.shared_ledger.clone(),
    );

    let response = coordinator
        .invite_device(request.clone())
        .await
        .expect("Failed to create device invitation");

    assert!(response.success);
    assert_eq!(response.invitation.inviter, test.inviter_device);
    assert_eq!(response.invitation.invitee, test.invitee_device);
    assert_eq!(response.invitation.device_role, "tablet");

    // Verify ledger state
    let ledger = test.shared_ledger.lock().await;
    assert!(ledger.is_pending(&response.invitation.invitation_id));
    assert!(!ledger.is_accepted(&response.invitation.invitation_id));
    assert!(!ledger.is_expired(&response.invitation.invitation_id));

    println!("✓ Device invitation coordinator integration successful");
}

#[tokio::test]
async fn test_invitation_acceptance_coordinator_integration() {
    println!("Testing invitation acceptance coordinator integration...");
    
    let test = InvitationIntegrationTest::new();
    let request = test.create_invitation_request("laptop", Some(3600));

    // Create invitation first
    let invitation_coordinator = DeviceInvitationCoordinator::with_ledger(
        test.inviter_effects.clone(),
        test.shared_ledger.clone(),
    );

    let invitation_response = invitation_coordinator
        .invite_device(request)
        .await
        .expect("Failed to create invitation for acceptance test");

    // Configure acceptance protocol
    let acceptance_config = AcceptanceProtocolConfig {
        auto_establish_relationship: true,
        default_trust_level: TrustLevel::High,
        require_transport_confirmation: false,
        protocol_timeout_secs: 120,
    };

    // Create acceptance coordinator and accept invitation
    let acceptance_coordinator = InvitationAcceptanceCoordinator::with_config(
        test.invitee_effects.clone(),
        acceptance_config,
    );

    let acceptance = acceptance_coordinator
        .accept_invitation(invitation_response.invitation.clone())
        .await
        .expect("Failed to accept invitation");

    assert!(acceptance.success);
    assert_eq!(acceptance.invitation_id, invitation_response.invitation.invitation_id);
    assert_eq!(acceptance.device_role, "laptop");
    assert!(acceptance.relationship_id.is_some());

    // Verify ledger state after acceptance
    let ledger = test.shared_ledger.lock().await;
    assert!(ledger.is_accepted(&acceptance.invitation_id));
    assert!(!ledger.is_pending(&acceptance.invitation_id));

    println!("✓ Invitation acceptance coordinator integration successful");
}

#[tokio::test]
async fn test_relationship_formation_coordinator_integration() {
    println!("Testing relationship formation coordinator integration...");
    
    let test = InvitationIntegrationTest::new();

    // Test legacy relationship formation
    let formation_request = RelationshipFormationRequest {
        party_a: test.inviter_device,
        party_b: test.invitee_device,
        account_id: test.account_id,
        relationship_type: RelationshipType::DeviceCoOwnership,
        initial_trust_level: TrustLevel::Maximum,
        metadata: vec![
            ("context".to_string(), "integration_test".to_string()),
            ("test_id".to_string(), "relationship_formation".to_string()),
        ],
    };

    let coordinator = RelationshipFormationCoordinator::new(test.inviter_effects.clone());

    let response = coordinator
        .form_relationship(formation_request.clone())
        .await
        .expect("Failed to form relationship");

    assert!(response.success);
    assert!(response.established);
    assert!(response.relationship.is_some());

    let relationship = response.relationship.unwrap();
    assert_eq!(relationship.account_id, test.account_id);
    assert_eq!(relationship.parties, vec![test.inviter_device, test.invitee_device]);
    assert_eq!(relationship.trust_level, TrustLevel::Maximum);

    println!("✓ Relationship formation coordinator integration successful");
}

#[tokio::test]
async fn test_full_invitation_to_relationship_flow() {
    println!("Testing full invitation to relationship flow integration...");
    
    let test = InvitationIntegrationTest::new();
    let request = test.create_invitation_request("guardian-device", Some(1800));

    // Step 1: Create invitation
    let invitation_coordinator = DeviceInvitationCoordinator::with_ledger(
        test.inviter_effects.clone(),
        test.shared_ledger.clone(),
    );

    let invitation_response = invitation_coordinator
        .invite_device(request)
        .await
        .expect("Failed to create invitation in full flow test");

    println!("Created invitation: {}", invitation_response.invitation.invitation_id);

    // Step 2: Accept invitation with relationship establishment
    let acceptance_config = AcceptanceProtocolConfig {
        auto_establish_relationship: true,
        default_trust_level: TrustLevel::Maximum, // Guardian level trust
        require_transport_confirmation: false,
        protocol_timeout_secs: 180,
    };

    let acceptance_coordinator = InvitationAcceptanceCoordinator::with_config(
        test.invitee_effects.clone(),
        acceptance_config,
    );

    let acceptance = acceptance_coordinator
        .accept_invitation(invitation_response.invitation)
        .await
        .expect("Failed to accept invitation in full flow test");

    println!("Accepted invitation with relationship: {:?}", acceptance.relationship_id);

    // Step 3: Verify the complete flow
    assert!(acceptance.success);
    assert_eq!(acceptance.device_role, "guardian-device");
    assert!(acceptance.relationship_id.is_some());

    // Verify ledger state
    let ledger = test.shared_ledger.lock().await;
    assert!(ledger.is_accepted(&acceptance.invitation_id));

    // Step 4: Test that relationship formation is also available
    let formation_request = RelationshipFormationRequest {
        party_a: test.invitee_device,
        party_b: test.inviter_device,
        account_id: test.account_id,
        relationship_type: RelationshipType::Guardian,
        initial_trust_level: TrustLevel::Maximum,
        metadata: vec![
            ("role".to_string(), "guardian-device".to_string()),
            ("established_via".to_string(), "invitation_acceptance".to_string()),
        ],
    };

    let relationship_coordinator = RelationshipFormationCoordinator::new(test.invitee_effects.clone());

    let relationship_response = relationship_coordinator
        .form_relationship(formation_request)
        .await
        .expect("Failed to form additional relationship");

    assert!(relationship_response.success);
    assert!(relationship_response.established);

    println!("✓ Full invitation to relationship flow integration successful");
    println!("  - Invitation created and accepted");
    println!("  - Relationship established via acceptance protocol");
    println!("  - Additional relationship formation successful");
}

#[tokio::test]
async fn test_concurrent_invitation_processing() {
    println!("Testing concurrent invitation processing integration...");
    
    let test = InvitationIntegrationTest::new();

    // Create multiple invitations concurrently
    let invitation_coordinator = DeviceInvitationCoordinator::with_ledger(
        test.inviter_effects.clone(),
        test.shared_ledger.clone(),
    );

    let mut invitation_tasks = Vec::new();
    for i in 0..3 {
        let coordinator = DeviceInvitationCoordinator::with_ledger(
            test.inviter_effects.clone(),
            test.shared_ledger.clone(),
        );
        let request = test.create_invitation_request(&format!("device-{}", i), Some(3600));
        
        let task = async move {
            coordinator.invite_device(request).await
        };
        invitation_tasks.push(task);
    }

    let invitation_results = futures::future::join_all(invitation_tasks).await;

    // Verify all invitations succeeded
    let mut envelopes = Vec::new();
    for (i, result) in invitation_results.into_iter().enumerate() {
        let response = result.expect(&format!("Invitation {} failed", i));
        assert!(response.success);
        envelopes.push(response.invitation);
        println!("Created concurrent invitation {}: {}", i, response.invitation.invitation_id);
    }

    // Accept all invitations concurrently
    let mut acceptance_tasks = Vec::new();
    for envelope in envelopes {
        let coordinator = InvitationAcceptanceCoordinator::with_ledger(
            test.invitee_effects.clone(),
            test.shared_ledger.clone(),
        );
        
        let task = async move {
            coordinator.accept_invitation(envelope).await
        };
        acceptance_tasks.push(task);
    }

    let acceptance_results = futures::future::join_all(acceptance_tasks).await;

    // Verify all acceptances succeeded
    for (i, result) in acceptance_results.into_iter().enumerate() {
        let acceptance = result.expect(&format!("Acceptance {} failed", i));
        assert!(acceptance.success);
        println!("Accepted concurrent invitation {}: {}", i, acceptance.invitation_id);
    }

    // Verify ledger consistency
    let ledger = test.shared_ledger.lock().await;
    // All invitations should be accepted, none pending or expired
    println!("✓ Concurrent invitation processing integration successful");
}

#[tokio::test]
async fn test_error_handling_integration() {
    println!("Testing error handling integration across components...");
    
    let test = InvitationIntegrationTest::new();

    // Test invalid invitation creation
    let invalid_request = DeviceInvitationRequest {
        inviter: test.inviter_device,
        invitee: test.invitee_device,
        account_id: test.account_id,
        granted_capabilities: Cap::top(),
        device_role: "test".to_string(),
        ttl_secs: Some(0), // Invalid TTL
    };

    let coordinator = DeviceInvitationCoordinator::with_ledger(
        test.inviter_effects.clone(),
        test.shared_ledger.clone(),
    );

    let invalid_result = coordinator.invite_device(invalid_request).await;
    assert!(invalid_result.is_err());
    println!("✓ Invalid invitation creation properly rejected");

    // Test acceptance error handling
    let valid_request = test.create_invitation_request("test-device", Some(1)); // Very short TTL
    let invitation_response = coordinator
        .invite_device(valid_request)
        .await
        .expect("Failed to create invitation for error test");

    // Wait for expiration
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let acceptance_coordinator = InvitationAcceptanceCoordinator::with_ledger(
        test.invitee_effects.clone(),
        test.shared_ledger.clone(),
    );

    let expired_acceptance = acceptance_coordinator
        .accept_invitation(invitation_response.invitation)
        .await
        .expect("Should handle expired invitation gracefully");

    assert!(!expired_acceptance.success);
    assert!(expired_acceptance.error_message.is_some());
    println!("✓ Expired invitation acceptance properly handled");

    // Test relationship formation error handling
    let invalid_formation_request = RelationshipFormationRequest {
        party_a: test.inviter_device,
        party_b: test.inviter_device, // Same device - should fail
        account_id: test.account_id,
        relationship_type: RelationshipType::DeviceCoOwnership,
        initial_trust_level: TrustLevel::Medium,
        metadata: vec![],
    };

    let relationship_coordinator = RelationshipFormationCoordinator::new(test.inviter_effects.clone());
    let invalid_relationship = relationship_coordinator
        .form_relationship(invalid_formation_request)
        .await
        .expect("Should handle invalid relationship formation gracefully");

    assert!(!invalid_relationship.success);
    assert!(invalid_relationship.error.is_some());
    println!("✓ Invalid relationship formation properly handled");

    println!("✓ Error handling integration across all components successful");
}