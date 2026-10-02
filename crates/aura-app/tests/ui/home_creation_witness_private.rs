use aura_app::views::home::HomeCreationWitness;
use aura_core::types::identifiers::{AuthorityId, ContextId};

fn main() {
    let created = aura_social::SocialFact::home_created_ms(
        aura_social::HomeId::from_bytes([1; 32]),
        ContextId::new_from_entropy([2; 32]),
        1,
        AuthorityId::new_from_entropy([3; 32]),
        "Den".into(),
    );
    let _ = HomeCreationWitness::from_created_fact(&created);
}
