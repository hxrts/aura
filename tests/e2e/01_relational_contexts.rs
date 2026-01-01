use aura_core::{hash, AuthorityId, Hash32};
use aura_core::relational::{GuardianBinding, GuardianParameters, RelationalFact};
use aura_relational::RelationalContext;

#[test]
fn relational_context_records_guardian_bindings() {
    let account = AuthorityId::new_from_entropy([1u8; 32]);
    let guardian = AuthorityId::new_from_entropy([1u8; 32]);

    let mut context = RelationalContext::with_id(Default::default(), vec![account, guardian]);

    let binding = GuardianBinding::new(
        commitment_for(&account),
        commitment_for(&guardian),
        GuardianParameters::default(),
    );

    context
        .add_fact(RelationalFact::GuardianBinding(binding))
        .unwrap();

    assert_eq!(context.guardian_bindings().len(), 1);
}

fn commitment_for(id: &AuthorityId) -> Hash32 {
    let digest = hash::hash(&id.to_bytes());
    Hash32::new(digest)
}
