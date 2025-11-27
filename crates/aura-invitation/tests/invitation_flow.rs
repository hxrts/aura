//! Test module for device invitation flow
//!
//! This module tests the end-to-end invitation lifecycle including
//! invitation creation, acceptance, and registry state validation.

#![allow(clippy::disallowed_methods)]
#![allow(clippy::expect_used)]

// Effect system imports now handled through testkit
use aura_core::{AccountId, DeviceId};
use aura_invitation::{
    device_invitation::{DeviceInvitationCoordinator, DeviceInvitationRequest},
    invitation_acceptance::InvitationAcceptanceCoordinator,
};
use aura_journal::semilattice::InvitationRecordRegistry;
use aura_macros::aura_test;
use aura_wot::{AccountAuthority, SerializableBiscuit};
use futures::lock::Mutex;
use std::sync::Arc;
use uuid::Uuid;
// Note: For testing, use mock handlers from aura-effects

fn sample_request(invitee: DeviceId) -> DeviceInvitationRequest {
    let account_id = AccountId(Uuid::new_v4());
    let authority = AccountAuthority::new(account_id);
    let device_token = authority
        .create_device_token(invitee)
        .unwrap_or_else(|_| panic!("Failed to create device token"));
    let root_key = authority.root_public_key();
    let serializable_token = SerializableBiscuit::new(device_token, root_key);

    DeviceInvitationRequest {
        inviter: DeviceId(Uuid::new_v4()),
        invitee,
        account_id,
        granted_token: serializable_token,
        device_role: "cli-device".into(),
        ttl_secs: Some(60),
    }
}

#[aura_test]
async fn invitation_lifecycle() -> aura_core::AuraResult<()> {
    let inviter_fixture = aura_testkit::create_test_fixture().await?;
    let invitee_fixture = aura_testkit::create_test_fixture().await?;
    let inviter_effects = inviter_fixture.effect_system_wrapped();
    let invitee_effects = invitee_fixture.effect_system_wrapped();
    let registry = Arc::new(Mutex::new(InvitationRecordRegistry::new()));

    let coordinator = DeviceInvitationCoordinator::with_registry(inviter_effects, registry.clone());
    let request = sample_request(DeviceId(Uuid::new_v4()));
    let response = coordinator.invite_device(request.clone()).await?;

    let acceptance_coordinator =
        InvitationAcceptanceCoordinator::with_registry(invitee_effects, registry.clone());
    let acceptance = acceptance_coordinator
        .accept_invitation(response.invitation.clone())
        .await?;

    assert_eq!(acceptance.invitation_id, response.invitation.invitation_id);

    let registry = registry.lock().await;
    let record = registry
        .get(&acceptance.invitation_id)
        .ok_or_else(|| aura_core::AuraError::invalid("record should exist"))?;
    println!("Record status: {:?}, Expected: Accepted", record.status);
    assert!(matches!(
        record.status,
        aura_journal::semilattice::InvitationStatus::Accepted
    ));
    Ok(())
}
