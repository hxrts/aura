use aura_core::domain::{Agreement, Propagation};
use aura_core::query::ConsensusId;
use aura_core::time::{OrderTime, PhysicalTime, TimeStamp};
use aura_core::types::identifiers::AuthorityId;
use aura_core::Hash32;
use aura_journal::{AckStorage, Fact, FactContent, FactJournal, JournalNamespace, SnapshotFact};

#[test]
fn journal_creation() {
    let auth_id = AuthorityId::new_from_entropy([9u8; 32]);
    let namespace = JournalNamespace::Authority(auth_id);
    let journal = FactJournal::new(namespace.clone());

    assert_eq!(journal.namespace, namespace);
    assert_eq!(journal.size(), 0);
}

#[test]
fn journal_merge() {
    use aura_core::semilattice::JoinSemilattice;

    let auth_id = AuthorityId::new_from_entropy([10u8; 32]);
    let namespace = JournalNamespace::Authority(auth_id);

    let mut journal1 = FactJournal::new(namespace.clone());
    let mut journal2 = FactJournal::new(namespace);

    let fact1 = Fact::new(
        OrderTime([1u8; 32]),
        TimeStamp::OrderClock(OrderTime([1u8; 32])),
        FactContent::Snapshot(SnapshotFact {
            state_hash: Hash32::default(),
            superseded_facts: vec![],
            sequence: 1,
        }),
    );
    let fact2 = Fact::new(
        OrderTime([2u8; 32]),
        TimeStamp::OrderClock(OrderTime([2u8; 32])),
        FactContent::Snapshot(SnapshotFact {
            state_hash: Hash32::default(),
            superseded_facts: vec![],
            sequence: 2,
        }),
    );

    journal1.add_fact(fact1.clone()).unwrap();
    journal2.add_fact(fact2.clone()).unwrap();

    let merged = journal1.join(&journal2);
    assert_eq!(merged.size(), 2);
    assert!(merged.contains_timestamp(&fact1.timestamp));
    assert!(merged.contains_timestamp(&fact2.timestamp));
}

#[test]
fn journal_merge_different_namespaces() {
    let journal1 = FactJournal::new(JournalNamespace::Authority(AuthorityId::new_from_entropy(
        [11u8; 32],
    )));
    let journal2 = FactJournal::new(JournalNamespace::Authority(AuthorityId::new_from_entropy(
        [12u8; 32],
    )));

    let err = journal1
        .try_join(&journal2)
        .expect_err("cross-namespace remote merge must return a typed error");

    assert_eq!(err.left, journal1.namespace);
    assert_eq!(err.right, journal2.namespace);
}

#[test]
fn fact_default_metadata() {
    let fact = Fact::new(
        OrderTime([1u8; 32]),
        TimeStamp::OrderClock(OrderTime([1u8; 32])),
        FactContent::Snapshot(SnapshotFact {
            state_hash: Hash32::default(),
            superseded_facts: vec![],
            sequence: 1,
        }),
    );

    assert!(fact.agreement.is_provisional());
    assert!(fact.propagation.is_local());
    assert!(!fact.ack_tracked);
    assert!(!fact.is_finalized());
    assert!(!fact.is_propagated());
}

#[test]
fn fact_with_ack_tracking() {
    let fact = Fact::new_with_ack_tracking(
        OrderTime([1u8; 32]),
        TimeStamp::OrderClock(OrderTime([1u8; 32])),
        FactContent::Snapshot(SnapshotFact {
            state_hash: Hash32::default(),
            superseded_facts: vec![],
            sequence: 1,
        }),
    );
    assert!(fact.ack_tracked);
}

#[test]
fn fact_builder_pattern() {
    let consensus_id = ConsensusId::new([1u8; 32]);
    let fact = Fact::new(
        OrderTime([1u8; 32]),
        TimeStamp::OrderClock(OrderTime([1u8; 32])),
        FactContent::Snapshot(SnapshotFact {
            state_hash: Hash32::default(),
            superseded_facts: vec![],
            sequence: 1,
        }),
    )
    .with_agreement(Agreement::finalized(consensus_id))
    .with_propagation(Propagation::complete())
    .with_ack_tracking();

    assert!(fact.is_finalized());
    assert!(fact.is_propagated());
    assert!(fact.ack_tracked);
}

#[test]
fn fact_equality_ignores_metadata() {
    let fact1 = Fact::new(
        OrderTime([1u8; 32]),
        TimeStamp::OrderClock(OrderTime([1u8; 32])),
        FactContent::Snapshot(SnapshotFact {
            state_hash: Hash32::default(),
            superseded_facts: vec![],
            sequence: 1,
        }),
    );

    let fact2 = Fact::new(
        OrderTime([1u8; 32]),
        TimeStamp::OrderClock(OrderTime([1u8; 32])),
        FactContent::Snapshot(SnapshotFact {
            state_hash: Hash32::default(),
            superseded_facts: vec![],
            sequence: 1,
        }),
    )
    .with_agreement(Agreement::finalized(ConsensusId::new([2u8; 32])))
    .with_propagation(Propagation::complete())
    .with_ack_tracking();

    assert_eq!(fact1, fact2);
}

