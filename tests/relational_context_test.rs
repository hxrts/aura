use aura_core::{hash, AuthorityId, Hash32};
use aura_relational::{
    guardian::{GuardianBinding, GuardianParameters},
    RelationalContext, RelationalFact,
};

#[test]
fn relational_context_records_guardian_bindings() {
    let account = AuthorityId::new();
    let guardian = AuthorityId::new();

    let mut context = RelationalContext::with_id(Default::default(), vec![account, guardian]);

    let binding = GuardianBinding {
        account_commitment: commitment_for(&account),
        guardian_commitment: commitment_for(&guardian),
        parameters: GuardianParameters::default(),
        consensus_proof: None,
    };

    context
        .add_fact(RelationalFact::GuardianBinding(binding))
        .unwrap();

    assert_eq!(context.guardian_bindings().len(), 1);
}

fn commitment_for(id: &AuthorityId) -> Hash32 {
    let digest = hash::hash(&id.to_bytes());
    Hash32::new(digest)
}
