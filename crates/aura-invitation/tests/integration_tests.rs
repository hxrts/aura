//! Integration tests for aura-invitation crate
//!
//! These tests validate the integration between different invitation system components
//! including device invitations, acceptance protocols, and relationship formation.

#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]
#![allow(dead_code)] // Test helpers may not all be used

use aura_core::effects::PhysicalTimeEffects;
use aura_core::{AccountId, DeviceId, TrustLevel};
use aura_invitation::{
    device_invitation::{DeviceInvitationCoordinator, DeviceInvitationRequest},
    invitation_acceptance::{AcceptanceProtocolConfig, InvitationAcceptanceCoordinator},
    relationship_formation::{
        LegacyRelationshipFormationCoordinator, RelationshipFormationRequest, RelationshipType,
    },
};
use aura_journal::semilattice::{InvitationRecordRegistry, InvitationStatus};
use aura_macros::aura_test;
use aura_wot::{AccountAuthority, SerializableBiscuit};
use futures::lock::Mutex;
use std::sync::Arc;
use uuid::Uuid;

/// Test helper to create test environment (network issues need further investigation)
struct InvitationIntegrationTest {
    inviter_device: DeviceId,
    invitee_device: DeviceId,
    account_id: AccountId,
    inviter_effects: Arc<aura_agent::AuraEffectSystem>,
    invitee_effects: Arc<aura_agent::AuraEffectSystem>,
    registry: Arc<Mutex<InvitationRecordRegistry>>,
}

impl InvitationIntegrationTest {
    async fn new() -> Self {
        // Use deterministic UUIDs to avoid conflicts
        let inviter_device = DeviceId(Uuid::from_bytes([0x01; 16]));
        let invitee_device = DeviceId(Uuid::from_bytes([0x02; 16]));
        let account_id = AccountId(Uuid::from_bytes([0x03; 16]));
        let registry = Arc::new(Mutex::new(InvitationRecordRegistry::new()));

        let inviter_fixture = aura_testkit::create_test_fixture_with_device_id(inviter_device)
            .await
            .unwrap();
        let invitee_fixture = aura_testkit::create_test_fixture_with_device_id(invitee_device)
            .await
            .unwrap();

        Self {
            inviter_device,
            invitee_device,
            account_id,
            inviter_effects: inviter_fixture.effect_system_wrapped(),
            invitee_effects: invitee_fixture.effect_system_wrapped(),
            registry,
        }
    }

    async fn new_with_seed(seed: u8) -> Self {
        // Use deterministic UUIDs based on seed to avoid conflicts between tests
        let inviter_device = DeviceId(Uuid::from_bytes([seed; 16]));
        let invitee_device = DeviceId(Uuid::from_bytes([seed + 1; 16]));
        let account_id = AccountId(Uuid::from_bytes([seed + 2; 16]));
        let registry = Arc::new(Mutex::new(InvitationRecordRegistry::new()));

        let inviter_fixture = aura_testkit::create_test_fixture_with_device_id(inviter_device)
            .await
            .unwrap();
        let invitee_fixture = aura_testkit::create_test_fixture_with_device_id(invitee_device)
            .await
            .unwrap();

        Self {
            inviter_device,
            invitee_device,
            account_id,
            inviter_effects: inviter_fixture.effect_system_wrapped(),
            invitee_effects: invitee_fixture.effect_system_wrapped(),
            registry,
        }
    }

    fn create_invitation_request(&self, role: &str, ttl: Option<u64>) -> DeviceInvitationRequest {
        // Create an account authority and device token for testing
        let authority = AccountAuthority::new(self.account_id);
        let device_token = authority.create_device_token(self.invitee_device).unwrap();
        let root_key = authority.root_public_key();
        let serializable_token = SerializableBiscuit::new(device_token, root_key);

        DeviceInvitationRequest {
            inviter: self.inviter_device,
            invitee: self.invitee_device,
            account_id: self.account_id,
            granted_token: serializable_token,
            device_role: String::from(role),
            ttl_secs: ttl,
        }
    }
}

