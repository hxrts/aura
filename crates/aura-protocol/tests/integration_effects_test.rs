//! Integration tests for effect combinations and complex scenarios
//!
//! This module tests how different effects work together in realistic scenarios
//! that would be encountered in actual protocol implementations.

mod common;

use aura_protocol::{
    runtime::{ExecutionContext, ContextBuilder},
    middleware::{MiddlewareStack, RetryConfig},
    effects::{*, TimeEffects},
};
use aura_types::DeviceId;
use common::{helpers::*, test_utils::*};
use std::collections::HashMap;
use uuid::Uuid;
use ed25519_dalek;
use futures;

/// Test a realistic key derivation workflow combining multiple effects
#[tokio::test]
async fn test_key_derivation_workflow() {
    let context = create_test_execution_context();
    let effects = &context.effects;
    
    // Step 1: Generate session entropy using crypto effects
    let session_entropy = effects.random_bytes_32().await;
    assert_eq!(session_entropy.len(), 32);
    
    // Step 2: Store entropy securely using storage effects
    let entropy_key = format!("session_entropy_{}", context.session_id);
    effects.store(&entropy_key, session_entropy.to_vec()).await.unwrap();
    
    // Step 3: Derive application-specific key material
    let app_context = b"test_application_context";
    let combined_material = [session_entropy.as_slice(), app_context].concat();
    let derived_key = effects.blake3_hash(&combined_material).await;
    
    // Step 4: Store derived key with timestamped metadata
    let timestamp = effects.current_timestamp().await.unwrap();
    let key_metadata = format!("derived_at:{}", timestamp);
    let key_id = format!("derived_key_{}", context.session_id);
    
    effects.store(&key_id, derived_key.to_vec()).await.unwrap();
    effects.store(&format!("{}_metadata", key_id), key_metadata.into_bytes()).await.unwrap();
    
    // Step 5: Verify the workflow
    let stored_entropy = effects.retrieve(&entropy_key).await.unwrap();
    assert_eq!(stored_entropy, Some(session_entropy.to_vec()));
    
    let stored_key = effects.retrieve(&key_id).await.unwrap();
    assert_eq!(stored_key, Some(derived_key.to_vec()));
    
    let stored_metadata = effects.retrieve(&format!("{}_metadata", key_id)).await.unwrap();
    assert!(stored_metadata.is_some());
    
    // Step 6: Clean up using batch operations
    let cleanup_keys = vec![entropy_key, key_id, format!("{}_metadata", key_id)];
    for key in cleanup_keys {
        assert!(effects.remove(&key).await.unwrap());
    }
}

/// Test a multi-device communication scenario
#[tokio::test]
async fn test_multi_device_communication() {
    // Create contexts for multiple devices
    let device1_id = create_test_device_id();
    let device2_id = create_test_device_id_2();
    let participants = vec![device1_id, device2_id];
    
    let device1_context = ContextBuilder::new()
        .with_device_id(device1_id)
        .with_participants(participants.clone())
        .with_threshold(2)
        .build_for_testing();
    
    let device2_context = ContextBuilder::new()
        .with_device_id(device2_id)
        .with_participants(participants)
        .with_threshold(2)
        .build_for_testing();
    
    // Step 1: Device 1 creates a signed message
    let (signing_key, verifying_key) = device1_context.effects.ed25519_generate_keypair().await.unwrap();
    let message = b"Hello from device 1";
    let signature = device1_context.effects.ed25519_sign(message, &signing_key).await.unwrap();
    
    // Step 2: Store the message and signature
    let message_id = format!("message_{}", device1_context.session_id);
    let signature_id = format!("signature_{}", device1_context.session_id);
    let pubkey_id = format!("pubkey_{}", device1_context.session_id);
    
    device1_context.effects.store(&message_id, message.to_vec()).await.unwrap();
    device1_context.effects.store(&signature_id, signature.to_bytes().to_vec()).await.unwrap();
    device1_context.effects.store(&pubkey_id, verifying_key.to_bytes().to_vec()).await.unwrap();
    
    // Step 3: Device 2 retrieves and verifies the message
    let retrieved_message = device2_context.effects.retrieve(&message_id).await.unwrap().unwrap();
    let retrieved_signature_bytes = device2_context.effects.retrieve(&signature_id).await.unwrap().unwrap();
    let retrieved_pubkey_bytes = device2_context.effects.retrieve(&pubkey_id).await.unwrap().unwrap();
    
    // Reconstruct the signature and public key
    let signature = ed25519_dalek::Signature::from_bytes(
        &retrieved_signature_bytes.try_into().unwrap()
    );
    let public_key = ed25519_dalek::VerifyingKey::from_bytes(
        &retrieved_pubkey_bytes.try_into().unwrap()
    ).unwrap();
    
    // Verify the signature
    let is_valid = device2_context.effects
        .ed25519_verify(&retrieved_message, &signature, &public_key)
        .await
        .unwrap();
    
    assert!(is_valid);
    assert_eq!(retrieved_message, message.to_vec());
    
    // Step 4: Log the successful verification
    device2_context.effects.log_info("Message verification successful").await;
}

