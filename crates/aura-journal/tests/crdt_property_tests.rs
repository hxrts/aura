// DISABLED: This test uses an outdated journal API that has been refactored
// TODO: Update tests to use the new Event/AccountLedger system
#![cfg(disabled)]
// CRDT Property Tests using proptest
//
// Property-based tests that verify CRDT properties hold for all inputs:
// - Convergence: Any two replicas receiving same events converge
// - Commutativity: Order of applying events doesn't matter
// - Idempotence: Applying same event multiple times has same effect as once
// - Associativity: Grouping of operations doesn't matter
// - Monotonicity: State only grows, never shrinks (for grow-only CRDTs)

use aura_journal::{EventData, Journal, JournalEvent};
use aura_types::DeviceId;
use proptest::prelude::*;
use std::collections::HashSet;
use uuid::Uuid;

// Strategy to generate arbitrary device IDs
fn device_id_strategy() -> impl Strategy<Value = DeviceId> {
    any::<[u8; 16]>().prop_map(|bytes| DeviceId(Uuid::from_bytes(bytes)))
}

// Strategy to generate arbitrary journal events
fn journal_event_strategy() -> impl Strategy<Value = JournalEvent> {
    (
        any::<[u8; 16]>(), // event_id
        device_id_strategy(),
        any::<u64>(),                               // sequence_number
        any::<u64>(),                               // timestamp
        prop::collection::vec(any::<u8>(), 0..100), // payload
    )
        .prop_map(
            |(event_id_bytes, device_id, sequence, timestamp, payload)| JournalEvent {
                event_id: Uuid::from_bytes(event_id_bytes),
                device_id,
                sequence_number: sequence,
                timestamp,
                data: EventData::Custom {
                    event_type: "test".to_string(),
                    payload,
                },
                signature: vec![0u8; 64],
            },
        )
}

// Strategy to generate a list of events
fn event_list_strategy() -> impl Strategy<Value = Vec<JournalEvent>> {
    prop::collection::vec(journal_event_strategy(), 1..20)
}

