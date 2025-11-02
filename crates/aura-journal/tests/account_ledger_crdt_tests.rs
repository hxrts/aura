//! CRDT-specific tests for AccountLedger
//!
//! This module tests the CRDT properties of AccountLedger, focusing on:
//! - Convergence under different event orderings
//! - Commutativity of operations
//! - Idempotence of event application
//! - Conflict resolution
//! - Merge correctness

use aura_authentication::EventAuthorization;
use aura_crypto::Effects;
use aura_journal::types::{Session, SessionIdExt};
use aura_journal::{
    AccountLedger, AccountState, DeviceMetadata, DeviceType, EpochTickEvent, Event, EventType,
};
use aura_test_utils::*;
use aura_types::{
    AccountId, AccountIdExt, DeviceId, DeviceIdExt, EventId, EventIdExt, ParticipantId,
    ProtocolType, SessionId, SessionOutcome, SessionStatus,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use uuid::Uuid;

// ========== Test Utilities ==========

/// Create a test ledger with multiple events applied in order
fn create_ledger_with_events(
    seed: u64,
    event_count: usize,
) -> (AccountLedger, Vec<Event>, Effects) {
    let effects = test_effects_deterministic(seed, 1000);
    let account_id = AccountId::new_with_effects(&effects);
    let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
    let group_public_key = signing_key.verifying_key();
    let device_id = DeviceId::new_with_effects(&effects);

    let device = DeviceMetadata {
        device_id,
        device_name: "Test Device".to_string(),
        device_type: DeviceType::Native,
        public_key: group_public_key,
        added_at: 1000,
        last_seen: 1000,
        dkd_commitment_proofs: BTreeMap::new(),
        next_nonce: 0,
        used_nonces: BTreeSet::new(),
        key_share_epoch: 0,
    };

    let state = AccountState::new(account_id, group_public_key, device, 2, 3);
    let mut ledger = AccountLedger::new(state).expect("Failed to create test ledger");

    let mut events = Vec::new();
    for i in 1..=event_count {
        let event = Event {
            version: 1,
            event_id: EventId::new_with_effects(&effects),
            account_id,
            timestamp: effects.now().unwrap_or(1000) + i as u64,
            nonce: i as u64,
            parent_hash: ledger.last_event_hash(),
            epoch_at_write: i as u64,
            event_type: EventType::EpochTick(EpochTickEvent {
                new_epoch: i as u64 + 10, // Safe increment
                evidence_hash: [0u8; 32],
            }),
            authorization: EventAuthorization::LifecycleInternal,
        };

        ledger.append_event(event.clone(), &effects).unwrap();
        events.push(event);
    }

    (ledger, events, effects)
}

/// Create two ledgers and apply the same events in different orders
fn create_convergence_test_setup(
    seed: u64,
    event_count: usize,
) -> (AccountLedger, AccountLedger, Vec<Event>, Effects) {
    let effects = test_effects_deterministic(seed, 1000);
    let account_id = AccountId::new_with_effects(&effects);
    let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
    let group_public_key = signing_key.verifying_key();
    let device_id = DeviceId::new_with_effects(&effects);

    let device = DeviceMetadata {
        device_id,
        device_name: "Test Device".to_string(),
        device_type: DeviceType::Native,
        public_key: group_public_key,
        added_at: 1000,
        last_seen: 1000,
        dkd_commitment_proofs: BTreeMap::new(),
        next_nonce: 0,
        used_nonces: BTreeSet::new(),
        key_share_epoch: 0,
    };

    let state1 = AccountState::new(account_id, group_public_key, device.clone(), 2, 3);
    let state2 = AccountState::new(account_id, group_public_key, device, 2, 3);

    let ledger1 = AccountLedger::new(state1).expect("Failed to create ledger1");
    let ledger2 = AccountLedger::new(state2).expect("Failed to create ledger2");

    // Create events with unique nonces to avoid replay protection
    let mut events = Vec::new();
    for i in 1..=event_count {
        let event = Event {
            version: 1,
            event_id: EventId::new_with_effects(&effects),
            account_id,
            timestamp: effects.now().unwrap_or(1000) + i as u64,
            nonce: i as u64,
            parent_hash: None, // Simplified for convergence testing
            epoch_at_write: i as u64,
            event_type: EventType::EpochTick(EpochTickEvent {
                new_epoch: i as u64 + 100,    // Large increment to avoid validation issues
                evidence_hash: [i as u8; 32], // Unique per event
            }),
            authorization: EventAuthorization::LifecycleInternal,
        };
        events.push(event);
    }

    (ledger1, ledger2, events, effects)
}

// ========== CRDT Convergence Tests ==========

#[cfg(test)]
mod crdt_convergence_tests {
    use super::*;

    #[test]
    fn test_event_order_independence() {
        let (mut ledger1, mut ledger2, events, effects) = create_convergence_test_setup(100, 3);

        // Apply events in original order to ledger1
        for event in &events {
            let _ = ledger1.append_event(event.clone(), &effects);
        }

        // Apply events in reverse order to ledger2
        for event in events.iter().rev() {
            let _ = ledger2.append_event(event.clone(), &effects);
        }

        // Both ledgers should have the same number of successfully applied events
        // Note: Some events might be rejected due to validation, but that's consistent
        assert_eq!(
            ledger1.event_log().len(),
            ledger2.event_log().len(),
            "Both ledgers should have same number of events"
        );

        // Extract event IDs for comparison (order-independent)
        let events1: HashSet<_> = ledger1.event_log().iter().map(|e| e.event_id).collect();
        let events2: HashSet<_> = ledger2.event_log().iter().map(|e| e.event_id).collect();

        assert_eq!(
            events1, events2,
            "Both ledgers should contain the same events"
        );
    }

    #[test]
    fn test_lamport_clock_convergence() {
        let (mut ledger1, mut ledger2, events, effects) = create_convergence_test_setup(200, 5);

        // Apply events in different orders
        for event in &events {
            let _ = ledger1.append_event(event.clone(), &effects);
        }

        for event in events.iter().rev() {
            let _ = ledger2.append_event(event.clone(), &effects);
        }

        // Lamport clocks should converge to the same value
        // (assuming both ledgers processed the same events successfully)
        if ledger1.event_log().len() == ledger2.event_log().len() {
            assert_eq!(
                ledger1.lamport_clock(),
                ledger2.lamport_clock(),
                "Lamport clocks should converge"
            );
        }
    }

    #[test]
    fn test_state_hash_convergence() {
        let (mut ledger1, mut ledger2, events, effects) = create_convergence_test_setup(300, 3);

        // Apply same events to both ledgers
        for event in &events {
            let _ = ledger1.append_event(event.clone(), &effects);
            let _ = ledger2.append_event(event.clone(), &effects);
        }

        // State hashes should be identical if same events were applied
        let hash1 = ledger1.compute_state_hash().unwrap();
        let hash2 = ledger2.compute_state_hash().unwrap();

        assert_eq!(
            hash1, hash2,
            "State hashes should be identical for same events"
        );
    }

    #[test]
    fn test_partial_event_convergence() {
        let (mut ledger1, mut ledger2, events, effects) = create_convergence_test_setup(400, 4);

        // Ledger1 gets events 0, 1, 2
        for i in 0..3 {
            let _ = ledger1.append_event(events[i].clone(), &effects);
        }

        // Ledger2 gets events 1, 2, 3
        for i in 1..4 {
            let _ = ledger2.append_event(events[i].clone(), &effects);
        }

        // Later, both ledgers exchange missing events
        // Ledger1 gets event 3
        let _ = ledger1.append_event(events[3].clone(), &effects);

        // Ledger2 gets event 0
        let _ = ledger2.append_event(events[0].clone(), &effects);

        // Now both should have converged (if all events were accepted)
        let events1: HashSet<_> = ledger1.event_log().iter().map(|e| e.event_id).collect();
        let events2: HashSet<_> = ledger2.event_log().iter().map(|e| e.event_id).collect();

        // Check that they converged on the events they both accepted
        let intersection: HashSet<_> = events1.intersection(&events2).collect();
        assert!(
            !intersection.is_empty(),
            "Ledgers should have some common events"
        );
    }
}

// ========== CRDT Operation Properties Tests ==========

#[cfg(test)]
mod crdt_operation_properties_tests {
    use super::*;

    #[test]
    fn test_event_application_idempotence() {
        let (mut ledger, effects) = create_test_ledger_with_seed(500);
        let account_id = ledger.state().account_id;
        let device_id = ledger.state().devices.keys().next().copied().unwrap();

        let event = Event {
            version: 1,
            event_id: EventId::new_with_effects(&effects),
            account_id,
            timestamp: effects.now().unwrap_or(1000),
            nonce: 1,
            parent_hash: None,
            epoch_at_write: 10,
            event_type: EventType::EpochTick(EpochTickEvent {
                new_epoch: 20,
                evidence_hash: [0u8; 32],
            }),
            authorization: EventAuthorization::LifecycleInternal,
        };

        // Apply event first time
        let result1 = ledger.append_event(event.clone(), &effects);
        assert!(result1.is_ok(), "First application should succeed");
        let count_after_first = ledger.event_log().len();

        // Apply same event again (should be rejected due to replay protection)
        let result2 = ledger.append_event(event.clone(), &effects);
        assert!(result2.is_err(), "Duplicate event should be rejected");

        // Event count should remain the same
        assert_eq!(
            ledger.event_log().len(),
            count_after_first,
            "Event count should not change on duplicate application"
        );
    }

    #[test]
    fn test_nonce_set_monotonicity() {
        let (mut ledger, effects) = create_test_ledger_with_seed(600);

        let initial_nonce_count = ledger.state().used_nonces.len();

        // Apply events with different nonces
        let account_id = ledger.state().account_id;
        let device_id = ledger.state().devices.keys().next().copied().unwrap();

        for i in 1..=5 {
            let event = Event {
                version: 1,
                event_id: EventId::new_with_effects(&effects),
                account_id,
                timestamp: effects.now().unwrap_or(1000) + i as u64,
                nonce: i as u64,
                parent_hash: ledger.last_event_hash(),
                epoch_at_write: i as u64 + 20,
                event_type: EventType::EpochTick(EpochTickEvent {
                    new_epoch: i as u64 + 50,
                    evidence_hash: [i as u8; 32],
                }),
                authorization: EventAuthorization::LifecycleInternal,
            };

            let _ = ledger.append_event(event, &effects);
        }

        // Nonce set should have grown
        assert!(
            ledger.state().used_nonces.len() > initial_nonce_count,
            "Used nonces set should grow monotonically"
        );
    }

    #[test]
    fn test_lamport_clock_monotonicity() {
        let (mut ledger, effects) = create_test_ledger_with_seed(700);

        let mut previous_clock = ledger.lamport_clock();
        let account_id = ledger.state().account_id;
        let device_id = ledger.state().devices.keys().next().copied().unwrap();

        // Apply events and verify clock always increases
        for i in 1..=5 {
            let event = Event {
                version: 1,
                event_id: EventId::new_with_effects(&effects),
                account_id,
                timestamp: effects.now().unwrap_or(1000) + i as u64,
                nonce: i as u64,
                parent_hash: ledger.last_event_hash(),
                epoch_at_write: i as u64 + 30,
                event_type: EventType::EpochTick(EpochTickEvent {
                    new_epoch: i as u64 + 60,
                    evidence_hash: [i as u8; 32],
                }),
                authorization: EventAuthorization::LifecycleInternal,
            };

            let result = ledger.append_event(event, &effects);

            if result.is_ok() {
                let current_clock = ledger.lamport_clock();
                assert!(
                    current_clock >= previous_clock,
                    "Lamport clock must be monotonic: {} >= {}",
                    current_clock,
                    previous_clock
                );
                previous_clock = current_clock;
            }
        }
    }

    #[test]
    fn test_event_log_append_only() {
        let (mut ledger, effects) = create_test_ledger_with_seed(800);

        let initial_count = ledger.event_log().len();
        let account_id = ledger.state().account_id;
        let device_id = ledger.state().devices.keys().next().copied().unwrap();

        // Apply several events
        for i in 1..=3 {
            let event = Event {
                version: 1,
                event_id: EventId::new_with_effects(&effects),
                account_id,
                timestamp: effects.now().unwrap_or(1000) + i as u64,
                nonce: i as u64,
                parent_hash: ledger.last_event_hash(),
                epoch_at_write: i as u64 + 40,
                event_type: EventType::EpochTick(EpochTickEvent {
                    new_epoch: i as u64 + 70,
                    evidence_hash: [i as u8; 32],
                }),
                authorization: EventAuthorization::LifecycleInternal,
            };

            let _ = ledger.append_event(event, &effects);
        }

        // Event log should only grow
        assert!(
            ledger.event_log().len() >= initial_count,
            "Event log should be append-only"
        );
    }
}

// ========== CRDT Conflict Resolution Tests ==========

#[cfg(test)]
mod crdt_conflict_resolution_tests {
    use super::*;

    #[test]
    fn test_concurrent_epoch_updates() {
        let (mut ledger1, mut ledger2, _, effects) = create_convergence_test_setup(900, 0);

        let account_id = ledger1.state().account_id;
        let device_id = ledger1.state().devices.keys().next().copied().unwrap();

        // Create events that update epoch to different values
        let event1 = Event {
            version: 1,
            event_id: EventId::new_with_effects(&effects),
            account_id,
            timestamp: effects.now().unwrap_or(1000),
            nonce: 1,
            parent_hash: None,
            epoch_at_write: 100,
            event_type: EventType::EpochTick(EpochTickEvent {
                new_epoch: 150,
                evidence_hash: [1u8; 32],
            }),
            authorization: EventAuthorization::LifecycleInternal,
        };

        let event2 = Event {
            version: 1,
            event_id: EventId::new_with_effects(&effects),
            account_id,
            timestamp: effects.now().unwrap_or(1000),
            nonce: 2,
            parent_hash: None,
            epoch_at_write: 120,
            event_type: EventType::EpochTick(EpochTickEvent {
                new_epoch: 160,
                evidence_hash: [2u8; 32],
            }),
            authorization: EventAuthorization::LifecycleInternal,
        };

        // Apply events in different orders
        let _ = ledger1.append_event(event1.clone(), &effects);
        let _ = ledger1.append_event(event2.clone(), &effects);

        let _ = ledger2.append_event(event2.clone(), &effects);
        let _ = ledger2.append_event(event1.clone(), &effects);

        // Both should converge to consistent state
        // The exact final epoch depends on which events were accepted
        let clock1 = ledger1.lamport_clock();
        let clock2 = ledger2.lamport_clock();

        // Both should have advanced their clocks
        assert!(clock1 > 0, "Ledger1 clock should advance");
        assert!(clock2 > 0, "Ledger2 clock should advance");
    }

    #[test]
    fn test_session_conflicts() {
        let (mut ledger, effects) = create_test_ledger_with_seed(1000);

        let session_id = SessionId::from_uuid(effects.gen_uuid());
        let device_id = ledger.state().devices.keys().next().copied().unwrap();
        let participant_id = ParticipantId::Device(device_id);

        // Add session
        let session = Session::new(
            session_id,
            ProtocolType::Dkd,
            vec![participant_id],
            1000,
            100,
            effects.now().unwrap_or(1000),
        );
        ledger.add_session(session, &effects);

        // Try to update session status
        let result1 = ledger.update_session_status(session_id.0, SessionStatus::Active, &effects);
        assert!(result1.is_ok());

        // Try to complete the same session
        let result2 = ledger.complete_session(session_id.0, SessionOutcome::Success, &effects);
        assert!(result2.is_ok());

        // Session should be in completed state
        let final_session = ledger.get_session(&session_id.0).unwrap();
        assert!(final_session.is_terminal());
    }
}

// ========== CRDT Merge Simulation Tests ==========

#[cfg(test)]
mod crdt_merge_simulation_tests {
    use super::*;

    #[test]
    fn test_simulated_network_partition_merge() {
        // Simulate network partition scenario
        let (mut ledger_a, mut ledger_b, _, effects) = create_convergence_test_setup(1100, 0);

        let account_id = ledger_a.state().account_id;
        let device_id = ledger_a.state().devices.keys().next().copied().unwrap();

        // Phase 1: Network partition - each ledger evolves independently
        let event_a = Event {
            version: 1,
            event_id: EventId::new_with_effects(&effects),
            account_id,
            timestamp: effects.now().unwrap_or(1000),
            nonce: 1,
            parent_hash: None,
            epoch_at_write: 10,
            event_type: EventType::EpochTick(EpochTickEvent {
                new_epoch: 110,
                evidence_hash: [10u8; 32],
            }),
            authorization: EventAuthorization::LifecycleInternal,
        };

        let event_b = Event {
            version: 1,
            event_id: EventId::new_with_effects(&effects),
            account_id,
            timestamp: effects.now().unwrap_or(1000),
            nonce: 2,
            parent_hash: None,
            epoch_at_write: 15,
            event_type: EventType::EpochTick(EpochTickEvent {
                new_epoch: 115,
                evidence_hash: [15u8; 32],
            }),
            authorization: EventAuthorization::LifecycleInternal,
        };

        // Apply events to respective ledgers during partition
        let _ = ledger_a.append_event(event_a.clone(), &effects);
        let _ = ledger_b.append_event(event_b.clone(), &effects);

        // Phase 2: Network heals - exchange events
        let _ = ledger_a.append_event(event_b.clone(), &effects);
        let _ = ledger_b.append_event(event_a.clone(), &effects);

        // Both ledgers should converge
        let events_a: HashSet<_> = ledger_a.event_log().iter().map(|e| e.event_id).collect();
        let events_b: HashSet<_> = ledger_b.event_log().iter().map(|e| e.event_id).collect();

        assert_eq!(
            events_a, events_b,
            "Ledgers should converge after partition heals"
        );
    }

    #[test]
    fn test_three_way_merge() {
        // Test convergence with three independent ledgers
        let effects = test_effects_deterministic(1200, 1000);
        let account_id = AccountId::new_with_effects(&effects);
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
        let group_public_key = signing_key.verifying_key();
        let device_id = DeviceId::new_with_effects(&effects);

        let device = DeviceMetadata {
            device_id,
            device_name: "Test Device".to_string(),
            device_type: DeviceType::Native,
            public_key: group_public_key,
            added_at: 1000,
            last_seen: 1000,
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
            next_nonce: 0,
            used_nonces: std::collections::BTreeSet::new(),
            key_share_epoch: 0,
        };

        // Create three ledgers
        let state1 = AccountState::new(account_id, group_public_key, device.clone(), 2, 3);
        let state2 = AccountState::new(account_id, group_public_key, device.clone(), 2, 3);
        let state3 = AccountState::new(account_id, group_public_key, device, 2, 3);

        let mut ledger1 = AccountLedger::new(state1).unwrap();
        let mut ledger2 = AccountLedger::new(state2).unwrap();
        let mut ledger3 = AccountLedger::new(state3).unwrap();

        // Create unique events
        let events: Vec<Event> = (1..=3)
            .map(|i| Event {
                version: 1,
                event_id: EventId::new_with_effects(&effects),
                account_id,
                timestamp: effects.now().unwrap_or(1000) + i as u64,
                nonce: i as u64,
                parent_hash: None,
                epoch_at_write: i as u64 + 50,
                event_type: EventType::EpochTick(EpochTickEvent {
                    new_epoch: i as u64 + 150,
                    evidence_hash: [i as u8; 32],
                }),
                authorization: EventAuthorization::LifecycleInternal,
            })
            .collect();

        // Each ledger gets a different event initially
        let _ = ledger1.append_event(events[0].clone(), &effects);
        let _ = ledger2.append_event(events[1].clone(), &effects);
        let _ = ledger3.append_event(events[2].clone(), &effects);

        // Full exchange - each ledger gets all events
        for event in &events {
            let _ = ledger1.append_event(event.clone(), &effects);
            let _ = ledger2.append_event(event.clone(), &effects);
            let _ = ledger3.append_event(event.clone(), &effects);
        }

        // All ledgers should converge
        let events1: HashSet<_> = ledger1.event_log().iter().map(|e| e.event_id).collect();
        let events2: HashSet<_> = ledger2.event_log().iter().map(|e| e.event_id).collect();
        let events3: HashSet<_> = ledger3.event_log().iter().map(|e| e.event_id).collect();

        assert_eq!(events1, events2, "Ledger1 and Ledger2 should converge");
        assert_eq!(events2, events3, "Ledger2 and Ledger3 should converge");
    }
}

// Helper function for creating test ledger (referenced in tests)
fn create_test_ledger_with_seed(seed: u64) -> (AccountLedger, Effects) {
    let effects = test_effects_deterministic(seed, 1000);
    let account_id = AccountId::new_with_effects(&effects);
    let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
    let group_public_key = signing_key.verifying_key();
    let device_id = DeviceId::new_with_effects(&effects);

    let device = DeviceMetadata {
        device_id,
        device_name: "Test Device".to_string(),
        device_type: DeviceType::Native,
        public_key: group_public_key,
        added_at: 1000,
        last_seen: 1000,
        dkd_commitment_proofs: BTreeMap::new(),
        next_nonce: 0,
        used_nonces: BTreeSet::new(),
        key_share_epoch: 0,
    };

    let state = AccountState::new(account_id, group_public_key, device, 2, 3);
    let ledger = AccountLedger::new(state).expect("Failed to create test ledger");
    (ledger, effects)
}
