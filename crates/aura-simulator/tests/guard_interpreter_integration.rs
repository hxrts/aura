//! Integration tests for the simulation effect interpreter
//!
//! These tests demonstrate deterministic execution and replay capabilities
//! of the simulation effect interpreter for testing distributed protocols.

use aura_core::{
    effects::{
        guard::{EffectCommand, EffectInterpreter, EffectResult, JournalEntry, SimulationEvent},
        NetworkAddress,
    },
    identifiers::{AuthorityId, ContextId},
    journal::Fact,
    time::{PhysicalTime, TimeStamp},
};
use aura_simulator::effects::{SimulationEffectInterpreter, SimulationState};
use std::sync::Arc;

fn authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

/// Simulate a multi-party protocol with deterministic replay
#[tokio::test]
async fn test_multi_party_protocol_simulation() {
    let time = TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: 1000,
        uncertainty: None,
    });

    // Create three authorities
    let alice = authority(1);
    let bob = authority(2);
    let carol = authority(3);

    // Create interpreters with shared state
    let shared_state = Arc::new(std::sync::Mutex::new(SimulationState::new(
        42,
        time.clone(),
    )));

    let alice_interp = SimulationEffectInterpreter::from_state(
        shared_state.clone(),
        alice,
        NetworkAddress::new("test://alice".to_string()),
    );
    let bob_interp = SimulationEffectInterpreter::from_state(
        shared_state.clone(),
        bob,
        NetworkAddress::new("test://bob".to_string()),
    );
    let carol_interp = SimulationEffectInterpreter::from_state(
        shared_state,
        carol,
        NetworkAddress::new("test://carol".to_string()),
    );

    // Set initial budgets
    alice_interp.set_initial_budget(alice, 1000);
    bob_interp.set_initial_budget(bob, 1000);
    carol_interp.set_initial_budget(carol, 1000);

    // Simulate a threshold signing protocol
    // 1. Alice initiates by storing session metadata
    alice_interp
        .execute(EffectCommand::StoreMetadata {
            key: "session_id".to_string(),
            value: "threshold_sign_123".to_string(),
        })
        .await
        .unwrap();

    alice_interp
        .execute(EffectCommand::RecordLeakage {
            bits: 32, // Session ID leaks some metadata
        })
        .await
        .unwrap();

    // 2. Alice sends signing requests to Bob and Carol
    let signing_request = vec![1, 2, 3, 4, 5]; // Mock signing package
    alice_interp
        .execute(EffectCommand::SendEnvelope {
            to: NetworkAddress::new("test://bob".to_string()),
            peer_id: None,
            envelope: signing_request.clone(),
        })
        .await
        .unwrap();

    alice_interp
        .execute(EffectCommand::SendEnvelope {
            to: NetworkAddress::new("test://carol".to_string()),
            peer_id: None,
            envelope: signing_request,
        })
        .await
        .unwrap();

    // 3. Bob and Carol process requests and charge flow budget
    let context = ContextId::new_from_entropy([0u8; 32]);
    bob_interp
        .execute(EffectCommand::ChargeBudget {
            context,
            authority: bob,
            peer: alice,
            amount: 50,
        })
        .await
        .unwrap();

    carol_interp
        .execute(EffectCommand::ChargeBudget {
            context,
            authority: carol,
            peer: alice,
            amount: 50,
        })
        .await
        .unwrap();

    // 4. Bob and Carol generate signature shares (using nonces)
    let bob_nonce = match bob_interp
        .execute(EffectCommand::GenerateNonce { bytes: 32 })
        .await
        .unwrap()
    {
        EffectResult::Nonce(n) => n,
        _ => panic!("Expected nonce"),
    };

    let carol_nonce = match carol_interp
        .execute(EffectCommand::GenerateNonce { bytes: 32 })
        .await
        .unwrap()
    {
        EffectResult::Nonce(n) => n,
        _ => panic!("Expected nonce"),
    };

    // 5. Record signature generation in journal
    let fact = Fact::default(); // Mock fact
    bob_interp
        .execute(EffectCommand::AppendJournal {
            entry: JournalEntry {
                fact: fact.clone(),
                authority: bob,
                timestamp: time.clone(),
            },
        })
        .await
        .unwrap();

    carol_interp
        .execute(EffectCommand::AppendJournal {
            entry: JournalEntry {
                fact,
                authority: carol,
                timestamp: time.clone(),
            },
        })
        .await
        .unwrap();

    // 6. Send shares back to Alice
    bob_interp
        .execute(EffectCommand::SendEnvelope {
            to: NetworkAddress::new("test://alice".to_string()),
            peer_id: None,
            envelope: bob_nonce.clone(),
        })
        .await
        .unwrap();

    carol_interp
        .execute(EffectCommand::SendEnvelope {
            to: NetworkAddress::new("test://alice".to_string()),
            peer_id: None,
            envelope: carol_nonce.clone(),
        })
        .await
        .unwrap();

    // Verify state
    let state = alice_interp.snapshot_state();
    assert_eq!(
        state.metadata.get("session_id"),
        Some(&"threshold_sign_123".to_string())
    );
    assert_eq!(state.total_leakage_bits, 32);
    assert_eq!(state.message_queue.len(), 4); // 2 requests + 2 responses
    assert_eq!(state.journal.len(), 2); // Bob and Carol's entries

    // Verify deterministic nonces (same seed should produce same nonces in replay)
    let events = alice_interp.events();
    assert!(!events.is_empty());

    // Test replay in new interpreter
    let _replay_state = SimulationState::new(42, time.clone()); // Same seed
    let replay_interp = SimulationEffectInterpreter::new(
        42,
        time,
        alice,
        NetworkAddress::new("test://alice".to_string()),
    );

    // Set same initial conditions
    replay_interp.set_initial_budget(alice, 1000);
    replay_interp.set_initial_budget(bob, 1000);
    replay_interp.set_initial_budget(carol, 1000);

    // Replay should produce identical state
    replay_interp.replay(events).await.unwrap();

    let replay_final = replay_interp.snapshot_state();
    assert_eq!(replay_final.metadata, state.metadata);
    assert_eq!(replay_final.total_leakage_bits, state.total_leakage_bits);
    assert_eq!(replay_final.journal.len(), state.journal.len());
    assert_eq!(replay_final.message_queue.len(), state.message_queue.len());
}

