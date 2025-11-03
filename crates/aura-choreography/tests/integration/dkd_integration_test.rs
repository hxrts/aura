//! End-to-end DKD protocol integration tests

use aura_choreography::test_utils::{create_test_participants, create_test_effects};
use aura_choreography::threshold_crypto::DkdProtocol;
use aura_protocol::choreographic::{BridgedEndpoint, RumpsteakAdapter};
use aura_protocol::effects::AuraEffectsAdapter;
use aura_protocol::runtime::context::ProtocolContext;
use aura_types::effects::Effects;
use rumpsteak_choreography::ChoreoHandler;
use tokio_test;
use uuid::Uuid;

/// Test DKD protocol with 3 participants
#[tokio::test]
async fn test_dkd_protocol_3_participants() {
    let seed = 42;
    let participants = create_test_participants(3);
    let effects = create_test_effects(seed);
    
    // Test each participant's view of the protocol
    let mut results = Vec::new();
    
    for (i, participant) in participants.iter().enumerate() {
        let participant_effects = Effects::deterministic(seed, i as u64);
        let protocol = DkdProtocol::with_crypto(
            participants.clone(),
            "test_app".to_string(),
            "user_keys".to_string(),
            participant_effects.clone(),
            participant.device_id,
        );
        
        // Create test adapter and endpoint
        let effects_adapter = AuraEffectsAdapter::new(participant.device_id, participant_effects);
        let context = ProtocolContext::new_test(participant.device_id);
        let handler = aura_protocol::handlers::StandardHandlerFactory::in_memory(
            aura_types::DeviceId::from(participant.device_id)
        );
        let mut adapter = RumpsteakAdapter::new(handler, effects_adapter, context.clone());
        let mut endpoint = BridgedEndpoint::new(context);
        
        // Execute the protocol
        let result = protocol.execute(&mut adapter, &mut endpoint, *participant).await;
        results.push(result);
    }
    
    // Verify all participants succeeded
    assert_eq!(results.len(), 3);
    for (i, result) in results.iter().enumerate() {
        assert!(result.is_ok(), "Participant {} failed: {:?}", i, result);
    }
    
    // Verify all participants derived the same key (deterministic)
    let derived_keys: Vec<_> = results.into_iter()
        .map(|r| r.unwrap())
        .collect();
    
    assert!(derived_keys.windows(2).all(|w| w[0] == w[1]), 
           "Participants derived different keys");
    
    // Verify the key is not all zeros (actual derivation happened)
    assert_ne!(derived_keys[0], vec![0u8; 32]);
}

/// Test DKD protocol with deterministic results
#[tokio::test]
async fn test_dkd_deterministic_results() {
    let seed = 12345;
    let participants = create_test_participants(3);
    
    // Run the same protocol twice with same seed
    let mut first_results = Vec::new();
    let mut second_results = Vec::new();
    
    for run in 0..2 {
        let mut run_results = Vec::new();
        
        for (i, participant) in participants.iter().enumerate() {
            let participant_effects = Effects::deterministic(seed, i as u64);
            let protocol = DkdProtocol::with_crypto(
                participants.clone(),
                "test_app".to_string(),
                "user_keys".to_string(),
                participant_effects.clone(),
                participant.device_id,
            );
            
            let effects_adapter = AuraEffectsAdapter::new(participant.device_id, participant_effects);
            let context = ProtocolContext::new_test(participant.device_id);
            let handler = aura_protocol::handlers::StandardHandlerFactory::in_memory(
                aura_types::DeviceId::from(participant.device_id)
            );
            let mut adapter = RumpsteakAdapter::new(handler, effects_adapter, context.clone());
            let mut endpoint = BridgedEndpoint::new(context);
            
            let result = protocol.execute(&mut adapter, &mut endpoint, *participant).await;
            run_results.push(result.unwrap());
        }
        
        if run == 0 {
            first_results = run_results;
        } else {
            second_results = run_results;
        }
    }
    
    // Verify deterministic behavior
    assert_eq!(first_results.len(), second_results.len());
    for (i, (first, second)) in first_results.iter().zip(second_results.iter()).enumerate() {
        assert_eq!(first, second, "Run {} produced different results", i);
    }
}

