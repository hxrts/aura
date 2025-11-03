//! Fixed middleware integration tests

use aura_choreography::integration::effects_integration::{ChoreographicEffectsAdapter, ChoreographicEffects};
use aura_types::effects::Effects;
use rumpsteak_choreography::ChoreographyError;
use uuid::Uuid;

/// Test choreographic effects adapter basic functionality
#[tokio::test]
async fn test_choreographic_effects_adapter() {
    let device_id = Uuid::new_v4();
    let effects = Effects::deterministic(42, 0);
    let adapter = ChoreographicEffectsAdapter::new(
        device_id,
        effects,
        "TestProtocol".to_string(),
    );
    
    // Test recording choreographic events
    let result = adapter.record_choreographic_event(
        "test_event",
        device_id,
        "test_phase",
    ).await;
    assert!(result.is_ok(), "Recording choreographic event should succeed");
    
    // Test phase transitions
    let result = adapter.record_phase_transition(
        "phase1",
        "phase2",
        device_id,
    ).await;
    assert!(result.is_ok(), "Recording phase transition should succeed");
    
    // Test message recording
    let other_device = Uuid::new_v4();
    let result = adapter.record_message_send(
        device_id,
        other_device,
        "TestMessage",
        100,
    ).await;
    assert!(result.is_ok(), "Recording message send should succeed");
}

/// Test choreographic effects adapter timeout handling
#[tokio::test]
async fn test_choreographic_effects_timeout() {
    let device_id = Uuid::new_v4();
    let effects = Effects::deterministic(12345, 0);
    let adapter = ChoreographicEffectsAdapter::new(
        device_id,
        effects,
        "TimeoutTest".to_string(),
    );
    
    let start_time = tokio::time::Instant::now();
    let short_timeout = std::time::Duration::from_millis(1);
    
    // Wait a bit to ensure timeout
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    
    // This should timeout
    let result = adapter.check_timeout(start_time, short_timeout);
    assert!(result.is_err(), "Should timeout with short duration");
    
    // Test with longer timeout
    let long_timeout = std::time::Duration::from_secs(60);
    let result = adapter.check_timeout(start_time, long_timeout);
    assert!(result.is_ok(), "Should not timeout with long duration");
}

/// Test choreographic effects adapter Byzantine behavior recording
#[tokio::test]
async fn test_choreographic_effects_byzantine_recording() {
    let device_id = Uuid::new_v4();
    let effects = Effects::deterministic(98765, 0);
    let adapter = ChoreographicEffectsAdapter::new(
        device_id,
        effects,
        "ByzantineTest".to_string(),
    );
    
    let byzantine_device = Uuid::new_v4();
    let result = adapter.record_byzantine_behavior(
        byzantine_device,
        "invalid_signature",
        "Participant provided malformed signature",
    ).await;
    
    assert!(result.is_ok(), "Recording Byzantine behavior should succeed");
}

/// Test choreographic effects adapter with multiple operations
#[tokio::test]
async fn test_choreographic_effects_multiple_operations() {
    let device_id = Uuid::new_v4();
    let effects = Effects::deterministic(55555, 0);
    let adapter = ChoreographicEffectsAdapter::new(
        device_id,
        effects,
        "MultiOpTest".to_string(),
    );
    
    // Record a sequence of protocol operations
    let operations = vec![
        ("init", "Protocol initialization"),
        ("commit", "Commitment phase"),
        ("sign", "Signature generation"),
        ("verify", "Signature verification"),
        ("complete", "Protocol completion"),
    ];
    
    for (event, phase) in operations {
        let result = adapter.record_choreographic_event(event, device_id, phase).await;
        assert!(result.is_ok(), "Recording {} event should succeed", event);
    }
    
    // Record phase transitions
    let transitions = vec![
        ("init", "commit"),
        ("commit", "sign"),
        ("sign", "verify"),
        ("verify", "complete"),
    ];
    
    for (from, to) in transitions {
        let result = adapter.record_phase_transition(from, to, device_id).await;
        assert!(result.is_ok(), "Transition from {} to {} should succeed", from, to);
    }
}

