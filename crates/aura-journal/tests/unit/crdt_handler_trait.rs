use aura_journal::crdt::CrdtSemantics;

#[test]
fn crdt_semantics() {
    assert!(CrdtSemantics::JoinSemilattice.uses_join());
    assert!(!CrdtSemantics::JoinSemilattice.uses_meet());

    assert!(CrdtSemantics::MeetSemilattice.uses_meet());
    assert!(!CrdtSemantics::MeetSemilattice.uses_join());

    assert!(CrdtSemantics::OperationBased.requires_causal_ordering());
    assert!(!CrdtSemantics::JoinSemilattice.requires_causal_ordering());
    assert!(CrdtSemantics::DeltaBased.uses_join());
}

#[test]
fn usage_guidance() {
    assert!(CrdtSemantics::JoinSemilattice
        .usage_guidance()
        .contains("accumulating"));
    assert!(CrdtSemantics::MeetSemilattice
        .usage_guidance()
        .contains("restricting"));
}
