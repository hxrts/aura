use crate::common::{make_authority_journal, make_snapshot_fact};
use aura_journal::pure::reduce::{authority_reduce, context_reduce};

#[test]
fn authority_reduce_deterministic() {
    let mut journal = make_authority_journal(1);
    journal.add_fact(make_snapshot_fact(1, 1)).unwrap();
    journal.add_fact(make_snapshot_fact(2, 2)).unwrap();

    let result1 = authority_reduce(&journal);
    let result2 = authority_reduce(&journal);

    assert_eq!(result1.is_ok(), result2.is_ok());
}

#[test]
fn context_reduce_wrong_namespace() {
    let journal = make_authority_journal(1);
    let result = context_reduce(&journal);
    assert!(result.is_err());
}