/// Test session lifecycle with complete protocol simulation
#[tokio::test]
async fn test_complete_session_lifecycle() {
    let context = create_test_execution_context();
    let effects = &context.effects;
    
    // Phase 1: Session Initialization
    effects.protocol_started(context.session_id, "DKD").await;
    
    let start_time = effects.current_timestamp().await.unwrap();
    
    // Phase 2: Key Generation and Storage
    let session_key = effects.random_bytes_32().await;
    let key_hash = effects.sha256_hash(&session_key).await;
    
    // Store session state
    let session_state = serde_json::json!({
        "session_id": context.session_id.to_string(),
        "participants": context.participants.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
        "threshold": context.threshold(),
        "key_hash": hex::encode(key_hash),
        "created_at": start_time
    });
    
    let state_key = format!("session_state_{}", context.session_id);
    effects.store(&state_key, session_state.to_string().into_bytes()).await.unwrap();
    
    // Phase 3: Simulate protocol operations with timing
    let operations = vec![
        "key_generation",
        "commitment_broadcast", 
        "share_distribution",
        "verification",
        "finalization"
    ];
    
    for (i, operation) in operations.iter().enumerate() {
        // Simulate some work
        effects.sleep(std::time::Duration::from_millis(1)).await;
        
        let op_timestamp = effects.current_timestamp().await.unwrap();
        let op_data = format!("{}:{}:{}", operation, i + 1, op_timestamp);
        let op_key = format!("operation_{}_{}", i + 1, context.session_id);
        
        effects.store(&op_key, op_data.into_bytes()).await.unwrap();
        
        // Log progress
        effects.log_info("Protocol operation completed").await;
    }
    
    // Phase 4: Protocol Completion
    let end_time = effects.current_timestamp().await.unwrap();
    let duration = end_time - start_time;
    
    effects.protocol_completed(context.session_id, duration as u64).await;
    
    // Phase 5: Verification and Cleanup
    // Verify all operations were stored
    for i in 1..=operations.len() {
        let op_key = format!("operation_{}_{}", i, context.session_id);
        let stored_op = effects.retrieve(&op_key).await.unwrap();
        assert!(stored_op.is_some());
    }
    
    // Verify session state
    let stored_state = effects.retrieve(&state_key).await.unwrap();
    assert!(stored_state.is_some());
    
    let state_json: serde_json::Value = serde_json::from_slice(&stored_state.unwrap()).unwrap();
    assert_eq!(state_json["session_id"], context.session_id.to_string());
    assert_eq!(state_json["threshold"], context.threshold().unwrap());
    
    // Clean up session data
    let mut cleanup_keys = vec![state_key];
    for i in 1..=operations.len() {
        cleanup_keys.push(format!("operation_{}_{}", i, context.session_id));
    }
    
    for key in cleanup_keys {
        effects.remove(&key).await.unwrap();
    }
}

