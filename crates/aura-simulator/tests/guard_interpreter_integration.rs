//! Integration tests for the simulation effect interpreter
//!
//! These tests demonstrate deterministic execution and replay capabilities
//! of the simulation effect interpreter for testing distributed protocols.

use aura_core::{
    effects::{
        guard_effects::{
            EffectCommand, EffectInterpreter, EffectResult, GuardOutcome, GuardSnapshot,
            JournalEntry, SimulationEvent,
        },
        NetworkAddress,
    },
    identifiers::{AuthorityId, ContextId},
    journal::Fact,
    time::TimeStamp,
};
use aura_simulator::effects::{SimulationEffectInterpreter, SimulationState};
use std::collections::HashMap;
use std::sync::Arc;

/// Simulate a multi-party protocol with deterministic replay
#[tokio::test]
async fn test_multi_party_protocol_simulation() {
    let time = TimeStamp::now_physical();

    // Create three authorities
    let alice = AuthorityId::new();
    let bob = AuthorityId::new();
    let carol = AuthorityId::new();

    // Create interpreters with shared state
    let shared_state = Arc::new(std::sync::Mutex::new(SimulationState::new(42, time)));

    let alice_interp = SimulationEffectInterpreter::from_state(
        shared_state.clone(),
        alice,
        NetworkAddress::from_parts("test", "alice"),
    );
    let bob_interp = SimulationEffectInterpreter::from_state(
        shared_state.clone(),
        bob,
        NetworkAddress::from_parts("test", "bob"),
    );
    let carol_interp = SimulationEffectInterpreter::from_state(
        shared_state,
        carol,
        NetworkAddress::from_parts("test", "carol"),
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
            to: NetworkAddress::from_parts("test", "bob"),
            envelope: signing_request.clone(),
        })
        .await
        .unwrap();

    alice_interp
        .execute(EffectCommand::SendEnvelope {
            to: NetworkAddress::from_parts("test", "carol"),
            envelope: signing_request,
        })
        .await
        .unwrap();

    // 3. Bob and Carol process requests and charge flow budget
    bob_interp
        .execute(EffectCommand::ChargeBudget {
            authority: bob,
            amount: 50,
        })
        .await
        .unwrap();

    carol_interp
        .execute(EffectCommand::ChargeBudget {
            authority: carol,
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
                timestamp: time,
            },
        })
        .await
        .unwrap();

    carol_interp
        .execute(EffectCommand::AppendJournal {
            entry: JournalEntry {
                fact,
                authority: carol,
                timestamp: time,
            },
        })
        .await
        .unwrap();

    // 6. Send shares back to Alice
    bob_interp
        .execute(EffectCommand::SendEnvelope {
            to: NetworkAddress::from_parts("test", "alice"),
            envelope: bob_nonce.clone(),
        })
        .await
        .unwrap();

    carol_interp
        .execute(EffectCommand::SendEnvelope {
            to: NetworkAddress::from_parts("test", "alice"),
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
    assert!(events.len() > 0);

    // Test replay in new interpreter
    let replay_state = SimulationState::new(42, time); // Same seed
    let replay_interp = SimulationEffectInterpreter::new(
        42,
        time,
        alice,
        NetworkAddress::from_parts("test", "alice"),
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
    let time = TimeStamp::now_physical();
    let authority = AuthorityId::new();
    let addr = NetworkAddress::from_parts("test", "guard_test");

    let interp = SimulationEffectInterpreter::new(42, time, authority, addr);
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
    let guard_effects = vec![
        EffectCommand::ChargeBudget {
            authority,
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
            to: NetworkAddress::from_parts("test", "client"),
            envelope: vec![200], // HTTP 200 OK
        },
    ];

    // Execute all effects
    for effect in guard_effects {
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
    let time = TimeStamp::now_physical();
    let authority = AuthorityId::new();
    let addr = NetworkAddress::from_parts("test", "exhaustion_test");

    let interp = SimulationEffectInterpreter::new(42, time, authority, addr);
    interp.set_initial_budget(authority, 100);

    // First charge should succeed
    let result = interp
        .execute(EffectCommand::ChargeBudget {
            authority,
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
            authority,
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
            authority,
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
    let time = TimeStamp::now_physical();
    let authority = AuthorityId::new();
    let addr = NetworkAddress::from_parts("test", "determinism_test");

    // Run the same scenario twice with same seed
    let mut run_results = Vec::new();

    for _ in 0..2 {
        let interp = SimulationEffectInterpreter::new(42, time, authority, addr);

        // Generate multiple nonces
        let mut nonces = Vec::new();
        for i in 0..5 {
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
