use aura_app::views::invitations::{
    Invitation, InvitationDirection, InvitationStatus, InvitationType, InvitationsState,
};
use aura_core::types::identifiers::AuthorityId;

fn main() {
    let authority = AuthorityId::new_from_entropy([8; 32]);
    let mut invitations = InvitationsState::default();
    let raw_invitation = Invitation {
        id: "raw".to_string(),
        invitation_type: InvitationType::Contact,
        status: InvitationStatus::Pending,
        direction: InvitationDirection::Received,
        from_id: authority,
        from_name: String::new(),
        to_id: None,
        to_name: None,
        created_at: 0,
        expires_at: None,
        message: None,
        home_id: None,
        home_name: None,
    };
    invitations.add_invitation(raw_invitation);
}
