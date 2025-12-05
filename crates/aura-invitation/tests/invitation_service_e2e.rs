use aura_core::identifiers::{AuthorityId, ContextId};
use aura_invitation::{
    guards::GuardSnapshot, Invitation, InvitationConfig, InvitationService, InvitationStatus,
    InvitationType,
};
use uuid::Uuid;

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

fn make_invitation(
    context_id: ContextId,
    sender: AuthorityId,
    receiver: AuthorityId,
    invitation_id: &str,
    invitation_type: InvitationType,
    now_ms: u64,
    expires_ms: Option<u64>,
) -> Invitation {
    Invitation {
        invitation_id: invitation_id.to_string(),
        context_id,
        sender_id: sender,
        receiver_id: receiver,
        invitation_type,
        status: InvitationStatus::Pending,
        created_at: now_ms,
        expires_at: expires_ms,
        message: Some("hi".into()),
    }
}

#[test]
fn invitation_send_and_accept_end_to_end() {
    let sender = AuthorityId::from_uuid(Uuid::new_v4());
    let receiver = AuthorityId::from_uuid(Uuid::new_v4());
    let ctx = ContextId::from_uuid(Uuid::new_v4());

    let mut sender_service = InvitationService::new(sender, InvitationConfig::default());
    let mut receiver_service = InvitationService::new(receiver, InvitationConfig::default());

    // Sender prepares to send
    let send_snapshot = snapshot_with_caps(sender, ctx, &["invitation:send"]);
    let invitation_id = "inv-123";
    let outcome = sender_service.prepare_send_invitation(
        &send_snapshot,
        receiver,
        InvitationType::Contact {
            petname: Some("pal".into()),
        },
        Some("welcome".into()),
        Some(60_000),
        invitation_id.to_string(),
    );
    assert!(outcome.is_allowed(), "send should be allowed");

    // Simulate effect execution by caching the invitation for both parties
    let inv = make_invitation(
        ctx,
        sender,
        receiver,
        invitation_id,
        InvitationType::Contact {
            petname: Some("pal".into()),
        },
        send_snapshot.now_ms,
        Some(send_snapshot.now_ms + 60_000),
    );
    sender_service.cache_invitation(inv.clone());
    receiver_service.cache_invitation(inv);

    // Receiver prepares to accept
    let accept_snapshot = snapshot_with_caps(receiver, ctx, &["invitation:accept"]);
    let accept_outcome =
        receiver_service.prepare_accept_invitation(&accept_snapshot, invitation_id);
    assert!(
        accept_outcome.is_allowed(),
        "accept should be allowed for pending invitation"
    );

    // Apply acceptance: update cached status
    sender_service.update_invitation_status(invitation_id, InvitationStatus::Accepted);
    receiver_service.update_invitation_status(invitation_id, InvitationStatus::Accepted);

    // Validate cache views
    assert_eq!(receiver_service.list_pending_invitations().len(), 0);
    assert_eq!(sender_service.list_sent_invitations().len(), 1);
    assert_eq!(receiver_service.list_received_invitations().len(), 1);
    assert_eq!(
        receiver_service
            .get_cached_invitation(invitation_id)
            .map(|i| i.status.clone()),
        Some(InvitationStatus::Accepted)
    );
}

#[test]
fn invitation_decline_flow_marks_status() {
    let sender = AuthorityId::from_uuid(Uuid::new_v4());
    let receiver = AuthorityId::from_uuid(Uuid::new_v4());
    let ctx = ContextId::from_uuid(Uuid::new_v4());

    let mut svc = InvitationService::new(receiver, InvitationConfig::default());
    let invitation_id = "inv-decline";
    let inv = make_invitation(
        ctx,
        sender,
        receiver,
        invitation_id,
        InvitationType::Guardian {
            subject_authority: sender,
        },
        1_000,
        None,
    );
    svc.cache_invitation(inv);

    let snapshot = snapshot_with_caps(receiver, ctx, &["invitation:decline"]);
    let outcome = svc.prepare_decline_invitation(&snapshot, invitation_id);
    assert!(
        outcome.is_allowed(),
        "decline should be allowed when pending"
    );

    svc.update_invitation_status(invitation_id, InvitationStatus::Declined);
    assert_eq!(
        svc.get_cached_invitation(invitation_id)
            .map(|i| i.status.clone()),
        Some(InvitationStatus::Declined)
    );
}
