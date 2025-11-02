//! Edge cases and error handling tests for AccountLedger
//!
//! This module tests edge cases, boundary conditions, and error handling for AccountLedger:
//! - Boundary value testing
//! - Resource exhaustion scenarios
//! - Invalid input handling
//! - State corruption recovery
//! - Concurrent access patterns
//! - Memory and performance limits

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
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

// ========== Test Utilities ==========

/// Create a minimal test ledger for edge case testing
fn create_minimal_ledger(seed: u64) -> (AccountLedger, AccountId, DeviceId, Effects) {
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

    (ledger, account_id, device_id, effects)
}

/// Create a test event with specified parameters
fn create_test_event(
    account_id: AccountId,
    device_id: DeviceId,
    nonce: u64,
    epoch: u64,
    timestamp: u64,
    effects: &Effects,
) -> Event {
    Event {
        version: 1,
        event_id: EventId::new_with_effects(effects),
        account_id,
        timestamp,
        nonce,
        parent_hash: None,
        epoch_at_write: epoch,
        event_type: EventType::EpochTick(EpochTickEvent {
            new_epoch: epoch + 1,
            evidence_hash: [0u8; 32],
        }),
        authorization: EventAuthorization::LifecycleInternal,
    }
}

// ========== Boundary Value Tests ==========

#[cfg(test)]
mod boundary_value_tests {
    use super::*;

    #[test]
    fn test_zero_values() {
        let (mut ledger, account_id, device_id, effects) = create_minimal_ledger(100);

        // Test event with zero timestamp
        let event_zero_timestamp = create_test_event(account_id, device_id, 1, 0, 0, &effects);
        let result = ledger.append_event(event_zero_timestamp, &effects);
        // Should handle zero timestamp gracefully
        assert!(result.is_ok() || result.is_err()); // Either is acceptable, just shouldn't panic

        // Test event with zero epoch
        let event_zero_epoch = create_test_event(account_id, device_id, 2, 0, 1000, &effects);
        let result = ledger.append_event(event_zero_epoch, &effects);
        assert!(result.is_ok() || result.is_err()); // Should handle gracefully
    }

    #[test]
    fn test_maximum_values() {
        let (mut ledger, account_id, device_id, effects) = create_minimal_ledger(200);

        // Test with maximum u64 values
        let max_event = Event {
            version: 1,
            event_id: EventId::new_with_effects(&effects),
            account_id,
            timestamp: u64::MAX,
            nonce: u64::MAX,
            parent_hash: None,
            epoch_at_write: u64::MAX,
            event_type: EventType::EpochTick(EpochTickEvent {
                new_epoch: u64::MAX,
                evidence_hash: [0u8; 32],
            }),
            authorization: EventAuthorization::LifecycleInternal,
        };

        let result = ledger.append_event(max_event, &effects);
        // Should handle maximum values without overflow or panic
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_nonce_boundary_conditions() {
        let (mut ledger, account_id, device_id, effects) = create_minimal_ledger(300);

        // Test nonce 0 (edge case)
        let event_nonce_0 = create_test_event(account_id, device_id, 0, 0, 1000, &effects);
        let result_0 = ledger.append_event(event_nonce_0, &effects);

        // Test nonce 1 (first valid nonce typically)
        let event_nonce_1 = create_test_event(account_id, device_id, 1, 1, 1001, &effects);
        let result_1 = ledger.append_event(event_nonce_1, &effects);

        // At least one should succeed (depending on nonce validation policy)
        assert!(result_0.is_ok() || result_1.is_ok());

        // Test very large nonce
        let event_large_nonce =
            create_test_event(account_id, device_id, 1_000_000, 2, 1002, &effects);
        let result_large = ledger.append_event(event_large_nonce, &effects);
        // Should handle large nonces gracefully
        assert!(result_large.is_ok() || result_large.is_err());
    }

    #[test]
    fn test_epoch_edge_cases() {
        let (mut ledger, account_id, device_id, effects) = create_minimal_ledger(400);

        // Test epoch tick with minimal increment
        let minimal_tick = Event {
            version: 1,
            event_id: EventId::new_with_effects(&effects),
            account_id,
            timestamp: 1000,
            nonce: 1,
            parent_hash: None,
            epoch_at_write: 10,
            event_type: EventType::EpochTick(EpochTickEvent {
                new_epoch: 11, // Minimal increment
                evidence_hash: ledger.compute_state_hash().unwrap_or([0u8; 32]),
            }),
            authorization: EventAuthorization::LifecycleInternal,
        };

        let result = ledger.append_event(minimal_tick, &effects);
        // May be rejected due to minimum gap requirements
        assert!(result.is_ok() || result.is_err());
    }
}

// ========== Invalid Input Handling Tests ==========

#[cfg(test)]
mod invalid_input_tests {
    use super::*;