proptest! {
    /// Property: CRDT Convergence
    /// Two journals receiving the same set of events in different orders converge to same state
    #[test]
    fn prop_crdt_convergence(events in event_list_strategy()) {
        let mut journal1 = Journal::new();
        let mut journal2 = Journal::new();

        // Journal 1: Apply events in original order
        for event in &events {
            journal1.apply_event(event.clone());
        }

        // Journal 2: Apply events in reverse order
        for event in events.iter().rev() {
            journal2.apply_event(event.clone());
        }

        // Extract event IDs (order-independent comparison)
        let events1: HashSet<_> = journal1.events().iter().map(|e| e.event_id).collect();
        let events2: HashSet<_> = journal2.events().iter().map(|e| e.event_id).collect();

        prop_assert_eq!(events1, events2, "Journals must converge regardless of event order");
        prop_assert_eq!(journal1.events().len(), journal2.events().len(),
            "Journals must have same number of events");
    }

    /// Property: CRDT Commutativity
    /// For any two events A and B, applying A then B is equivalent to B then A
    #[test]
    fn prop_crdt_commutativity(
        event_a in journal_event_strategy(),
        event_b in journal_event_strategy()
    ) {
        // Skip if events are identical
        prop_assume!(event_a.event_id != event_b.event_id);

        let mut journal1 = Journal::new();
        let mut journal2 = Journal::new();

        // Journal 1: A then B
        journal1.apply_event(event_a.clone());
        journal1.apply_event(event_b.clone());

        // Journal 2: B then A
        journal2.apply_event(event_b.clone());
        journal2.apply_event(event_a.clone());

        // Both should converge to same state
        let events1: HashSet<_> = journal1.events().iter().map(|e| e.event_id).collect();
        let events2: HashSet<_> = journal2.events().iter().map(|e| e.event_id).collect();

        prop_assert_eq!(events1, events2,
            "Commutativity: order of applying events should not matter");
    }

    /// Property: CRDT Idempotence
    /// Applying the same event multiple times has the same effect as applying it once
    #[test]
    fn prop_crdt_idempotence(event in journal_event_strategy()) {
        let mut journal1 = Journal::new();
        let mut journal2 = Journal::new();

        // Journal 1: Apply event once
        journal1.apply_event(event.clone());

        // Journal 2: Apply event multiple times
        journal2.apply_event(event.clone());
        journal2.apply_event(event.clone());
        journal2.apply_event(event.clone());

        // Both should have exactly one event
        prop_assert_eq!(journal1.events().len(), 1,
            "Journal 1 should have exactly 1 event");
        prop_assert_eq!(journal2.events().len(), 1,
            "Journal 2 should have exactly 1 event (idempotent)");

        let events1: HashSet<_> = journal1.events().iter().map(|e| e.event_id).collect();
        let events2: HashSet<_> = journal2.events().iter().map(|e| e.event_id).collect();

        prop_assert_eq!(events1, events2,
            "Idempotence: multiple applications should be equivalent to single application");
    }

    /// Property: CRDT Associativity
    /// Grouping of event applications doesn't matter: (A ∪ B) ∪ C = A ∪ (B ∪ C)
    #[test]
    fn prop_crdt_associativity(
        events_a in event_list_strategy(),
        events_b in event_list_strategy(),
        events_c in event_list_strategy()
    ) {
        let mut journal1 = Journal::new();
        let mut journal2 = Journal::new();

        // Journal 1: (A ∪ B) ∪ C
        for event in &events_a {
            journal1.apply_event(event.clone());
        }
        for event in &events_b {
            journal1.apply_event(event.clone());
        }
        for event in &events_c {
            journal1.apply_event(event.clone());
        }

        // Journal 2: A ∪ (B ∪ C) - different grouping
        for event in &events_a {
            journal2.apply_event(event.clone());
        }
        for event in events_b.iter().chain(events_c.iter()) {
            journal2.apply_event(event.clone());
        }

        let events1: HashSet<_> = journal1.events().iter().map(|e| e.event_id).collect();
        let events2: HashSet<_> = journal2.events().iter().map(|e| e.event_id).collect();

        prop_assert_eq!(events1, events2,
            "Associativity: grouping of operations should not matter");
    }

    /// Property: Monotonicity (for add-only CRDT)
    /// Number of events only increases or stays the same, never decreases
    #[test]
    fn prop_crdt_monotonicity(events in event_list_strategy()) {
        let mut journal = Journal::new();
        let mut previous_count = 0;

        for event in events {
            journal.apply_event(event);
            let current_count = journal.events().len();

            prop_assert!(current_count >= previous_count,
                "Monotonicity: event count should never decrease ({} -> {})",
                previous_count, current_count);

            previous_count = current_count;
        }
    }

    /// Property: Event ID uniqueness within journal
    /// All events in a journal have unique event IDs
    #[test]
    fn prop_event_id_uniqueness(events in event_list_strategy()) {
        let mut journal = Journal::new();

        for event in &events {
            journal.apply_event(event.clone());
        }

        let event_ids: Vec<_> = journal.events().iter().map(|e| e.event_id).collect();
        let unique_ids: HashSet<_> = event_ids.iter().collect();

        prop_assert_eq!(event_ids.len(), unique_ids.len(),
            "All event IDs in journal must be unique");
    }

    /// Property: Convergence under arbitrary reordering
    /// For any permutation of events, all journals converge
    #[test]
    fn prop_convergence_arbitrary_order(
        events in event_list_strategy(),
        seed in any::<u64>()
    ) {
        use rand::{SeedableRng, seq::SliceRandom};
        use rand::rngs::StdRng;

        let mut journal1 = Journal::new();
        let mut journal2 = Journal::new();
        let mut journal3 = Journal::new();

        // Journal 1: Original order
        for event in &events {
            journal1.apply_event(event.clone());
        }

        // Journal 2: Random permutation 1
        let mut events_perm1 = events.clone();
        let mut rng1 = StdRng::seed_from_u64(seed);
        events_perm1.shuffle(&mut rng1);
        for event in &events_perm1 {
            journal2.apply_event(event.clone());
        }

        // Journal 3: Random permutation 2
        let mut events_perm2 = events.clone();
        let mut rng2 = StdRng::seed_from_u64(seed.wrapping_add(1));
        events_perm2.shuffle(&mut rng2);
        for event in &events_perm2 {
            journal3.apply_event(event.clone());
        }

        // All journals must converge
        let events1: HashSet<_> = journal1.events().iter().map(|e| e.event_id).collect();
        let events2: HashSet<_> = journal2.events().iter().map(|e| e.event_id).collect();
        let events3: HashSet<_> = journal3.events().iter().map(|e| e.event_id).collect();

        prop_assert_eq!(events1, events2,
            "Journals with different orderings must converge");
        prop_assert_eq!(events1, events3,
            "Journals with different orderings must converge");
    }

    /// Property: Duplicate events from different devices
    /// Events from different devices with same sequence numbers coexist
    #[test]
    fn prop_per_device_sequences(
        device1 in device_id_strategy(),
        device2 in device_id_strategy(),
        sequence in any::<u64>(),
        timestamp in any::<u64>()
    ) {
        prop_assume!(device1 != device2);

        let mut journal = Journal::new();

        let event1 = JournalEvent {
            event_id: Uuid::new_v4(),
            device_id: device1,
            sequence_number: sequence,
            timestamp,
            data: EventData::Custom {
                event_type: "test".to_string(),
                payload: b"device1".to_vec(),
            },
            signature: vec![0u8; 64],
        };

        let event2 = JournalEvent {
            event_id: Uuid::new_v4(),
            device_id: device2,
            sequence_number: sequence, // Same sequence number
            timestamp,
            data: EventData::Custom {
                event_type: "test".to_string(),
                payload: b"device2".to_vec(),
            },
            signature: vec![0u8; 64],
        };

        journal.apply_event(event1.clone());
        journal.apply_event(event2.clone());

        // Both events should be present (sequence numbers are per-device)
        prop_assert_eq!(journal.events().len(), 2,
            "Events from different devices with same sequence number should coexist");

        let event_ids: HashSet<_> = journal.events().iter().map(|e| e.event_id).collect();
        prop_assert!(event_ids.contains(&event1.event_id));
        prop_assert!(event_ids.contains(&event2.event_id));
    }

    /// Property: Merge idempotence
    /// Merging two journals multiple times is equivalent to merging once
    #[test]
    fn prop_merge_idempotence(
        events1 in event_list_strategy(),
        events2 in event_list_strategy()
    ) {
        let mut journal_a1 = Journal::new();
        let mut journal_b1 = Journal::new();

        // Setup: journal_a has events1, journal_b has events2
        for event in &events1 {
            journal_a1.apply_event(event.clone());
        }
        for event in &events2 {
            journal_b1.apply_event(event.clone());
        }

        // Merge once
        let mut merged_once = journal_a1.clone();
        for event in journal_b1.events() {
            merged_once.apply_event(event.clone());
        }

        // Merge multiple times
        let mut merged_multiple = journal_a1.clone();
        for _ in 0..3 {
            for event in journal_b1.events() {
                merged_multiple.apply_event(event.clone());
            }
        }

        let events_once: HashSet<_> = merged_once.events().iter().map(|e| e.event_id).collect();
        let events_multiple: HashSet<_> = merged_multiple.events().iter().map(|e| e.event_id).collect();

        prop_assert_eq!(events_once, events_multiple,
            "Merging multiple times should be equivalent to merging once");
    }

    /// Property: Convergence with gaps
    /// Journals converge even when events arrive with sequence number gaps
    #[test]
    fn prop_convergence_with_gaps(
        device in device_id_strategy(),
        sequences in prop::collection::vec(any::<u64>(), 5..10)
    ) {
        let mut journal1 = Journal::new();
        let mut journal2 = Journal::new();

        // Create events with arbitrary sequence numbers (may have gaps)
        let events: Vec<_> = sequences.iter().enumerate()
            .map(|(i, &seq)| JournalEvent {
                event_id: Uuid::new_v4(),
                device_id: device,
                sequence_number: seq,
                timestamp: (i as u64) * 1000,
                data: EventData::Custom {
                    event_type: "test".to_string(),
                    payload: format!("event-{}", i).into_bytes(),
                },
                signature: vec![0u8; 64],
            })
            .collect();

        // Journal 1: Ascending order
        let mut sorted_events = events.clone();
        sorted_events.sort_by_key(|e| e.sequence_number);
        for event in &sorted_events {
            journal1.apply_event(event.clone());
        }

        // Journal 2: Descending order
        for event in sorted_events.iter().rev() {
            journal2.apply_event(event.clone());
        }

        let events1: HashSet<_> = journal1.events().iter().map(|e| e.event_id).collect();
        let events2: HashSet<_> = journal2.events().iter().map(|e| e.event_id).collect();

        prop_assert_eq!(events1, events2,
            "Journals must converge even with sequence number gaps");
    }

    /// Property: Eventual consistency under partition
    /// After network partition heals, all replicas converge
    #[test]
    fn prop_eventual_consistency_after_partition(
        partition1_events in event_list_strategy(),
        partition2_events in event_list_strategy()
    ) {
        // Phase 1: Network partition - journals evolve independently
        let mut journal_p1 = Journal::new();
        let mut journal_p2 = Journal::new();

        for event in &partition1_events {
            journal_p1.apply_event(event.clone());
        }
        for event in &partition2_events {
            journal_p2.apply_event(event.clone());
        }

        // Phase 2: Partition heals - exchange all events
        let mut healed1 = journal_p1.clone();
        let mut healed2 = journal_p2.clone();

        for event in journal_p2.events() {
            healed1.apply_event(event.clone());
        }
        for event in journal_p1.events() {
            healed2.apply_event(event.clone());
        }

        // Verify convergence
        let events_healed1: HashSet<_> = healed1.events().iter().map(|e| e.event_id).collect();
        let events_healed2: HashSet<_> = healed2.events().iter().map(|e| e.event_id).collect();

        prop_assert_eq!(events_healed1, events_healed2,
            "Journals must converge after partition heals");

        // Verify all events are preserved
        let all_original_events: HashSet<_> = partition1_events.iter()
            .chain(partition2_events.iter())
            .map(|e| e.event_id)
            .collect();

        for event_id in &all_original_events {
            prop_assert!(events_healed1.contains(event_id),
                "All events must be preserved after partition heals");
        }
    }
}
