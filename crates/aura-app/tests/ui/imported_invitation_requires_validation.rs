use aura_app::views::invitations::InvitationCreationWitness;
use aura_core::types::identifiers::{AuthorityId, ContextId, InvitationId};
use aura_invitation::{Invitation, InvitationStatus, InvitationType};

fn main() {
    let authority = AuthorityId::new_from_entropy([1; 32]);
    let raw = Invitation {
        invitation_id: InvitationId::new("raw"),
        context_id: ContextId::new_from_entropy([2; 32]),
        sender_id: authority,
        receiver_id: authority,
        invitation_type: InvitationType::Contact { nickname: None },
        status: InvitationStatus::Pending,
        created_at: 0,
        expires_at: None,
        message: None,
        receiver_nickname: None,
    };
    let _ = InvitationCreationWitness::from_imported(&raw, authority);
}
