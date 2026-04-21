use aura_core::Authority;
use aura_core::AuthorityId;
use aura_journal::authority_state::{reduce_authority_state, AuthorityState, DerivedAuthority};
use aura_journal::{FactJournal, JournalNamespace};

#[test]
fn derived_authority_creation_from_journal() {
    let authority_id = AuthorityId::new_from_entropy([7u8; 32]);
    let journal = FactJournal::new(JournalNamespace::Authority(authority_id));

    let result = DerivedAuthority::from_journal(authority_id, &journal);
    assert!(result.is_ok(), "{:?}", result.err());

    if let Ok(derived_authority) = result {
        assert_eq!(derived_authority.authority_id(), authority_id);
    }
}

#[test]
fn reduce_authority_state_basic() {
    let authority_id = AuthorityId::new_from_entropy([8u8; 32]);
    let journal = FactJournal::new(JournalNamespace::Authority(authority_id));

    let result = reduce_authority_state(authority_id, &journal);
    assert!(result.is_ok(), "{:?}", result.err());

    if let Ok(authority_state) = result {
        assert_eq!(authority_state.authority_id, Some(authority_id));
        assert_eq!(authority_state.tree_state.threshold(), 1);
    }
}

#[test]
fn authority_state_creation() {
    let tree_state = aura_core::types::authority::TreeStateSummary::new();
    let authority_id = AuthorityId::new_from_entropy([9u8; 32]);

    let authority_state = AuthorityState::new(tree_state.clone());
    assert!(authority_state.authority_id.is_none());
    assert_eq!(authority_state.tree_state.threshold(), 1);

    let authority_state_with_id = AuthorityState::with_authority(tree_state, authority_id);
    assert_eq!(authority_state_with_id.authority_id, Some(authority_id));
    assert_eq!(authority_state_with_id.tree_state.threshold(), 1);
}
