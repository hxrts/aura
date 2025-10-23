//! Debug test for signature validation issue

use crate::Simulation;
use aura_coordination::{Instruction, InstructionResult};
use aura_journal::{Event, EventType, EventAuthorization};

#[tokio::test]
async fn debug_signature_validation() {
    println!("=== Testing signature validation ===");
    
    let mut sim = Simulation::new(42);
    
    // Create a shared account with one device
    let (_account_id, device_info) = sim
        .add_account_with_devices(&["alice"])
        .await;
    
    let alice = device_info[0].0;
    let alice_device_id = device_info[0].1;
    
    let alice_participant = sim.get_participant(alice).unwrap();
    
    // Check device key in ledger
    let (account_id, next_nonce, last_event_hash, _device_info) = {
        let ledger = alice_participant.ledger().await;
        let state = ledger.state();
        println!("Account ID: {:?}", state.account_id);
        println!("Device count: {}", state.devices.len());
        
        let device_key = if let Some(device) = state.devices.get(&alice_device_id) {
            println!("Device ID: {:?}", device.device_id);
            println!("Device public key: {:?}", device.public_key);
            Some(device.public_key)
        } else {
            None
        };
        
        (state.account_id, state.next_nonce, state.last_event_hash, device_key)
    };
    
    // Create a protocol context
    let session_id = sim.generate_uuid();
    let mut ctx = alice_participant.create_protocol_context(
        session_id,
        vec![alice_device_id],
        Some(1),
    );
    
    println!("\nContext device ID: {:?}", ctx.device_id);
    
    // Try to create and sign an event
    let test_event = Event {
        version: 1,
        event_id: aura_journal::EventId::new(),
        account_id,
        timestamp: 1000,
        nonce: next_nonce,
        parent_hash: last_event_hash,
        epoch_at_write: 0,
        event_type: EventType::InitiateDkdSession(aura_journal::InitiateDkdSessionEvent {
            session_id,
            context_id: vec![],
            threshold: 1,
            participants: vec![alice_device_id],
            start_epoch: 0,
            ttl_in_epochs: 100,
        }),
        authorization: EventAuthorization::DeviceCertificate {
            device_id: alice_device_id,
            signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
        },
    };
    
    // Sign the event
    match ctx.sign_event(&test_event) {
        Ok(signature) => {
            println!("\nSuccessfully signed event");
            println!("Signature: {:?}", signature);
            
            // Create properly signed event
            let mut signed_event = test_event.clone();
            signed_event.authorization = EventAuthorization::DeviceCertificate {
                device_id: alice_device_id,
                signature,
            };
            
            // Try to write it
            match ctx.execute(Instruction::WriteToLedger(signed_event)).await {
                Ok(InstructionResult::EventWritten) => {
                    println!("✅ Successfully wrote event to ledger!");
                },
                Ok(other) => {
                    println!("❌ Unexpected result: {:?}", other);
                },
                Err(e) => {
                    println!("❌ Failed to write event: {:?}", e);
                }
            }
        },
        Err(e) => {
            println!("❌ Failed to sign event: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_device_key_setup() {
    println!("=== Testing device key setup ===");
    
    let mut sim = Simulation::new(42);
    
    // Create account with devices
    let (_account_id, device_info) = sim
        .add_account_with_devices(&["alice", "bob"])
        .await;
    
    let alice_id = device_info[0].0;
    let bob_id = device_info[1].0;
    
    let alice = sim.get_participant(alice_id).unwrap();
    let bob = sim.get_participant(bob_id).unwrap();
    
    // Check if both see the same ledger state
    let alice_ledger = alice.ledger_snapshot().await;
    let bob_ledger = bob.ledger_snapshot().await;
    
    println!("Alice sees {} devices", alice_ledger.state().devices.len());
    println!("Bob sees {} devices", bob_ledger.state().devices.len());
    
    // They should see the same account ID
    assert_eq!(alice_ledger.state().account_id, bob_ledger.state().account_id);
    println!("✅ Both participants see the same account");
}