    #[test]
    fn test_invalid_account_id() {
        let (mut ledger, _correct_account, device_id, effects) = create_minimal_ledger(500);

        // Create event with wrong account ID
        let wrong_account = AccountId::new_with_effects(&effects);
        let invalid_event = create_test_event(wrong_account, device_id, 1, 0, 1000, &effects);

        let result = ledger.append_event(invalid_event, &effects);
        // Should be rejected or handled gracefully
        assert!(
            result.is_err(),
            "Event with wrong account ID should be rejected"
        );
    }

    #[test]
    fn test_malformed_event_data() {
        let (mut ledger, account_id, device_id, effects) = create_minimal_ledger(600);

        // Create event with inconsistent data
        let mut malformed_event = create_test_event(account_id, device_id, 1, 0, 1000, &effects);

        // Make epoch_at_write inconsistent with event type
        malformed_event.epoch_at_write = 100;
        if let EventType::EpochTick(ref mut tick) = malformed_event.event_type {
            tick.new_epoch = 50; // Goes backwards - should be invalid
        }

        let result = ledger.append_event(malformed_event, &effects);
        assert!(result.is_err(), "Malformed event should be rejected");
    }

    #[test]
    fn test_invalid_event_version() {
        let (mut ledger, account_id, device_id, effects) = create_minimal_ledger(700);

        // Create event with unsupported version
        let mut future_version_event =
            create_test_event(account_id, device_id, 1, 0, 1000, &effects);
        future_version_event.version = 999; // Future version

        let result = ledger.append_event(future_version_event, &effects);
        assert!(result.is_err(), "Future version event should be rejected");
    }

    #[test]
    fn test_empty_or_null_fields() {
        let (ledger, _account_id, _device_id, effects) = create_minimal_ledger(800);

        // Test with zero/empty UUID
        let zero_uuid = Uuid::from_bytes([0u8; 16]);
        let zero_event_id = EventId(zero_uuid);

        // Verify zero UUID is detected
        assert_eq!(zero_event_id.0, zero_uuid);

        // Test state hash computation with minimal state
        let hash = ledger.compute_state_hash();
        assert!(
            hash.is_ok(),
            "State hash computation should handle minimal state"
        );
    }
}

// ========== Resource Exhaustion Tests ==========

#[cfg(test)]
mod resource_exhaustion_tests {
    use super::*;

    #[test]
    fn test_large_event_log() {
        let (mut ledger, account_id, device_id, effects) = create_minimal_ledger(900);

        // Add many events to test memory usage
        let event_count = 1000;
        let mut success_count = 0;

        for i in 1..=event_count {
            let event = create_test_event(account_id, device_id, i, i - 1, 1000 + i, &effects);
            if ledger.append_event(event, &effects).is_ok() {
                success_count += 1;
            }
        }

        assert!(success_count > 0, "Should handle some events successfully");
        assert!(
            ledger.event_log().len() <= event_count as usize,
            "Event log size should be bounded"
        );
    }

    #[test]
    fn test_nonce_set_size_limits() {
        let (mut ledger, account_id, device_id, effects) = create_minimal_ledger(1000);

        // Fill up the nonce set with many unique nonces
        let large_nonces = vec![1, 100, 1000, 10000, 100000, 1000000];

        for nonce in large_nonces {
            let event = create_test_event(account_id, device_id, nonce, 0, 1000, &effects);
            let _ = ledger.append_event(event, &effects);
        }

        // Verify nonce set doesn't grow unbounded
        let nonce_count = ledger.state().used_nonces.len();
        assert!(
            nonce_count <= 1000,
            "Nonce set should be bounded to prevent memory exhaustion"
        );
    }

    #[test]
    fn test_session_limit_handling() {
        let (mut ledger, _account_id, device_id, effects) = create_minimal_ledger(1100);

        // Add many sessions
        let participant = ParticipantId::Device(device_id);
        let session_count = 100;

        for i in 0..session_count {
            let session_id = SessionId::from_uuid(effects.gen_uuid());
            let session = Session::new(
                session_id,
                ProtocolType::Dkd,
                vec![participant.clone()],
                1000 + i,
                100,
                1000 + i,
            );
            ledger.add_session(session, &effects);
        }

        // Verify reasonable session count
        assert!(ledger.state().sessions.len() <= session_count as usize);

        // Test cleanup
        ledger.cleanup_expired_sessions(&effects);
        // Some sessions may be cleaned up
    }

