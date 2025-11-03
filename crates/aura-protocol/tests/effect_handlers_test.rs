//! Tests for effect handlers
//!
//! This module tests the different handler implementations (real, mock, simulation)
//! and their conformance to the effect trait contracts.

mod common;

use aura_protocol::{
    handlers::{CompositeHandler, crypto::*, network::*, storage::*, time::*},
    effects::*,
};
use common::{helpers::*, test_utils::*};
use std::collections::HashMap;

/// Test that composite handlers implement all required effect traits
#[tokio::test]
async fn test_composite_handler_implements_all_effects() {
    let handler = create_test_handler();
    
    // Test that handler can be used as each effect type
    let network_effect: &dyn NetworkEffects = &handler;
    let storage_effect: &dyn StorageEffects = &handler;
    let crypto_effect: &dyn CryptoEffects = &handler;
    let time_effect: &dyn TimeEffects = &handler;
    let console_effect: &dyn ConsoleEffects = &handler;
    let ledger_effect: &dyn LedgerEffects = &handler;
    let choreographic_effect: &dyn ChoreographicEffects = &handler;

    // Basic smoke tests to ensure traits are working
    let peers = network_effect.connected_peers().await;
    assert!(peers.is_empty()); // No peers connected in test mode
    
    let random_bytes = crypto_effect.random_bytes(10).await;
    assert_eq!(random_bytes.len(), 10);
    
    let current_time = time_effect.current_epoch().await;
    assert!(current_time > 0);
}

/// Test network effects with different handler types
#[tokio::test]
async fn test_network_effects() {
    // Test with memory handler
    let device_id = create_test_device_id();
    let memory_handler = MemoryNetworkHandler::new(device_id.into());
    let peer_id = create_deterministic_uuid(1);
    let test_message = create_test_data(10);
    
    // Test send to peer (should succeed in memory)
    let result = memory_handler.send_to_peer(peer_id, test_message.clone()).await;
    assert!(result.is_ok());
    
    // Test broadcast
    let result = memory_handler.broadcast(test_message.clone()).await;
    assert!(result.is_ok());
    
    // Test connected peers
    let peers = memory_handler.connected_peers().await;
    assert!(peers.is_empty()); // Memory handler starts with no peers
    
    // Test real handler (basic instantiation)
    let device_id = create_test_device_id();
    let real_handler = RealNetworkHandler::new(device_id.into(), "tcp://localhost:0".to_string());
    assert!(!real_handler.is_peer_connected(peer_id).await);
}

/// Test storage effects with different handler types
#[tokio::test]
async fn test_storage_effects() {
    // Test with memory handler
    let memory_handler = MemoryStorageHandler::new();
    let test_key = "test_key";
    let test_value = create_test_data(20);
    
    // Test store and retrieve
    memory_handler.store(test_key, test_value.clone()).await.unwrap();
    let retrieved = memory_handler.retrieve(test_key).await.unwrap();
    assert_eq!(retrieved, Some(test_value.clone()));
    
    // Test exists
    assert!(memory_handler.exists(test_key).await.unwrap());
    assert!(!memory_handler.exists("nonexistent").await.unwrap());
    
    // Test list keys
    memory_handler.store("prefix_test", b"value".to_vec()).await.unwrap();
    let keys = memory_handler.list_keys(Some("prefix")).await.unwrap();
    assert_eq!(keys, vec!["prefix_test"]);
    
    // Test remove
    assert!(memory_handler.remove(test_key).await.unwrap());
    assert!(!memory_handler.exists(test_key).await.unwrap());
    
    // Test batch operations
    let mut batch = HashMap::new();
    batch.insert("batch1".to_string(), b"value1".to_vec());
    batch.insert("batch2".to_string(), b"value2".to_vec());
    
    memory_handler.store_batch(batch.clone()).await.unwrap();
    let retrieved_batch = memory_handler.retrieve_batch(&["batch1".to_string(), "batch2".to_string()]).await.unwrap();
    assert_eq!(retrieved_batch.len(), 2);
    assert_eq!(retrieved_batch.get("batch1"), Some(&b"value1".to_vec()));
    
    // Test stats
    let stats = memory_handler.stats().await.unwrap();
    assert!(stats.key_count > 0);
    
    // Test filesystem handler (basic instantiation)
    let fs_handler = FilesystemStorageHandler::new("/tmp/test_storage".into()).unwrap();
    let list_result = fs_handler.list_keys(None).await;
    assert!(list_result.is_ok());
}