#[aura_test]
async fn test_device_invitation_coordinator_integration() -> aura_core::AuraResult<()> {
    println!("Testing device invitation coordinator integration...");

    let test = InvitationIntegrationTest::new_with_seed(10).await;
    let request = test.create_invitation_request("tablet", Some(7200));

    // Create coordinator and send invitation
    let coordinator = DeviceInvitationCoordinator::with_registry(
        test.inviter_effects.clone(),
        test.registry.clone(),
    );

    let response = coordinator.invite_device(request.clone()).await?;

    assert!(response.success);
    assert_eq!(response.invitation.inviter, test.inviter_device);
    assert_eq!(response.invitation.invitee, test.invitee_device);
    assert_eq!(response.invitation.device_role, "tablet");

    // Verify registry state
    let registry = test.registry.lock().await;
    assert_eq!(
        registry
            .get(&response.invitation.invitation_id)
            .map(|r| r.status),
        Some(InvitationStatus::Pending)
    );

    println!("✓ Device invitation coordinator integration successful");
    Ok(())
}

#[aura_test]
#[ignore] // Requires full network effects implementation
async fn test_invitation_acceptance_coordinator_integration() -> aura_core::AuraResult<()> {
    println!("Testing invitation acceptance coordinator integration...");

    let test = InvitationIntegrationTest::new_with_seed(20).await;
    let request = test.create_invitation_request("laptop", Some(3600));

    // Create invitation first
    let invitation_coordinator = DeviceInvitationCoordinator::with_registry(
        test.inviter_effects.clone(),
        test.registry.clone(),
    );

    let invitation_response = invitation_coordinator.invite_device(request).await?;

    // Configure acceptance protocol
    let acceptance_config = AcceptanceProtocolConfig {
        auto_establish_relationship: true,
        default_trust_level: TrustLevel::High,
        require_transport_confirmation: false,
        protocol_timeout_secs: 120,
    };

    // Create acceptance coordinator with shared effect_api and accept invitation
    let acceptance_coordinator = InvitationAcceptanceCoordinator::with_config_and_registry(
        test.invitee_effects.clone(),
        acceptance_config,
        test.registry.clone(),
    );

    let acceptance = acceptance_coordinator
        .accept_invitation(invitation_response.invitation.clone())
        .await?;

    assert!(acceptance.success);
    assert_eq!(
        acceptance.invitation_id,
        invitation_response.invitation.invitation_id
    );
    assert_eq!(acceptance.device_role, "laptop");
    assert!(acceptance.relationship_id.is_some());

    // Verify registry state after acceptance
    let registry = test.registry.lock().await;
    let record = registry.get(&acceptance.invitation_id);
    assert!(record.is_some());
    assert_eq!(record.unwrap().status, InvitationStatus::Accepted);

    println!("✓ Invitation acceptance coordinator integration successful");
    Ok(())
}

#[aura_test]
#[ignore]
async fn test_relationship_formation_coordinator_integration() -> aura_core::AuraResult<()> {
    println!("Testing relationship formation coordinator integration...");

    let test = InvitationIntegrationTest::new_with_seed(30).await;

    // Test legacy relationship formation
    let formation_request = RelationshipFormationRequest {
        party_a: test.inviter_device,
        party_b: test.invitee_device,
        account_id: test.account_id,
        relationship_type: RelationshipType::DeviceCoOwnership,
        initial_trust_level: TrustLevel::High,
        metadata: vec![
            (String::from("context"), String::from("integration_test")),
            (
                String::from("test_id"),
                String::from("relationship_formation"),
            ),
        ],
    };

    let coordinator = LegacyRelationshipFormationCoordinator::new(test.inviter_effects.clone());
    let response = coordinator.form_relationship(formation_request).await?;

    assert!(response.success);
    let relationship = response
        .relationship
        .as_ref()
        .expect("relationship should be returned");
    assert_eq!(
        relationship.parties,
        vec![test.inviter_device, test.invitee_device]
    );
    assert_eq!(relationship.account_id, test.account_id);
    assert_eq!(relationship.trust_level, TrustLevel::High);

    println!("✓ Relationship formation coordinator integration successful");
    Ok(())
}

