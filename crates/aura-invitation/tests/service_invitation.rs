use aura_core::identifiers::{AuthorityId, ContextId};
use aura_invitation::{GuardSnapshot, InvitationConfig, InvitationService, InvitationType};
use uuid::Uuid;

fn snapshot_with_caps(caps: &[&str]) -> GuardSnapshot {
    GuardSnapshot::new(
        AuthorityId::from_uuid(Uuid::new_v4()),
        ContextId::from_uuid(Uuid::new_v4()),
        10,
        caps.iter().map(|c| c.to_string()).collect(),
        0,
        1,
    )
}

#[test]
fn prepare_send_invitation_allows_with_capabilities() {
    let svc = InvitationService::new(
        AuthorityId::from_uuid(Uuid::new_v4()),
        InvitationConfig::default(),
    );

    let snap = snapshot_with_caps(&["invitation:send"]);
    let outcome = svc.prepare_send_invitation(
        &snap,
        AuthorityId::from_uuid(Uuid::new_v4()),
        InvitationType::Contact { petname: None },
        Some("hi".to_string()),
        Some(1000),
        "inv-1".to_string(),
    );

    assert!(
        outcome.is_allowed(),
        "send should be allowed when capability present"
    );
}

#[test]
fn prepare_accept_invitation_requires_capability() {
    let svc = InvitationService::new(
        AuthorityId::from_uuid(Uuid::new_v4()),
        InvitationConfig::default(),
    );

    let snap = snapshot_with_caps(&[]); // no caps
    let outcome = svc.prepare_accept_invitation(&snap, "inv-absent");
    assert!(
        outcome.is_denied(),
        "accept should be denied without capability"
    );

    let snap_ok = snapshot_with_caps(&["invitation:accept"]);
    let outcome_ok = svc.prepare_accept_invitation(&snap_ok, "inv-absent");
    assert!(
        outcome_ok.is_allowed(),
        "accept should be allowed with capability"
    );
}