/// Test crypto effects with different handler types
#[tokio::test]
async fn test_crypto_effects() {
    // Test with real handler
    let real_handler = RealCryptoHandler::new();
    
    // Test random bytes generation
    let bytes1 = real_handler.random_bytes(16).await;
    let bytes2 = real_handler.random_bytes(16).await;
    assert_eq!(bytes1.len(), 16);
    assert_eq!(bytes2.len(), 16);
    // Note: Real handler produces truly random bytes, so they should be different
    
    // Test random_bytes_32
    let bytes32 = real_handler.random_bytes_32().await;
    assert_eq!(bytes32.len(), 32);
    
    // Test random range
    let random_val = real_handler.random_range(10..20).await;
    assert!(random_val >= 10 && random_val < 20);
    
    // Test hashing
    let test_data = b"test data for hashing";
    let blake3_hash = real_handler.blake3_hash(test_data).await;
    let sha256_hash = real_handler.sha256_hash(test_data).await;
    assert_eq!(blake3_hash.len(), 32);
    assert_eq!(sha256_hash.len(), 32);
    
    // Test that same input produces same hash
    let blake3_hash2 = real_handler.blake3_hash(test_data).await;
    assert_eq!(blake3_hash, blake3_hash2);
    
    // Test ED25519 operations
    let (signing_key, verifying_key) = real_handler.ed25519_generate_keypair().await.unwrap();
    let public_key = real_handler.ed25519_public_key(&signing_key).await;
    assert_eq!(verifying_key, public_key);
    
    let message = b"test message to sign";
    let signature = real_handler.ed25519_sign(message, &signing_key).await.unwrap();
    let is_valid = real_handler.ed25519_verify(message, &signature, &verifying_key).await.unwrap();
    assert!(is_valid);
    
    // Test with wrong message
    let wrong_message = b"wrong message";
    let is_valid_wrong = real_handler.ed25519_verify(wrong_message, &signature, &verifying_key).await.unwrap();
    assert!(!is_valid_wrong);
    
    // Test constant time comparison
    let data1 = b"same data";
    let data2 = b"same data";
    let data3 = b"different";
    assert!(real_handler.constant_time_eq(data1, data2));
    assert!(!real_handler.constant_time_eq(data1, data3));
    
    // Test secure zeroing
    let mut sensitive_data = vec![0x42u8; 10];
    real_handler.secure_zero(&mut sensitive_data);
    assert!(sensitive_data.iter().all(|&b| b == 0));
}

/// Test time effects
#[tokio::test]
async fn test_time_effects() {
    let real_handler = RealTimeHandler::new();
    
    // Test current timestamp
    let timestamp = real_handler.current_epoch().await;
    assert!(timestamp > 0);
    
    // Test sleep
    let start = std::time::Instant::now();
    real_handler.sleep_ms(10).await;
    let elapsed = start.elapsed();
    assert!(elapsed >= std::time::Duration::from_millis(5)); // Allow some variance
    
    // Test timeout with set_timeout
    let timeout_handle = real_handler.set_timeout(100).await;
    let cancel_result = real_handler.cancel_timeout(timeout_handle).await;
    assert!(cancel_result.is_ok());
}

/// Test console effects
#[tokio::test]
async fn test_console_effects() {
    let real_handler = StdoutConsoleHandler::new();
    
    // Test log methods (should not panic)
    real_handler.log_info("Test message").await;
    real_handler.log_error("Test error").await;
    
    // Test protocol events
    let protocol_id = create_deterministic_uuid(42);
    real_handler.protocol_started(protocol_id, "DKD").await;
    real_handler.protocol_completed(protocol_id, 1000).await;
    real_handler.protocol_failed(protocol_id, "test error").await;
}

/// Test ledger effects
#[tokio::test]
async fn test_ledger_effects() {
    let memory_handler = MemoryLedgerHandler::new();
    
    // Test device operations
    let device_id = create_test_device_id();
    let device_metadata = aura_journal::DeviceMetadata {
        device_id,
        device_name: "Test Device".to_string(),
        device_type: aura_journal::DeviceType::Native,
        public_key: create_test_keypair().1,
        added_at: 1000,
        last_seen: 1000,
        dkd_commitment_proofs: std::collections::BTreeMap::new(),
        next_nonce: 1,
        used_nonces: std::collections::BTreeSet::new(),
        key_share_epoch: 1,
    };
    
    memory_handler.add_device(device_metadata.clone()).await.unwrap();
    let retrieved_device = memory_handler.get_device(device_id).await.unwrap();
    assert!(retrieved_device.is_some());
    
    let all_devices = memory_handler.list_devices().await.unwrap();
    assert_eq!(all_devices.len(), 1);
    
    // Test account operations
    let account_id = aura_types::AccountId(create_deterministic_uuid(999));
    let account_exists = memory_handler.account_exists(account_id).await.unwrap();
    assert!(!account_exists); // New handler shouldn't have accounts
}