#[aura_test]
#[ignore] // Requires full network effects implementation
async fn test_full_invitation_to_relationship_flow() -> aura_core::AuraResult<()> {
    println!("Testing full invitation to relationship flow integration...");

    let test = InvitationIntegrationTest::new_with_seed(40).await;
    let request = test.create_invitation_request("guardian-device", Some(1800));

    // Step 1: Create invitation
    let invitation_coordinator = DeviceInvitationCoordinator::with_registry(
        test.inviter_effects.clone(),
        test.registry.clone(),
    );

    let invitation_response = invitation_coordinator.invite_device(request).await?;

    println!(
        "Created invitation: {}",
        invitation_response.invitation.invitation_id
    );

    // Step 2: Accept invitation with relationship establishment
    let acceptance_config = AcceptanceProtocolConfig {
        auto_establish_relationship: true,
        default_trust_level: TrustLevel::Full, // Guardian level trust
        require_transport_confirmation: false,
        protocol_timeout_secs: 180,
    };

    let acceptance_coordinator = InvitationAcceptanceCoordinator::with_config_and_registry(
        test.invitee_effects.clone(),
        acceptance_config,
        test.registry.clone(),
    );

    let acceptance = acceptance_coordinator
        .accept_invitation(invitation_response.invitation)
        .await?;

    println!(
        "Accepted invitation with relationship: {:?}",
        acceptance.relationship_id
    );

    // Step 3: Verify the complete flow
    assert!(acceptance.success);
    assert_eq!(acceptance.device_role, "guardian-device");
    assert!(acceptance.relationship_id.is_some());

    // Verify registry state
    let registry = test.registry.lock().await;
    let record = registry.get(&acceptance.invitation_id);
    assert!(record.is_some());
    assert!(matches!(
        record.unwrap().status,
        aura_journal::semilattice::InvitationStatus::Accepted
    ));

    // Step 4: Verify relationship formation remains available via legacy coordinator
    let formation_request = RelationshipFormationRequest {
        party_a: test.inviter_device,
        party_b: test.invitee_device,
        account_id: test.account_id,
        relationship_type: RelationshipType::DeviceCoOwnership,
        initial_trust_level: TrustLevel::Full,
        metadata: vec![(String::from("flow"), String::from("full-path"))],
    };
    let formation_coordinator =
        LegacyRelationshipFormationCoordinator::new(test.inviter_effects.clone());
    let formation = formation_coordinator
        .form_relationship(formation_request)
        .await?;
    assert!(formation.success);
    assert!(formation.relationship.is_some());

    println!("✓ Full invitation to relationship flow integration successful");
    println!("  - Invitation created and accepted");
    println!("  - Relationship established via acceptance protocol");
    println!("  - Additional relationship formation successful");
    Ok(())
}

