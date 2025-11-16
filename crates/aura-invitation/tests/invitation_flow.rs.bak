//! Test module for device invitation flow
//!
//! This module tests the end-to-end invitation lifecycle including
//! invitation creation, acceptance, and ledger state validation.

#![allow(clippy::disallowed_methods)]
#![allow(clippy::expect_used)]

use aura_core::{AccountId, Cap, DeviceId};
use aura_invitation::{
    device_invitation::{DeviceInvitationCoordinator, DeviceInvitationRequest},
    invitation_acceptance::InvitationAcceptanceCoordinator,
};
use aura_journal::semilattice::InvitationLedger;
use aura_macros::aura_test;
use aura_protocol::effects::AuraEffectSystem;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
// Note: For testing, use mock handlers from aura-effects

fn sample_request(invitee: DeviceId) -> DeviceInvitationRequest {
    DeviceInvitationRequest {
        inviter: DeviceId(Uuid::new_v4()),
        invitee,
        account_id: AccountId(Uuid::new_v4()),
        granted_capabilities: Cap::top(),
        device_role: "cli-device".into(),
        ttl_secs: Some(60),
    }
}

#[aura_test]
async fn invitation_lifecycle() -> aura_core::AuraResult<()> {
    let inviter_fixture = aura_testkit::create_test_fixture().await?;
    let invitee_fixture = aura_testkit::create_test_fixture().await?;
    let inviter_effects = inviter_fixture.effect_system();
    let invitee_effects = invitee_fixture.effect_system();
    let shared_ledger = Arc::new(Mutex::new(InvitationLedger::new()));

    let coordinator =
        DeviceInvitationCoordinator::with_ledger(inviter_effects, shared_ledger.clone());
    let request = sample_request(DeviceId(Uuid::new_v4()));
    let response = coordinator.invite_device(request.clone()).await?;

    let acceptance_coordinator =
        InvitationAcceptanceCoordinator::with_ledger(invitee_effects, shared_ledger.clone());
    let acceptance = acceptance_coordinator
        .accept_invitation(response.invitation.clone())
        .await?;

    assert_eq!(acceptance.invitation_id, response.invitation.invitation_id);

    let ledger = shared_ledger.lock().await;
    let record = ledger
        .get(&acceptance.invitation_id)
        .ok_or_else(|| aura_core::AuraError::invalid("record should exist"))?;
    println!("Record status: {:?}, Expected: Accepted", record.status);
    assert!(matches!(
        record.status,
        aura_journal::semilattice::InvitationStatus::Accepted
    ));
    Ok(())
}
