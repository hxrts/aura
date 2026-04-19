use aura_core::semilattice::JoinSemilattice;
use aura_core::AccountId;
use aura_journal::algebra::AccountState;

#[test]
fn account_state_creation() {
    let account_id = AccountId(uuid::Uuid::from_bytes([1u8; 16]));
    let (_sk, group_public_key) = aura_core::util::test_utils::test_key_pair(1);

    let state = AccountState::new(account_id, group_public_key);
    assert_eq!(state.get_epoch(), 0);
}

#[test]
fn epoch_management() {
    let account_id = AccountId(uuid::Uuid::from_bytes([3u8; 16]));
    let (_sk, group_public_key) = aura_core::util::test_utils::test_key_pair(2);

    let mut state = AccountState::new(account_id, group_public_key);
    assert_eq!(state.get_epoch(), 0);

    state.increment_epoch();
    assert_eq!(state.get_epoch(), 1);

    state.set_epoch_if_higher(5);
    assert_eq!(state.get_epoch(), 5);

    state.set_epoch_if_higher(3);
    assert_eq!(state.get_epoch(), 5);
}

#[test]
fn join_semilattice() {
    let account_id = AccountId(uuid::Uuid::from_bytes([4u8; 16]));
    let (_sk, group_public_key) = aura_core::util::test_utils::test_key_pair(3);

    let mut state1 = AccountState::new(account_id, group_public_key);
    let mut state2 = AccountState::new(account_id, group_public_key);

    state1.set_epoch_if_higher(3);
    state2.set_epoch_if_higher(5);

    let merged = state1.join(&state2);
    assert_eq!(merged.get_epoch(), 5);
}