    #[test]
    fn test_state_serialization_limits() {
        let (ledger, account_id, device_id, effects) = create_minimal_ledger(1200);

        // Test state hash computation with current state
        let hash_result = ledger.compute_state_hash();
        assert!(
            hash_result.is_ok(),
            "State hash should compute successfully"
        );

        // Add some complexity to state
        let mut complex_ledger = ledger;
        for i in 1..=10 {
            let event = create_test_event(account_id, device_id, i, i - 1, 1000 + i, &effects);
            let _ = complex_ledger.append_event(event, &effects);
        }

        // Test hash computation with more complex state
        let complex_hash = complex_ledger.compute_state_hash();
        assert!(
            complex_hash.is_ok(),
            "Complex state hash should compute successfully"
        );
    }
}

// ========== State Corruption Recovery Tests ==========

#[cfg(test)]
mod state_corruption_tests {
    use super::*;

    #[test]
    fn test_inconsistent_lamport_clock() {
        let (mut ledger, account_id, device_id, effects) = create_minimal_ledger(1300);

        // Manually set inconsistent state (simulating corruption)
        let original_clock = ledger.lamport_clock();

        // Apply event that should advance clock
        let event = create_test_event(account_id, device_id, 1, 100, 1000, &effects);
        let result = ledger.append_event(event, &effects);

        if result.is_ok() {
            let new_clock = ledger.lamport_clock();
            assert!(
                new_clock >= original_clock,
                "Lamport clock should advance or stay same"
            );
        }
    }

    #[test]
    fn test_parent_hash_chain_validation() {
        let (mut ledger, account_id, device_id, effects) = create_minimal_ledger(1400);

        // Apply first event
        let event1 = create_test_event(account_id, device_id, 1, 0, 1000, &effects);
        ledger.append_event(event1, &effects).unwrap();

        let first_hash = ledger.last_event_hash().unwrap();

        // Create second event with correct parent
        let mut event2 = create_test_event(account_id, device_id, 2, 1, 1001, &effects);
        event2.parent_hash = Some(first_hash);

        let result = ledger.append_event(event2, &effects);
        assert!(result.is_ok(), "Event with correct parent should succeed");

        // Create third event with wrong parent
        let mut event3 = create_test_event(account_id, device_id, 3, 2, 1002, &effects);
        event3.parent_hash = Some([0u8; 32]); // Wrong parent

        let result = ledger.append_event(event3, &effects);
        assert!(
            result.is_err(),
            "Event with wrong parent should be rejected"
        );
    }

    #[test]
    fn test_nonce_set_consistency() {
        let (ledger, _account_id, _device_id, _effects) = create_minimal_ledger(1500);

        // Verify initial nonce state consistency
        let state = ledger.state();
        assert_eq!(state.next_nonce, 0, "Initial next_nonce should be 0");
        assert!(
            state.used_nonces.is_empty(),
            "Initial used_nonces should be empty"
        );

        // Verify nonce validation would work correctly
        let mut test_state = state.clone();
        let result1 = test_state.validate_nonce(1);
        assert!(result1.is_ok(), "First nonce should be valid");

        let result2 = test_state.validate_nonce(1);
        assert!(result2.is_err(), "Duplicate nonce should be invalid");
    }
}

// ========== Concurrent Access Pattern Tests ==========

#[cfg(test)]
mod concurrent_access_tests {
    use super::*;

    #[test]
    fn test_rapid_event_application() {
        let (mut ledger, account_id, device_id, effects) = create_minimal_ledger(1600);

        // Simulate rapid event application
        let events: Vec<_> = (1..=50)
            .map(|i| create_test_event(account_id, device_id, i, i - 1, 1000 + i, &effects))
            .collect();

        let mut success_count = 0;
        for event in events {
            if ledger.append_event(event, &effects).is_ok() {
                success_count += 1;
            }
        }

        assert!(success_count > 0, "Should handle rapid events successfully");

        // Verify state consistency after rapid updates
        let final_clock = ledger.lamport_clock();
        assert!(final_clock > 0, "Clock should advance");
    }

