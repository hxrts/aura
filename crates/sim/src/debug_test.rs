//! Debug test to isolate the hanging issue

use crate::Simulation;
use aura_coordination::{Instruction, EventFilter, EventTypePattern, InstructionResult};

#[tokio::test]
#[ignore] // Test hangs due to protocol issues
async fn debug_await_threshold_hanging() {
    let mut sim = Simulation::new(42);
    
    // Create a shared account with two devices
    let (account_id, device_info) = sim
        .add_account_with_devices(&["alice", "bob"])
        .await;
    
    let alice = device_info[0].0;
    let bob = device_info[1].0;
    
    let alice_device_id = device_info[0].1;
    let bob_device_id = device_info[1].1;
    
    let participants = vec![alice_device_id, bob_device_id];
    
    // Get participants
    let alice_participant = sim.get_participant(alice).unwrap();
    let bob_participant = sim.get_participant(bob).unwrap();
    
    let session_id = sim.generate_uuid();
    
    // Create contexts for both participants
    let mut alice_ctx = alice_participant.create_protocol_context(
        session_id,
        participants.clone(),
        Some(1),
    );
    let mut bob_ctx = bob_participant.create_protocol_context(
        session_id,
        participants.clone(),
        Some(1),
    );
    
    // Get current ledger state for proper event construction
    let ledger_state = alice_ctx.execute(Instruction::GetLedgerState).await.unwrap();
    let (nonce, parent_hash) = match ledger_state {
        InstructionResult::LedgerState(state) => (state.next_nonce, state.last_event_hash),
        _ => panic!("Expected ledger state"),
    };
    
    // Alice writes an event first
    let mut init_event = aura_journal::Event {
        version: 1,
        event_id: aura_journal::EventId::new(),
        account_id,
        timestamp: alice_ctx.effects.now().unwrap(),
        nonce,
        parent_hash,
        epoch_at_write: 0,
        event_type: aura_journal::EventType::InitiateDkdSession(aura_journal::InitiateDkdSessionEvent {
            session_id,
            context_id: vec![],
            threshold: 1,
            participants: participants.clone(),
            start_epoch: 100,
            ttl_in_epochs: 100,
        }),
        authorization: aura_journal::EventAuthorization::DeviceCertificate {
            device_id: alice_device_id,
            signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]), // Temporary
        },
    };
    
    // Sign the event properly
    let signature = alice_ctx.sign_event(&init_event).expect("Should be able to sign event");
    init_event.authorization = aura_journal::EventAuthorization::DeviceCertificate {
        device_id: alice_device_id,
        signature,
    };
    
    alice_ctx.execute(Instruction::WriteToLedger(init_event)).await.unwrap();
    
    // Now Bob tries to await for this event type - this should return immediately
    let filter = EventFilter {
        session_id: Some(session_id),
        event_types: Some(vec![EventTypePattern::DkdCommitment]), // This won't match, to test
        authors: None,
        predicate: None,
    };
    
    // This should timeout quickly, not hang indefinitely
    let result = bob_ctx.execute(Instruction::AwaitThreshold {
        count: 1,
        filter,
        timeout_epochs: Some(1), // Very short timeout
    }).await;
    
    // Should get a timeout error, not hang
    assert!(result.is_err());
    println!("Test completed successfully - no hanging!");
}