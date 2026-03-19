//! Contact establishment matrix tests.
//!
//! Verifies that all authority class pair combinations can establish
//! contact invitations — cross-flow equivalence test.

#![allow(missing_docs)]

use aura_core::types::identifiers::{AuthorityId, ContextId, InvitationId};
use aura_core::FlowCost;
use aura_guards::types::CapabilityId;
use aura_invitation::{guards::GuardSnapshot, InvitationConfig, InvitationService, InvitationType};

#[derive(Clone, Copy, Debug)]
enum AuthorityClass {
    User,
    Home,
    Neighborhood,
}

impl AuthorityClass {
    fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Home => "home",
            Self::Neighborhood => "neighborhood",
        }
    }

    fn seed(self) -> u8 {
        match self {
            Self::User => 11,
            Self::Home => 22,
            Self::Neighborhood => 33,
        }
    }
}

fn authority_id(class: AuthorityClass, offset: u8) -> AuthorityId {
    let mut entropy = [0u8; 32];
    entropy[0] = class.seed();
    entropy[1] = offset;
    AuthorityId::new_from_entropy(entropy)
}

fn snapshot_with_send_cap(auth: AuthorityId, ctx: ContextId) -> GuardSnapshot {
    GuardSnapshot::new(
        auth,
        ctx,
        FlowCost::new(50),
        vec![CapabilityId::from("invitation:send")],
        0,
        1_700_000_000_000,
    )
}

#[test]
fn contact_establishment_matrix_allows_supported_authority_pairs() {
    let pairs = [
        (AuthorityClass::User, AuthorityClass::User),
        (AuthorityClass::User, AuthorityClass::Home),
        (AuthorityClass::Home, AuthorityClass::User),
        (AuthorityClass::Home, AuthorityClass::Home),
        (AuthorityClass::User, AuthorityClass::Neighborhood),
        (AuthorityClass::Neighborhood, AuthorityClass::User),
        (AuthorityClass::Home, AuthorityClass::Neighborhood),
        (AuthorityClass::Neighborhood, AuthorityClass::Home),
        (AuthorityClass::Neighborhood, AuthorityClass::Neighborhood),
    ];

    let context = ContextId::new_from_entropy([9u8; 32]);

    for (idx, (sender_class, receiver_class)) in pairs.iter().copied().enumerate() {
        let sender = authority_id(sender_class, idx as u8 + 1);
        let receiver = authority_id(receiver_class, idx as u8 + 101);
        let service = InvitationService::new(sender, InvitationConfig::default());
        let snapshot = snapshot_with_send_cap(sender, context);

        let invitation_id = InvitationId::new(format!(
            "contact-{}-{}-{idx}",
            sender_class.label(),
            receiver_class.label()
        ));

        let outcome = service.prepare_send_invitation(
            &snapshot,
            receiver,
            InvitationType::Contact { nickname: None },
            Some("matrix".to_string()),
            Some(60_000),
            invitation_id,
        );

        assert!(
            outcome.is_allowed(),
            "contact establishment should be allowed for {} -> {}",
            sender_class.label(),
            receiver_class.label()
        );
    }
}