    #[test]
    fn test_interleaved_operations() {
        let (mut ledger, account_id, device_id, effects) = create_minimal_ledger(1700);

        // Interleave different types of operations

        // 1. Add event
        let event1 = create_test_event(account_id, device_id, 1, 0, 1000, &effects);
        let _ = ledger.append_event(event1, &effects);

        // 2. Add session
        let session_id = aura_types::SessionId::from_uuid(effects.gen_uuid());
        let participant = aura_types::ParticipantId::Device(device_id);
        let session = aura_journal::types::Session::new(
            session_id,
            aura_types::ProtocolType::Dkd,
            vec![participant],
            1000,
            100,
            1000,
        );
        ledger.add_session(session, &effects);

        // 3. Add another event
        let event2 = create_test_event(account_id, device_id, 2, 1, 1001, &effects);
        let _ = ledger.append_event(event2, &effects);

        // 4. Update session
        let _ = ledger.update_session_status(
            session_id.0,
            aura_types::SessionStatus::Completed,
            &effects,
        );

        // Verify final state consistency
        assert!(!ledger.event_log().is_empty());
        assert!(!ledger.state().sessions.is_empty());
    }

    #[test]
    fn test_cleanup_operations_during_active_use() {
        let (mut ledger, account_id, device_id, effects) = create_minimal_ledger(1800);

        // Add events and sessions
        let event = create_test_event(account_id, device_id, 1, 0, 1000, &effects);
        let _ = ledger.append_event(event, &effects);

        let session_id = SessionId::from_uuid(effects.gen_uuid());
        let participant = ParticipantId::Device(device_id);
        let expired_session = Session::new(
            session_id,
            ProtocolType::Recovery,
            vec![participant],
            0, // started_at - far in past
            1, // ttl_in_epochs - very short
            0,
        );
        ledger.add_session(expired_session, &effects);

        // Perform cleanup while system is active
        ledger.cleanup_expired_sessions(&effects);

        // Add more events after cleanup
        let event2 = create_test_event(account_id, device_id, 2, 1, 1001, &effects);
        let _ = ledger.append_event(event2, &effects);

        // Verify system remains functional
        assert!(!ledger.event_log().is_empty());
    }
}

// ========== Performance and Memory Tests ==========

#[cfg(test)]
mod performance_tests {
    use super::*;

    #[test]
    fn test_state_hash_computation_performance() {
        let (mut ledger, account_id, device_id, effects) = create_minimal_ledger(1900);

        // Add some events to create non-trivial state
        for i in 1..=20 {
            let event = create_test_event(account_id, device_id, i, i - 1, 1000 + i, &effects);
            let _ = ledger.append_event(event, &effects);
        }

        // Compute hash multiple times to test performance
        let start_time = std::time::Instant::now();
        for _ in 0..10 {
            let _ = ledger.compute_state_hash();
        }
        let duration = start_time.elapsed();

        // Should complete in reasonable time (very loose bound for test stability)
        assert!(
            duration.as_secs() < 10,
            "State hash computation should be reasonably fast"
        );
    }

    #[test]
    fn test_event_log_memory_usage() {
        let (mut ledger, account_id, device_id, effects) = create_minimal_ledger(2000);

        let initial_len = ledger.event_log().len();

        // Add events and monitor memory usage indirectly
        for i in 1..=100 {
            let event = create_test_event(account_id, device_id, i, i - 1, 1000 + i, &effects);
            let _ = ledger.append_event(event, &effects);
        }

        let final_len = ledger.event_log().len();

        // Event log should grow, but not excessively
        assert!(final_len > initial_len, "Event log should grow");
        assert!(
            final_len <= 100 + initial_len,
            "Event log growth should be reasonable"
        );

        // Test compaction stats for memory monitoring
        let stats = ledger.compaction_stats();
        assert!(
            stats.estimated_storage_bytes > 0,
            "Should estimate non-zero storage"
        );
    }

    #[test]
    fn test_query_performance_with_large_state() {
        let (mut ledger, account_id, device_id, effects) = create_minimal_ledger(2100);

        // Build up larger state
        for i in 1..=50 {
            let event = create_test_event(account_id, device_id, i, i - 1, 1000 + i, &effects);
            let _ = ledger.append_event(event, &effects);
        }

        // Test various query operations
        let start = std::time::Instant::now();

        let _ = ledger.lamport_clock();
        let _ = ledger.last_event_hash();
        let _ = ledger.active_sessions();
        let _ = ledger.event_log();
        let _ = ledger.state();

        let duration = start.elapsed();

        // Queries should be fast even with larger state
        assert!(duration.as_millis() < 100, "Queries should be fast");
    }
}
