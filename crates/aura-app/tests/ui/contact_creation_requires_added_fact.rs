use aura_app::views::contacts::{Contact, ContactRelationshipState, ContactsState, ReadReceiptPolicy};
use aura_core::types::identifiers::AuthorityId;

fn main() {
    let peer = AuthorityId::new_from_entropy([7; 32]);
    let mut contacts = ContactsState::new();
    let raw_contact = Contact {
        id: peer,
        nickname: String::new(),
        nickname_suggestion: None,
        is_guardian: false,
        is_member: false,
        last_interaction: None,
        is_online: false,
        read_receipt_policy: ReadReceiptPolicy::default(),
        relationship_state: ContactRelationshipState::Contact,
        invitation_code: None,
    };
    contacts.apply_contact(raw_contact);
}