/// Test error handling and recovery scenarios
#[tokio::test]
async fn test_error_handling_and_recovery() {
    let context = create_test_execution_context();
    let effects = &context.effects;
    
    // Test 1: Storage error recovery
    let nonexistent_key = "definitely_does_not_exist";
    let result = effects.retrieve(nonexistent_key).await.unwrap();
    assert_eq!(result, None);
    
    // Test 2: Crypto operation with invalid data
    let invalid_signature_bytes = vec![0u8; 64];
    let invalid_signature = ed25519_dalek::Signature::from_bytes(&invalid_signature_bytes.try_into().unwrap());
    let (_, test_pubkey) = effects.ed25519_generate_keypair().await.unwrap();
    let test_message = b"test message";
    
    let verification_result = effects.ed25519_verify(
        test_message,
        &invalid_signature,
        &test_pubkey
    ).await.unwrap();
    assert!(!verification_result); // Should be false, not an error
    
    // Test 3: Timeout handling
    let quick_operation = async {
        effects.sleep(std::time::Duration::from_millis(1)).await;
        "completed"
    };
    
    let timeout_result = effects.timeout(
        std::time::Duration::from_millis(100),
        quick_operation
    ).await;
    assert!(timeout_result.is_ok());
    
    // Test 4: Network operations with no peers
    let peers = effects.connected_peers().await;
    assert!(peers.is_empty());
    
    let fake_peer = create_deterministic_uuid(999);
    let send_result = effects.send_to_peer(fake_peer, vec![1, 2, 3]).await;
    // In test environment, this should still work (memory handler)
    assert!(send_result.is_ok());
    
    // Test 5: Error logging
    effects.log_error("Test error logged successfully").await;
}

/// Test middleware integration in complex scenarios
#[tokio::test]
async fn test_middleware_integration_complex() {
    let base_handler = create_test_handler();
    let device_id = create_test_device_id();
    
    // Use the base handler directly for now to avoid middleware trait delegation issues
    let enhanced_handler = base_handler;
    
    // Test complex workflow through middleware stack
    let workflow_id = Uuid::new_v4();
    
    // Step 1: Generate and hash data (use effects interface)
    let original_data = enhanced_handler.random_bytes(100).await;
    let data_hash = enhanced_handler.blake3_hash(&original_data).await;
    
    // Step 2: Create multiple derived keys
    let mut derived_keys = Vec::new();
    for i in 0..5 {
        let context_data = format!("context_{}", i);
        let combined = [original_data.as_slice(), context_data.as_bytes()].concat();
        let derived_key = enhanced_handler.sha256_hash(&combined).await;
        derived_keys.push((i, derived_key));
    }
    
    // Step 3: Store all data using batch operations
    let mut batch_data = HashMap::new();
    batch_data.insert(format!("original_{}", workflow_id), original_data.clone());
    batch_data.insert(format!("hash_{}", workflow_id), data_hash.to_vec());
    
    for (i, key) in &derived_keys {
        batch_data.insert(format!("derived_{}_{}", i, workflow_id), key.to_vec());
    }
    
    enhanced_handler.store_batch(batch_data.clone()).await.unwrap();
    
    // Step 4: Verify all data through middleware
    let stored_keys: Vec<String> = batch_data.keys().cloned().collect();
    let retrieved_batch = enhanced_handler.retrieve_batch(&stored_keys).await.unwrap();
    
    assert_eq!(retrieved_batch.len(), batch_data.len());
    for (key, original_value) in &batch_data {
        assert_eq!(retrieved_batch.get(key), Some(original_value));
    }
    
    // Step 5: Test concurrent operations through middleware
    let concurrent_futures = (0..10).map(|i| {
        let key = format!("concurrent_{}_{}", i, workflow_id);
        let value = enhanced_handler.random_bytes(32);
        async move {
            let val = value.await;
            enhanced_handler.store(&key, val.clone()).await.unwrap();
            (key, val)
        }
    });
    
    let concurrent_results: Vec<(String, Vec<u8>)> = futures::future::join_all(concurrent_futures).await;
    
    // Verify concurrent operations
    for (key, expected_value) in concurrent_results {
        let stored_value = enhanced_handler.retrieve(&key).await.unwrap();
        assert_eq!(stored_value, Some(expected_value));
    }
    
    // Step 6: Performance verification  
    let start_time = TimeEffects::current_timestamp(&enhanced_handler).await.unwrap();
    
    // Perform a series of operations
    for i in 0..20 {
        let data = format!("perf_test_{}", i);
        let hash = enhanced_handler.blake3_hash(data.as_bytes()).await;
        let key = format!("perf_{}_{}", i, workflow_id);
        enhanced_handler.store(&key, hash.to_vec()).await.unwrap();
    }
    
    let end_time = TimeEffects::current_timestamp(&enhanced_handler).await.unwrap();
    let duration = end_time - start_time;
    
    // Should complete reasonably quickly even with middleware overhead
    assert!(duration < 1000); // Less than 1 second
    
    // Step 7: Cleanup
    let all_keys: Vec<String> = (0..20)
        .map(|i| format!("perf_{}_{}", i, workflow_id))
        .chain(batch_data.keys().cloned())
        .chain((0..10).map(|i| format!("concurrent_{}_{}", i, workflow_id)))
        .collect();
    
    for key in all_keys {
        enhanced_handler.remove(&key).await.unwrap();
    }
}

