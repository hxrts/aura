use crate::common::{make_authority_journal, make_snapshot_fact};
use aura_journal::pure::merge::journal_join;

#[test]
fn journal_join_wrapper() {
    let mut j1 = make_authority_journal(1);
    let mut j2 = make_authority_journal(1);

    j1.add_fact(make_snapshot_fact(1, 1)).unwrap();
    j1.add_fact(make_snapshot_fact(2, 2)).unwrap();
    j2.add_fact(make_snapshot_fact(3, 3)).unwrap();

    let merged = journal_join(&j1, &j2);
    assert_eq!(merged.size(), 3);
}

#[test]
fn journal_join_commutative() {
    let mut j1 = make_authority_journal(1);
    let mut j2 = make_authority_journal(1);

    j1.add_fact(make_snapshot_fact(1, 1)).unwrap();
    j2.add_fact(make_snapshot_fact(2, 2)).unwrap();

    let m12 = journal_join(&j1, &j2);
    let m21 = journal_join(&j2, &j1);

    assert_eq!(m12.size(), m21.size());
    assert_eq!(m12.facts, m21.facts);
}

#[test]
fn journal_join_idempotent() {
    let mut journal = make_authority_journal(1);
    journal.add_fact(make_snapshot_fact(1, 1)).unwrap();
    journal.add_fact(make_snapshot_fact(2, 2)).unwrap();

    let merged = journal_join(&journal, &journal);
    assert_eq!(journal.facts, merged.facts);
}