/// Test choreographic effects
#[tokio::test]
async fn test_choreographic_effects() {
    let memory_handler = MemoryChoreographicHandler::new();
    
    // Test role management
    let role_id = create_deterministic_uuid(1);
    let role = ChoreographicRole {
        device_id: role_id,
        role_index: 0,
    };
    
    memory_handler.register_role(role.clone()).await.unwrap();
    let retrieved_role = memory_handler.get_role(role_id).await.unwrap();
    assert!(retrieved_role.is_some());
    
    let all_roles = memory_handler.list_roles().await.unwrap();
    assert_eq!(all_roles.len(), 1);
    
    // Test choreography execution
    let choreography_id = create_deterministic_uuid(2);
    let protocol_name = "test_protocol";
    let participants = vec![create_test_device_id()];
    
    memory_handler.start_choreography(choreography_id, protocol_name.to_string(), participants).await.unwrap();
    let choreo_state = memory_handler.get_choreography_state(choreography_id).await.unwrap();
    assert!(choreo_state.is_some());
    
    // Test message sending
    let test_data = create_test_data(10);
    memory_handler.send_to_role(role_id, test_data.clone()).await.unwrap();
    
    // Test events
    let event = ChoreographyEvent {
        id: create_deterministic_uuid(3),
        choreography_id,
        role_id,
        event_type: "message_sent".to_string(),
        timestamp: 1000,
        data: test_data,
    };
    
    memory_handler.emit_event(event.clone()).await.unwrap();
    let events = memory_handler.get_events(choreography_id).await.unwrap();
    assert_eq!(events.len(), 1);
}

/// Test handler error conditions
#[tokio::test]
async fn test_handler_error_conditions() {
    let memory_storage = MemoryStorageHandler::new();
    
    // Test retrieving non-existent key
    let result = memory_storage.retrieve("nonexistent").await.unwrap();
    assert_eq!(result, None);
    
    // Test removing non-existent key
    let removed = memory_storage.remove("nonexistent").await.unwrap();
    assert!(!removed);
    
    let device_id = create_test_device_id();
    let memory_network = MemoryNetworkHandler::new(device_id.into());
    
    // Test sending to non-connected peer
    let peer_id = create_deterministic_uuid(999);
    let result = memory_network.send_to_peer(peer_id, vec![1, 2, 3]).await;
    // Memory handler should accept this (it's a mock)
    assert!(result.is_ok());
}

/// Test handler composition and polymorphism
#[tokio::test]
async fn test_handler_polymorphism() {
    // Test that we can use handlers polymorphically
    let handlers: Vec<Box<dyn StorageEffects + Send + Sync>> = vec![
        Box::new(MemoryStorageHandler::new()),
        Box::new(FilesystemStorageHandler::new("/tmp/test".into()).unwrap()),
    ];
    
    for handler in handlers {
        let test_key = "poly_test";
        let test_value = b"poly_value".to_vec();
        
        // Each handler should support the same interface
        let store_result = handler.store(test_key, test_value.clone()).await;
        let exists_result = handler.exists(test_key).await;
        let stats_result = handler.stats().await;
        
        // All operations should succeed (though results may differ)
        assert!(store_result.is_ok());
        assert!(exists_result.is_ok());
        assert!(stats_result.is_ok());
    }
}

/// Test that composite handlers properly delegate to sub-handlers
#[tokio::test]
async fn test_composite_handler_delegation() {
    let composite = create_test_handler();
    
    // Test network delegation
    let peers = composite.connected_peers().await;
    assert!(peers.is_empty());
    
    // Test storage delegation
    let test_key = "delegation_test";
    let test_value = b"test_value".to_vec();
    
    composite.store(test_key, test_value.clone()).await.unwrap();
    let retrieved = composite.retrieve(test_key).await.unwrap();
    assert_eq!(retrieved, Some(test_value));
    
    // Test crypto delegation
    let random_bytes = composite.random_bytes(8).await;
    assert_eq!(random_bytes.len(), 8);
    
    let test_data = b"delegation test";
    let hash = composite.blake3_hash(test_data).await;
    assert_eq!(hash.len(), 32);
    
    // Test time delegation
    let timestamp = TimeEffects::current_epoch(&composite).await;
    assert!(timestamp > 0);
}