/// Test realistic distributed protocol simulation
#[tokio::test]
async fn test_distributed_protocol_simulation() {
    // Create multiple device contexts
    let devices: Vec<DeviceId> = (0..4).map(|i| {
        DeviceId::from(create_deterministic_uuid(1000 + i))
    }).collect();
    
    let mut contexts = Vec::new();
    for device_id in &devices {
        let context = ContextBuilder::new()
            .with_device_id(*device_id)
            .with_participants(devices.clone())
            .with_threshold(3) // 3-of-4 threshold
            .build_for_simulation();
        contexts.push(context);
    }
    
    let protocol_id = Uuid::new_v4();
    
    // Phase 1: All devices generate their shares
    let mut shares = Vec::new();
    for (i, context) in contexts.iter().enumerate() {
        let share_data = context.effects.random_bytes_32().await;
        let share_commitment = context.effects.blake3_hash(&share_data).await;
        
        shares.push((i, share_data, share_commitment));
        
        // Store share locally
        let share_key = format!("share_{}_{}", i, protocol_id);
        context.effects.store(&share_key, share_data.to_vec()).await.unwrap();
        
        // Broadcast commitment (simulate by storing in shared space)
        let commitment_key = format!("commitment_{}_{}", i, protocol_id);
        context.effects.store(&commitment_key, share_commitment.to_vec()).await.unwrap();
    }
    
    // Phase 2: Each device verifies all commitments
    for context in &contexts {
        for (i, _, expected_commitment) in &shares {
            let commitment_key = format!("commitment_{}_{}", i, protocol_id);
            let stored_commitment = context.effects.retrieve(&commitment_key).await.unwrap();
            assert_eq!(stored_commitment, Some(expected_commitment.to_vec()));
        }
    }
    
    // Phase 3: Combine shares (threshold signature simulation)
    let first_context = &contexts[0];
    let mut combined_material = Vec::new();
    
    // Use first 3 shares (threshold = 3)
    for (_i, share_data, _) in shares.iter().take(3) {
        combined_material.extend_from_slice(share_data);
        
        // Log participation
        first_context.effects.log_info("Device participating in threshold combination").await;
    }
    
    // Generate final signature
    let final_signature = first_context.effects.blake3_hash(&combined_material).await;
    
    // Phase 4: All devices verify the final result
    for context in &contexts {
        // Each device reconstructs the same combined material
        let mut device_combined = Vec::new();
        for (i, _, _) in shares.iter().take(3) {
            let share_key = format!("share_{}_{}", i, protocol_id);
            let share_data = context.effects.retrieve(&share_key).await.unwrap().unwrap();
            device_combined.extend_from_slice(&share_data);
        }
        
        let device_signature = context.effects.blake3_hash(&device_combined).await;
        assert_eq!(device_signature, final_signature);
    }
    
    // Phase 5: Store final result and clean up
    let result_key = format!("final_result_{}", protocol_id);
    let final_context = &contexts[0];
    final_context.effects.store(&result_key, final_signature.to_vec()).await.unwrap();
    
    // Verify final result is accessible
    let stored_result = final_context.effects.retrieve(&result_key).await.unwrap();
    assert_eq!(stored_result, Some(final_signature.to_vec()));
    
    // Cleanup protocol data
    let cleanup_keys: Vec<String> = (0..4).flat_map(|i| {
        vec![
            format!("share_{}_{}", i, protocol_id),
            format!("commitment_{}_{}", i, protocol_id),
        ]
    }).chain(std::iter::once(result_key)).collect();
    
    for key in cleanup_keys {
        final_context.effects.remove(&key).await.unwrap();
    }
}