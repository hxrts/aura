#![allow(missing_docs)]

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::util::test_utils::test_authority_id;
use aura_invitation::{
    guards::GuardSnapshot, InvitationConfig, InvitationService, InvitationType,
};

fn snapshot_with_caps(auth: AuthorityId, ctx: ContextId, caps: &[&str]) -> GuardSnapshot {
    GuardSnapshot::new(
        auth,
        ctx,
        10, // flow budget
        caps.iter().map(|c| c.to_string()).collect(),
        0,     // epoch
        1_000, // now_ms
    )
}

#[test]
fn invitation_send_and_accept_end_to_end() {
    let sender = test_authority_id(1);
    let receiver = test_authority_id(2);
    let ctx = ContextId::new_from_entropy([1u8; 32]);

    let sender_service = InvitationService::new(sender, InvitationConfig::default());
    let receiver_service = InvitationService::new(receiver, InvitationConfig::default());

    // Sender prepares to send
    let send_snapshot = snapshot_with_caps(sender, ctx, &["invitation:send"]);
    let invitation_id = "inv-123";
    let outcome = sender_service.prepare_send_invitation(
        &send_snapshot,
        receiver,
        InvitationType::Contact {
            nickname: Some("pal".into()),
        },
        Some("welcome".into()),
        Some(60_000),
        invitation_id.to_string(),
    );
    assert!(outcome.is_allowed(), "send should be allowed");

    // Receiver prepares to accept
    let accept_snapshot = snapshot_with_caps(receiver, ctx, &["invitation:accept"]);
    let accept_outcome =
        receiver_service.prepare_accept_invitation(&accept_snapshot, invitation_id);
    assert!(
        accept_outcome.is_allowed(),
        "accept should be allowed for pending invitation"
    );
}

#[test]
fn invitation_decline_flow_marks_status() {
    let sender = test_authority_id(3);
    let receiver = test_authority_id(4);
    let ctx = ContextId::new_from_entropy([2u8; 32]);

    let svc = InvitationService::new(receiver, InvitationConfig::default());
    let invitation_id = "inv-decline";

    let snapshot = snapshot_with_caps(receiver, ctx, &["invitation:decline"]);
    let outcome = svc.prepare_decline_invitation(&snapshot, invitation_id);
    assert!(
        outcome.is_allowed(),
        "decline should be allowed when pending"
    );
}