#[test]
fn fact_ordering_ignores_metadata() {
    let fact1 = Fact::new(
        OrderTime([1u8; 32]),
        TimeStamp::OrderClock(OrderTime([1u8; 32])),
        FactContent::Snapshot(SnapshotFact {
            state_hash: Hash32::default(),
            superseded_facts: vec![],
            sequence: 1,
        }),
    );

    let fact2 = Fact::new(
        OrderTime([2u8; 32]),
        TimeStamp::OrderClock(OrderTime([2u8; 32])),
        FactContent::Snapshot(SnapshotFact {
            state_hash: Hash32::default(),
            superseded_facts: vec![],
            sequence: 2,
        }),
    )
    .with_propagation(Propagation::complete());

    assert!(fact1 < fact2);
}

#[test]
fn same_order_different_content_are_distinct() {
    let namespace = JournalNamespace::Authority(AuthorityId::new_from_entropy([24u8; 32]));
    let mut journal = FactJournal::new(namespace);
    let shared_order = OrderTime([9u8; 32]);

    let fact1 = Fact::new(
        shared_order,
        TimeStamp::OrderClock(OrderTime([10u8; 32])),
        FactContent::Snapshot(SnapshotFact {
            state_hash: Hash32::new([1u8; 32]),
            superseded_facts: vec![],
            sequence: 1,
        }),
    );
    let fact2 = Fact::new(
        OrderTime([9u8; 32]),
        TimeStamp::OrderClock(OrderTime([11u8; 32])),
        FactContent::Snapshot(SnapshotFact {
            state_hash: Hash32::new([2u8; 32]),
            superseded_facts: vec![],
            sequence: 2,
        }),
    );

    journal.add_fact(fact1).unwrap();
    journal.add_fact(fact2).unwrap();
    assert_eq!(journal.size(), 2);
}

#[test]
fn gc_ack_tracking_basic() {
    let auth_id = AuthorityId::new_from_entropy([20u8; 32]);
    let namespace = JournalNamespace::Authority(auth_id);
    let mut journal = FactJournal::new(namespace);
    let mut ack_storage = AckStorage::new();

    let fact = Fact::new(
        OrderTime([1u8; 32]),
        TimeStamp::OrderClock(OrderTime([1u8; 32])),
        FactContent::Snapshot(SnapshotFact {
            state_hash: Hash32::default(),
            superseded_facts: vec![],
            sequence: 1,
        }),
    )
    .with_ack_tracking()
    .with_agreement(Agreement::Finalized {
        consensus_id: ConsensusId([1u8; 32]),
    });

    journal.add_fact(fact.clone()).unwrap();

    let peer = AuthorityId::new_from_entropy([21u8; 32]);
    ack_storage
        .record_ack(
            &fact.order,
            peer,
            PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            },
        )
        .unwrap();

    assert_eq!(ack_storage.len(), 1);
    assert_eq!(journal.ack_tracked_facts().count(), 1);

    let result = ack_storage.gc_by_consistency(&mut journal, |c| c.agreement.is_finalized());

    assert_eq!(result.facts_collected, 1);
    assert_eq!(result.facts_remaining, 0);
    assert!(ack_storage.is_empty());
    assert_eq!(journal.ack_tracked_facts().count(), 0);
}

#[test]
fn gc_ack_tracking_partial() {
    let auth_id = AuthorityId::new_from_entropy([22u8; 32]);
    let namespace = JournalNamespace::Authority(auth_id);
    let mut journal = FactJournal::new(namespace);
    let mut ack_storage = AckStorage::new();

    let fact1 = Fact::new(
        OrderTime([1u8; 32]),
        TimeStamp::OrderClock(OrderTime([1u8; 32])),
        FactContent::Snapshot(SnapshotFact {
            state_hash: Hash32::default(),
            superseded_facts: vec![],
            sequence: 1,
        }),
    )
    .with_ack_tracking()
    .with_agreement(Agreement::Finalized {
        consensus_id: ConsensusId([1u8; 32]),
    });
    let fact2 = Fact::new(
        OrderTime([2u8; 32]),
        TimeStamp::OrderClock(OrderTime([2u8; 32])),
        FactContent::Snapshot(SnapshotFact {
            state_hash: Hash32::default(),
            superseded_facts: vec![],
            sequence: 2,
        }),
    )
    .with_ack_tracking()
    .with_agreement(Agreement::Provisional);

    journal.add_fact(fact1.clone()).unwrap();
    journal.add_fact(fact2.clone()).unwrap();

    let peer = AuthorityId::new_from_entropy([23u8; 32]);
    ack_storage
        .record_ack(
            &fact1.order,
            peer,
            PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            },
        )
        .unwrap();
    ack_storage
        .record_ack(
            &fact2.order,
            peer,
            PhysicalTime {
                ts_ms: 2000,
                uncertainty: None,
            },
        )
        .unwrap();

    assert_eq!(ack_storage.len(), 2);
    assert_eq!(journal.ack_tracked_facts().count(), 2);

    let result = ack_storage.gc_by_consistency(&mut journal, |c| c.agreement.is_finalized());

    assert_eq!(result.facts_collected, 1);
    assert_eq!(result.facts_remaining, 1);
    assert_eq!(ack_storage.len(), 1);
    assert_eq!(journal.ack_tracked_facts().count(), 1);

    let remaining_fact = journal.get_fact(&fact2.order).unwrap();
    assert!(remaining_fact.ack_tracked);
}