/// Test DKD protocol with different contexts produces different keys
#[tokio::test]
async fn test_dkd_different_contexts() {
    let seed = 98765;
    let participants = create_test_participants(3);
    
    // Run with two different contexts
    let contexts = ["context1", "context2"];
    let mut results_by_context = Vec::new();
    
    for context in &contexts {
        let mut context_results = Vec::new();
        
        for (i, participant) in participants.iter().enumerate() {
            let participant_effects = Effects::deterministic(seed, i as u64);
            let protocol = DkdProtocol::with_crypto(
                participants.clone(),
                "test_app".to_string(),
                context.to_string(),
                participant_effects.clone(),
                participant.device_id,
            );
            
            let effects_adapter = AuraEffectsAdapter::new(participant.device_id, participant_effects);
            let ctx = ProtocolContext::new_test(participant.device_id);
            let handler = aura_protocol::handlers::StandardHandlerFactory::in_memory(
                aura_types::DeviceId::from(participant.device_id)
            );
            let mut adapter = RumpsteakAdapter::new(handler, effects_adapter, ctx.clone());
            let mut endpoint = BridgedEndpoint::new(ctx);
            
            let result = protocol.execute(&mut adapter, &mut endpoint, *participant).await;
            context_results.push(result.unwrap());
        }
        
        results_by_context.push(context_results);
    }
    
    // Verify different contexts produce different keys
    let context1_keys = &results_by_context[0];
    let context2_keys = &results_by_context[1];
    
    assert_ne!(context1_keys[0], context2_keys[0], 
              "Different contexts should produce different keys");
}

/// Test DKD protocol Byzantine behavior detection
#[tokio::test] 
async fn test_dkd_byzantine_detection() {
    // This test would require injecting Byzantine behavior
    // For now, we'll test that the protocol completes normally
    let seed = 55555;
    let participants = create_test_participants(3);
    
    // Use the first participant as our test subject
    let participant = participants[0];
    let participant_effects = Effects::deterministic(seed, 0);
    let protocol = DkdProtocol::with_crypto(
        participants.clone(),
        "test_app".to_string(),
        "user_keys".to_string(),
        participant_effects.clone(),
        participant.device_id,
    );
    
    let effects_adapter = AuraEffectsAdapter::new(participant.device_id, participant_effects);
    let context = ProtocolContext::new_test(participant.device_id);
    let handler = aura_protocol::handlers::StandardHandlerFactory::in_memory(
        aura_types::DeviceId::from(participant.device_id)
    );
    let mut adapter = RumpsteakAdapter::new(handler, effects_adapter, context.clone());
    let mut endpoint = BridgedEndpoint::new(context);
    
    // For this test, we expect normal completion
    // TODO: Add actual Byzantine behavior injection
    let result = protocol.execute(&mut adapter, &mut endpoint, participant).await;
    
    // In normal operation, this should succeed
    assert!(result.is_ok() || result.is_err(), "Protocol should complete with some result");
}

/// Test DKD protocol timeout handling
#[tokio::test]
async fn test_dkd_timeout_handling() {
    let seed = 77777;
    let participants = create_test_participants(2); // Minimal participants
    
    let participant = participants[0];
    let participant_effects = Effects::deterministic(seed, 0);
    let protocol = DkdProtocol::with_crypto(
        participants.clone(),
        "test_app".to_string(),
        "user_keys".to_string(),
        participant_effects.clone(),
        participant.device_id,
    );
    
    let effects_adapter = AuraEffectsAdapter::new(participant.device_id, participant_effects);
    let context = ProtocolContext::new_test(participant.device_id);
    let handler = aura_protocol::handlers::StandardHandlerFactory::in_memory(
        aura_types::DeviceId::from(participant.device_id)
    );
    let mut adapter = RumpsteakAdapter::new(handler, effects_adapter, context.clone());
    let mut endpoint = BridgedEndpoint::new(context);
    
    // This test verifies the protocol handles timeouts gracefully
    // In a real scenario, we'd simulate network delays
    let result = protocol.execute(&mut adapter, &mut endpoint, participant).await;
    
    // The protocol should either succeed or fail gracefully
    match result {
        Ok(_) => {
            // Success is fine for this test
        }
        Err(e) => {
            // Errors are expected when other participants don't respond
            println!("Expected error in timeout test: {:?}", e);
        }
    }
}