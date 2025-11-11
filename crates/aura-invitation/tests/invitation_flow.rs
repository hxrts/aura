use aura_core::{AccountId, Cap, DeviceId, Top};
use aura_invitation::{
    device_invitation::{DeviceInvitationCoordinator, DeviceInvitationRequest},
    invitation_acceptance::InvitationAcceptanceCoordinator,
};
// Note: For testing, use mock handlers from aura-effects

fn sample_request(invitee: DeviceId) -> DeviceInvitationRequest {
    DeviceInvitationRequest {
        inviter: DeviceId::new(),
        invitee,
        account_id: AccountId::new(),
        granted_capabilities: Cap::top(),
        device_role: "cli-device".into(),
        ttl_secs: Some(60),
    }
}

#[tokio::test]
async fn invitation_lifecycle() {
    let effects = AuraEffectSystem::for_testing(DeviceId::new());
    let mut coordinator = DeviceInvitationCoordinator::new(effects.clone());
    let request = sample_request(DeviceId::new());
    let response = coordinator
        .invite_device(request.clone())
        .await
        .expect("invitation should succeed");

    let acceptance_coordinator = InvitationAcceptanceCoordinator::new(effects.clone());
    let acceptance = acceptance_coordinator
        .accept_invitation(response.invitation.clone())
        .await
        .expect("invitee should accept invitation");

    assert_eq!(acceptance.invitation_id, response.invitation.invitation_id);

    let ledger = coordinator.ledger_snapshot().await;
    let record = ledger
        .get(&acceptance.invitation_id)
        .expect("record should exist");
    assert!(matches!(
        record.status,
        aura_journal::semilattice::InvitationStatus::Accepted
    ));
}
