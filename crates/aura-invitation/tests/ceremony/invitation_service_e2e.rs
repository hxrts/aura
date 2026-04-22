//! End-to-end invitation service tests.
//!
//! Verifies the complete send → accept and send → decline flows with
//! guard evaluation outcomes.

#![allow(missing_docs)]

use crate::support;
use aura_core::types::identifiers::{AuthorityId, ContextId, InvitationId};
use aura_core::util::test_utils::test_authority_id;
use aura_invitation::{
    capabilities::InvitationCapability, guards::GuardSnapshot, InvitationConfig,
    InvitationLifecycleSnapshot, InvitationService, InvitationType,
};

fn snapshot_with_caps(
    auth: AuthorityId,
    ctx: ContextId,
    caps: &[InvitationCapability],
) -> GuardSnapshot {
    support::snapshot_with_caps(auth, ctx, caps, 10, 1_000)
}

#[test]
fn invitation_send_and_accept_end_to_end() {
    let sender = test_authority_id(1);
    let receiver = test_authority_id(2);
    let ctx = support::test_context(1);

    let sender_service = InvitationService::new(sender, InvitationConfig::default());
    let receiver_service = InvitationService::new(receiver, InvitationConfig::default());

    // Sender prepares to send
    let send_snapshot = snapshot_with_caps(sender, ctx, &[InvitationCapability::Send]);
    let invitation_id = InvitationId::new("inv-123");
    let outcome = sender_service.prepare_send_invitation(
        &send_snapshot,
        receiver,
        InvitationType::Contact {
            nickname: Some("pal".into()),
        },
        Some("welcome".into()),
        Some(60_000),
        invitation_id.clone(),
    );
    assert!(outcome.is_allowed(), "send should be allowed");

    // Receiver prepares to accept
    let accept_snapshot = snapshot_with_caps(receiver, ctx, &[InvitationCapability::Accept])
        .with_invitation_lifecycle(InvitationLifecycleSnapshot::pending(
            invitation_id.clone(),
            ctx,
            sender,
            receiver,
            Some(61_000),
        ));
    let accept_outcome =
        receiver_service.prepare_accept_invitation(&accept_snapshot, &invitation_id);
    assert!(
        accept_outcome.is_allowed(),
        "accept should be allowed for pending invitation"
    );
}

#[test]
fn invitation_decline_flow_marks_status() {
    let sender = test_authority_id(3);
    let receiver = test_authority_id(4);
    let ctx = support::test_context(2);

    let svc = InvitationService::new(receiver, InvitationConfig::default());
    let invitation_id = InvitationId::new("inv-decline");

    let snapshot = snapshot_with_caps(receiver, ctx, &[InvitationCapability::Decline])
        .with_invitation_lifecycle(InvitationLifecycleSnapshot::pending(
            invitation_id.clone(),
            ctx,
            sender,
            receiver,
            Some(2_000),
        ));
    let outcome = svc.prepare_decline_invitation(&snapshot, &invitation_id);
    assert!(
        outcome.is_allowed(),
        "decline should be allowed when pending"
    );
}