/// Test effects access through adapter
#[tokio::test]
async fn test_choreographic_effects_access() {
    let device_id = Uuid::new_v4();
    let effects = Effects::deterministic(77777, 0);
    let adapter = ChoreographicEffectsAdapter::new(
        device_id,
        effects.clone(),
        "EffectsAccessTest".to_string(),
    );
    
    // Test accessing underlying effects
    let adapter_effects = adapter.effects();
    
    // Verify we can use effects functions
    let random_bytes = adapter_effects.random_bytes_array::<16>();
    assert_eq!(random_bytes.len(), 16, "Should generate 16 random bytes");
    
    let test_data = b"test data for hashing";
    let hash = adapter_effects.blake3_hash(test_data);
    assert_eq!(hash.len(), 32, "Blake3 hash should be 32 bytes");
    
    // Test that deterministic effects produce same results
    let effects2 = Effects::deterministic(77777, 0);
    let adapter2 = ChoreographicEffectsAdapter::new(
        device_id,
        effects2,
        "EffectsAccessTest".to_string(),
    );
    
    let random_bytes2 = adapter2.effects().random_bytes_array::<16>();
    let hash2 = adapter2.effects().blake3_hash(test_data);
    
    assert_eq!(random_bytes, random_bytes2, "Deterministic effects should produce same random bytes");
    assert_eq!(hash, hash2, "Deterministic effects should produce same hash");
}

/// Test choreographic effects adapter error conversion
#[tokio::test]
async fn test_choreographic_effects_error_conversion() {
    let device_id = Uuid::new_v4();
    let effects = Effects::deterministic(88888, 0);
    let adapter = ChoreographicEffectsAdapter::new(
        device_id,
        effects,
        "ErrorTest".to_string(),
    );
    
    // Test timeout error conversion
    let start_time = tokio::time::Instant::now();
    let zero_timeout = std::time::Duration::from_millis(0);
    
    // Small delay to ensure timeout
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    
    let result = adapter.check_timeout(start_time, zero_timeout);
    match result {
        Err(ChoreographyError::ProtocolViolation(msg)) => {
            assert!(msg.contains("timeout") || msg.contains("Timeout"), 
                   "Error message should mention timeout: {}", msg);
        }
        Err(other) => {
            panic!("Expected ProtocolViolation with timeout, got: {:?}", other);
        }
        Ok(_) => {
            panic!("Expected timeout error, but operation succeeded");
        }
    }
}

/// Test choreographic effects adapter concurrent operations
#[tokio::test]
async fn test_choreographic_effects_concurrent() {
    let device_id = Uuid::new_v4();
    let effects = Effects::deterministic(99999, 0);
    let adapter = ChoreographicEffectsAdapter::new(
        device_id,
        effects,
        "ConcurrentTest".to_string(),
    );
    
    // Run multiple operations concurrently
    let mut handles = Vec::new();
    
    for i in 0..5 {
        let adapter_clone = adapter.clone();
        let handle = tokio::spawn(async move {
            adapter_clone.record_choreographic_event(
                &format!("concurrent_event_{}", i),
                device_id,
                &format!("phase_{}", i),
            ).await
        });
        handles.push(handle);
    }
    
    // Wait for all operations to complete
    for handle in handles {
        let result = handle.await.expect("Task should complete");
        assert!(result.is_ok(), "Concurrent operation should succeed");
    }
}

/// Integration test combining multiple components
#[tokio::test]
async fn test_middleware_integration_full() {
    let device_id = Uuid::new_v4();
    let effects = Effects::deterministic(11111, 0);
    let adapter = ChoreographicEffectsAdapter::new(
        device_id,
        effects,
        "FullIntegrationTest".to_string(),
    );
    
    // Simulate a complete protocol execution
    let start_time = tokio::time::Instant::now();
    let timeout = std::time::Duration::from_secs(30);
    
    // Phase 1: Initialization
    adapter.record_choreographic_event("protocol_start", device_id, "init").await
        .expect("Should record protocol start");
    adapter.record_phase_transition("init", "setup", device_id).await
        .expect("Should record phase transition");
    
    // Phase 2: Message exchange simulation
    let peer_devices: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
    
    for peer in &peer_devices {
        adapter.record_message_send(device_id, *peer, "SetupMessage", 256).await
            .expect("Should record message send");
    }
    
    adapter.record_phase_transition("setup", "execution", device_id).await
        .expect("Should record phase transition");
    
    // Phase 3: Execution
    adapter.record_choreographic_event("execution_start", device_id, "execution").await
        .expect("Should record execution start");
    
    // Simulate some work
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    
    // Phase 4: Completion
    adapter.record_phase_transition("execution", "completion", device_id).await
        .expect("Should record phase transition");
    adapter.record_choreographic_event("protocol_complete", device_id, "completion").await
        .expect("Should record protocol completion");
    
    // Verify we're still within timeout
    let result = adapter.check_timeout(start_time, timeout);
    assert!(result.is_ok(), "Protocol should complete within timeout");
}