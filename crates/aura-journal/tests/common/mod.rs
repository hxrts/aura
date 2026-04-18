use aura_core::time::{OrderTime, PhysicalTime, TimeStamp};
use aura_core::types::identifiers::{AuthorityId, DeviceId};
use aura_core::Hash32;
use aura_journal::{Fact, FactContent, FactJournal, JournalNamespace, SnapshotFact};

pub fn make_authority_journal(seed: u8) -> FactJournal {
    let auth_id = AuthorityId::new_from_entropy([seed; 32]);
    FactJournal::new(JournalNamespace::Authority(auth_id))
}

pub fn make_snapshot_fact(order_byte: u8, sequence: u64) -> Fact {
    Fact::new(
        OrderTime([order_byte; 32]),
        TimeStamp::OrderClock(OrderTime([order_byte; 32])),
        FactContent::Snapshot(SnapshotFact {
            state_hash: Hash32::default(),
            superseded_facts: vec![],
            sequence,
        }),
    )
}

pub fn physical_time_ms(ts_ms: u64) -> TimeStamp {
    TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms,
        uncertainty: None,
    })
}

pub fn test_uuid(seed: u8) -> uuid::Uuid {
    uuid::Uuid::from_bytes([seed; 16])
}

pub fn test_device_id(seed: u8) -> DeviceId {
    DeviceId(test_uuid(seed))
}