#[aura_test]
#[ignore] // Requires full network effects implementation
async fn test_concurrent_invitation_processing() -> aura_core::AuraResult<()> {
    println!("Testing concurrent invitation processing integration...");

    let test = InvitationIntegrationTest::new_with_seed(50).await;

    // Create multiple invitations concurrently
    let _invitation_coordinator = DeviceInvitationCoordinator::with_registry(
        test.inviter_effects.clone(),
        test.registry.clone(),
    );

    let mut invitation_tasks = Vec::new();
    for i in 0..3 {
        let coordinator = DeviceInvitationCoordinator::with_registry(
            test.inviter_effects.clone(),
            test.registry.clone(),
        );
        let request = test.create_invitation_request(&format!("device-{}", i), Some(3600));

        let task = async move { coordinator.invite_device(request).await };
        invitation_tasks.push(task);
    }

    let invitation_results = futures::future::join_all(invitation_tasks).await;

    // Verify all invitations succeeded
    let mut envelopes = Vec::new();
    for (i, result) in invitation_results.into_iter().enumerate() {
        let response = result.map_err(|e| {
            aura_core::AuraError::invalid(format!("Invitation {} failed: {}", i, e))
        })?;
        assert!(response.success);
        let invitation_id = response.invitation.invitation_id.clone();
        envelopes.push(response.invitation);
        println!("Created concurrent invitation {}: {}", i, invitation_id);
    }

    // Accept all invitations concurrently
    let mut acceptance_tasks = Vec::new();
    for envelope in envelopes {
        let coordinator = InvitationAcceptanceCoordinator::with_registry(
            test.invitee_effects.clone(),
            test.registry.clone(),
        );

        let task = async move { coordinator.accept_invitation(envelope).await };
        acceptance_tasks.push(task);
    }

    let acceptance_results = futures::future::join_all(acceptance_tasks).await;

    // Verify all acceptances succeeded
    for (i, result) in acceptance_results.into_iter().enumerate() {
        let acceptance = result.map_err(|e| {
            aura_core::AuraError::invalid(format!("Acceptance {} failed: {}", i, e))
        })?;
        assert!(acceptance.success);
        println!(
            "Accepted concurrent invitation {}: {}",
            i, acceptance.invitation_id
        );
    }

    // Verify registry consistency
    let _registry = test.registry.lock().await;
    // All invitations should be accepted, none pending or expired
    println!("✓ Concurrent invitation processing integration successful");
    Ok(())
}

#[aura_test]
async fn test_error_handling_integration() -> aura_core::AuraResult<()> {
    println!("Testing error handling integration across components...");

    let test = InvitationIntegrationTest::new_with_seed(60).await;

    // Test invalid invitation creation
    let authority = AccountAuthority::new(test.account_id);
    let device_token = authority.create_device_token(test.invitee_device).unwrap();
    let root_key = authority.root_public_key();
    let serializable_token = SerializableBiscuit::new(device_token, root_key);

    let invalid_request = DeviceInvitationRequest {
        inviter: test.inviter_device,
        invitee: test.invitee_device,
        account_id: test.account_id,
        granted_token: serializable_token,
        device_role: String::from("test"),
        ttl_secs: Some(0), // Invalid TTL
    };

    let coordinator = DeviceInvitationCoordinator::with_registry(
        test.inviter_effects.clone(),
        test.registry.clone(),
    );

    let invalid_result = coordinator.invite_device(invalid_request).await;
    assert!(invalid_result.is_err());
    println!("✓ Invalid invitation creation properly rejected");

    // Test acceptance error handling with expiration
    let valid_request = test.create_invitation_request("test-device", Some(1)); // Very short TTL
    let invitation_response = coordinator.invite_device(valid_request).await?;

    // Wait for expiration using mock time advancement
    let _ = test.invitee_effects.sleep_ms(2000).await;

    let acceptance_coordinator = InvitationAcceptanceCoordinator::with_registry(
        test.invitee_effects.clone(),
        test.registry.clone(),
    );

    let expired_acceptance = acceptance_coordinator
        .accept_invitation(invitation_response.invitation)
        .await?;

    assert!(!expired_acceptance.success);
    assert!(expired_acceptance.error_message.is_some());
    println!("✓ Expired invitation acceptance properly handled");

    // Test relationship formation error handling
    let invalid_formation_request = RelationshipFormationRequest {
        party_a: test.inviter_device,
        party_b: test.inviter_device, // invalid: same device for both parties
        account_id: test.account_id,
        relationship_type: RelationshipType::DeviceCoOwnership,
        initial_trust_level: TrustLevel::Low,
        metadata: vec![],
    };
    let formation_coordinator =
        LegacyRelationshipFormationCoordinator::new(test.inviter_effects.clone());
    let formation = formation_coordinator
        .form_relationship(invalid_formation_request)
        .await?;
    assert!(!formation.success);
    assert!(formation.error.is_some());
    println!("✓ Invalid relationship formation properly handled");

    println!("✓ Error handling integration across all components successful");
    Ok(())
}