/// Test guard chain evaluation with simulation
#[tokio::test]
async fn test_guard_chain_simulation() {
    let time = TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: 1000,
        uncertainty: None,
    });
    let authority = authority(4);
    let addr = NetworkAddress::new("test://guard_test".to_string());

    let interp = SimulationEffectInterpreter::new(42, time.clone(), authority, addr);
    interp.set_initial_budget(authority, 500);

    // Simulate a guard chain that:
    // 1. Checks authorization (metadata lookup)
    // 2. Charges flow budget
    // 3. Records leakage
    // 4. Appends to journal
    // 5. Sends response

    // Mock authorized user check
    interp
        .execute(EffectCommand::StoreMetadata {
            key: "user:alice:authorized".to_string(),
            value: "true".to_string(),
        })
        .await
        .unwrap();

    // Guard evaluation produces these effects
    let guard = vec![
        EffectCommand::ChargeBudget {
            context: ContextId::new_from_entropy([1u8; 32]),
            authority,
            peer: authority,
            amount: 100,
        },
        EffectCommand::RecordLeakage {
            bits: 16, // User ID leaked
        },
        EffectCommand::AppendJournal {
            entry: JournalEntry {
                fact: Fact::default(),
                authority,
                timestamp: time,
            },
        },
        EffectCommand::SendEnvelope {
            to: NetworkAddress::new("test://client".to_string()),
            peer_id: None,
            envelope: vec![200], // HTTP 200 OK
        },
    ];

    // Execute all effects
    for effect in guard {
        interp.execute(effect).await.unwrap();
    }

    // Verify guard chain execution
    let state = interp.snapshot_state();
    assert_eq!(state.get_budget(&authority), 400); // 500 - 100
    assert_eq!(state.total_leakage_bits, 16);
    assert_eq!(state.journal.len(), 1);
    assert_eq!(state.message_queue.len(), 1);

    // Verify specific events were recorded
    let budget_events =
        interp.events_of_type(|e| matches!(e, SimulationEvent::BudgetCharged { .. }));
    assert_eq!(budget_events.len(), 1);

    let leakage_events =
        interp.events_of_type(|e| matches!(e, SimulationEvent::LeakageRecorded { .. }));
    assert_eq!(leakage_events.len(), 1);
}

/// Test failure scenarios and budget exhaustion
#[tokio::test]
async fn test_budget_exhaustion_simulation() {
    let time = TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: 1000,
        uncertainty: None,
    });
    let authority = authority(5);
    let addr = NetworkAddress::new("test://exhaustion_test".to_string());

    let interp = SimulationEffectInterpreter::new(42, time, authority, addr);
    interp.set_initial_budget(authority, 100);

    // First charge should succeed
    let result = interp
        .execute(EffectCommand::ChargeBudget {
            context: ContextId::new_from_entropy([2u8; 32]),
            authority,
            peer: authority,
            amount: 60,
        })
        .await
        .unwrap();

    match result {
        EffectResult::RemainingBudget(remaining) => {
            assert_eq!(remaining, 40);
        }
        _ => panic!("Expected remaining budget"),
    }

    // Second charge should succeed
    let result = interp
        .execute(EffectCommand::ChargeBudget {
            context: ContextId::new_from_entropy([3u8; 32]),
            authority,
            peer: authority,
            amount: 40,
        })
        .await
        .unwrap();

    match result {
        EffectResult::RemainingBudget(remaining) => {
            assert_eq!(remaining, 0);
        }
        _ => panic!("Expected remaining budget"),
    }

    // Third charge should fail
    let result = interp
        .execute(EffectCommand::ChargeBudget {
            context: ContextId::new_from_entropy([4u8; 32]),
            authority,
            peer: authority,
            amount: 10,
        })
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Insufficient budget"));
}

/// Test deterministic behavior across multiple runs
#[tokio::test]
async fn test_determinism_guarantee() {
    let time = TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: 1000,
        uncertainty: None,
    });
    let authority = authority(6);
    let addr = NetworkAddress::new("test://determinism_test".to_string());

    // Run the same scenario twice with same seed
    let mut run_results = Vec::new();

    for _ in 0..2 {
        let interp = SimulationEffectInterpreter::new(42, time.clone(), authority, addr.clone());

        // Generate multiple nonces
        let mut nonces = Vec::new();
        for _i in 0..5 {
            let result = interp
                .execute(EffectCommand::GenerateNonce { bytes: 16 })
                .await
                .unwrap();

            match result {
                EffectResult::Nonce(n) => nonces.push(n),
                _ => panic!("Expected nonce"),
            }
        }

        run_results.push(nonces);
    }

    // Both runs should produce identical nonces
    assert_eq!(run_results[0], run_results[1]);
}
