#![allow(missing_docs)]

use aura_core::identifiers::{ContextId, InvitationId};
use aura_core::util::test_utils::test_authority_id;
use aura_core::FlowCost;
use aura_guards::types::CapabilityId;
use aura_invitation::{GuardSnapshot, InvitationConfig, InvitationService, InvitationType};

fn snapshot_with_caps(caps: &[&str]) -> GuardSnapshot {
    GuardSnapshot::new(
        test_authority_id(10),
        ContextId::new_from_entropy([20u8; 32]),
        FlowCost::new(10),
        caps.iter().map(|c| CapabilityId::from(*c)).collect(),
        0,
        1,
    )
}

#[test]
fn prepare_send_invitation_allows_with_capabilities() {
    let svc = InvitationService::new(test_authority_id(1), InvitationConfig::default());

    let snap = snapshot_with_caps(&["invitation:send"]);
    let outcome = svc.prepare_send_invitation(
        &snap,
        test_authority_id(2),
        InvitationType::Contact { nickname: None },
        Some("hi".to_string()),
        Some(1000),
        InvitationId::new("inv-1"),
    );

    assert!(
        outcome.is_allowed(),
        "send should be allowed when capability present"
    );
}

#[test]
fn prepare_accept_invitation_requires_capability() {
    let svc = InvitationService::new(test_authority_id(3), InvitationConfig::default());

    let snap = snapshot_with_caps(&[]); // no caps
    let invitation_id = InvitationId::new("inv-absent");
    let outcome = svc.prepare_accept_invitation(&snap, &invitation_id);
    assert!(
        outcome.is_denied(),
        "accept should be denied without capability"
    );

    let snap_ok = snapshot_with_caps(&["invitation:accept"]);
    let outcome_ok = svc.prepare_accept_invitation(&snap_ok, &invitation_id);
    assert!(
        outcome_ok.is_allowed(),
        "accept should be allowed with capability"
    );
}
