use aura_app::views::invitations::InvitationCreationWitness;
use aura_core::types::identifiers::AuthorityId;

fn fabricate<T>() -> T {
    panic!("compile-fail fixture")
}

fn main() {
    let authority = AuthorityId::new_from_entropy([1; 32]);
    let _ = InvitationCreationWitness::from_sent_fact(&fabricate(), authority);
}
