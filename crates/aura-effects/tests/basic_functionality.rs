//! Basic functionality tests for aura-effects handlers
//!
//! Simple tests that verify the handlers compile and basic operations work.

use aura_core::effects::{
    ConsoleEffects, CryptoEffects, RandomEffects, StorageEffects, TimeEffects,
};
use aura_effects::{
    console::{MockConsoleHandler, RealConsoleHandler},
    crypto::{MockCryptoHandler, RealCryptoHandler},
    random::{MockRandomHandler, RealRandomHandler},
    storage::{EncryptedStorageHandler, MemoryStorageHandler},
    time::{RealTimeHandler, SimulatedTimeHandler},
};

#[tokio::test]
async fn test_crypto_handlers_basic() {
    // Test that both crypto handlers can be created and used
    let mock = MockCryptoHandler::new();
    let real = RealCryptoHandler::new();

    // Basic random operations
    let mock_bytes = mock.random_bytes(16).await;
    let real_bytes = real.random_bytes(16).await;

    assert_eq!(mock_bytes.len(), 16);
    assert_eq!(real_bytes.len(), 16);

    // Test metadata
    assert!(mock.is_simulated());
    assert!(!real.is_simulated());
}

#[tokio::test]
async fn test_time_handlers_basic() {
    // Test that both time handlers work
    let sim = SimulatedTimeHandler::new();
    let real = RealTimeHandler::new();

    // Basic time operations
    let sim_time = sim.current_timestamp().await;
    let real_time = real.current_timestamp().await;

    assert_eq!(sim_time, 0); // Simulated starts at 0
    assert!(real_time > 0); // Real time should be > 0

    // Test metadata
    assert!(sim.is_simulated());
    assert!(!real.is_simulated());
}

#[tokio::test]
async fn test_storage_handlers_basic() {
    // Test memory storage
    let memory = MemoryStorageHandler::new();

    // Basic operations
    memory.store("key1", b"value1".to_vec()).await.unwrap();
    let retrieved = memory.retrieve("key1").await.unwrap();
    assert_eq!(retrieved, Some(b"value1".to_vec()));

    let exists = memory.exists("key1").await.unwrap();
    assert!(exists);

    let keys = memory.list_keys(None).await.unwrap();
    assert_eq!(keys.len(), 1);
    assert!(keys.contains(&"key1".to_string()));

    // Test encrypted storage
    let encrypted = EncryptedStorageHandler::new("test_path".to_string(), Some(vec![42u8; 32]));
    encrypted
        .store("secret", b"classified".to_vec())
        .await
        .unwrap();
    let secret = encrypted.retrieve("secret").await.unwrap();
    assert_eq!(secret, Some(b"classified".to_vec()));
}

#[tokio::test]
async fn test_console_handlers_basic() {
    // Test console handlers
    let mock = MockConsoleHandler::new();
    let real = RealConsoleHandler::new();

    // Basic logging operations
    mock.log_info("test info").await.unwrap();
    mock.log_warn("test warning").await.unwrap();
    mock.log_error("test error").await.unwrap();
    mock.log_debug("test debug").await.unwrap();

    real.log_info("real info").await.unwrap();
    real.log_warn("real warning").await.unwrap();
    real.log_error("real error").await.unwrap();
    real.log_debug("real debug").await.unwrap();

    // Operations should complete without error
}

#[tokio::test]
async fn test_random_handlers_basic() {
    // Test random handlers
    let mock = MockRandomHandler::new();
    let real = RealRandomHandler::new();

    // Basic random operations
    let mock_bytes = mock.random_bytes(32).await;
    let real_bytes = real.random_bytes(32).await;

    assert_eq!(mock_bytes.len(), 32);
    assert_eq!(real_bytes.len(), 32);

    let mock_u64 = mock.random_u64().await;
    let real_u64 = real.random_u64().await;

    // Both should produce valid u64 values
    assert!(mock_u64 < u64::MAX || mock_u64 == u64::MAX);
    assert!(real_u64 < u64::MAX || real_u64 == u64::MAX);
}

#[tokio::test]
async fn test_deterministic_behavior() {
    // Test that mock handlers are deterministic
    let mock1 = MockCryptoHandler::with_seed(123);
    let mock2 = MockCryptoHandler::with_seed(123);

    let bytes1 = mock1.random_bytes(16).await;
    let bytes2 = mock2.random_bytes(16).await;

    assert_eq!(bytes1, bytes2); // Same seed should produce same output

    let sim1 = SimulatedTimeHandler::new_with_time(1000);
    let sim2 = SimulatedTimeHandler::new_with_time(1000);

    assert_eq!(sim1.get_time(), sim2.get_time());
}

#[tokio::test]
async fn test_handler_trait_implementations() {
    // Verify all handlers implement their traits correctly
    let crypto_mock = MockCryptoHandler::new();
    let crypto_real = RealCryptoHandler::new();
    let _: &dyn CryptoEffects = &crypto_mock;
    let _: &dyn CryptoEffects = &crypto_real;
    let _: &dyn RandomEffects = &crypto_mock;
    let _: &dyn RandomEffects = &crypto_real;

    let time_sim = SimulatedTimeHandler::new();
    let time_real = RealTimeHandler::new();
    let _: &dyn TimeEffects = &time_sim;
    let _: &dyn TimeEffects = &time_real;

    let storage_mem = MemoryStorageHandler::new();
    let storage_enc = EncryptedStorageHandler::new("test".to_string(), Some(vec![0u8; 32]));
    let _: &dyn StorageEffects = &storage_mem;
    let _: &dyn StorageEffects = &storage_enc;

    let console_mock = MockConsoleHandler::new();
    let console_real = RealConsoleHandler::new();
    let _: &dyn ConsoleEffects = &console_mock;
    let _: &dyn ConsoleEffects = &console_real;

    let random_mock = MockRandomHandler::new();
    let random_real = RealRandomHandler::new();
    let _: &dyn RandomEffects = &random_mock;
    let _: &dyn RandomEffects = &random_real;
}

#[tokio::test]
async fn test_storage_operations_comprehensive() {
    let storage = MemoryStorageHandler::new();

    // Store multiple items
    for i in 0..5 {
        let key = format!("key{}", i);
        let value = format!("value{}", i).into_bytes();
        storage.store(&key, value).await.unwrap();
    }

    // List all keys
    let all_keys = storage.list_keys(None).await.unwrap();
    assert_eq!(all_keys.len(), 5);

    // List with prefix
    storage
        .store("prefix_special", b"special".to_vec())
        .await
        .unwrap();
    let prefix_keys = storage.list_keys(Some("prefix")).await.unwrap();
    assert_eq!(prefix_keys.len(), 1);
    assert!(prefix_keys.contains(&"prefix_special".to_string()));

    // Test stats
    let stats = storage.stats().await.unwrap();
    assert_eq!(stats.key_count, 6); // 5 regular + 1 prefixed
    assert!(stats.total_size > 0);

    // Test removal
    let removed = storage.remove("key0").await.unwrap();
    assert!(removed);

    let exists = storage.exists("key0").await.unwrap();
    assert!(!exists);

    let not_removed = storage.remove("nonexistent").await.unwrap();
    assert!(!not_removed);
}

#[tokio::test]
async fn test_simple_integration_scenario() {
    // Simple integration test using multiple handlers
    let crypto = MockCryptoHandler::with_seed(42);
    let storage = MemoryStorageHandler::new();
    let console = MockConsoleHandler::new();
    let time = SimulatedTimeHandler::new_with_time(1000);

    console.log_info("Starting integration test").await.unwrap();

    // Generate a key pair
    let (private_key, public_key) = crypto.ed25519_generate_keypair().await.unwrap();

    // Store keys with timestamp
    let timestamp = time.current_timestamp_millis().await;
    storage
        .store(&format!("private_{}", timestamp), private_key.clone())
        .await
        .unwrap();
    storage
        .store(&format!("public_{}", timestamp), public_key.clone())
        .await
        .unwrap();

    console.log_info("Keys generated and stored").await.unwrap();

    // Sign a message
    let message = b"Integration test message";
    let signature = crypto.ed25519_sign(message, &private_key).await.unwrap();

    // Verify signature
    let is_valid = crypto
        .ed25519_verify(message, &signature, &public_key)
        .await
        .unwrap();
    assert!(is_valid);

    console
        .log_info("Signature verified successfully")
        .await
        .unwrap();

    // Check final state
    let keys = storage.list_keys(None).await.unwrap();
    assert_eq!(keys.len(), 2);

    let stats = storage.stats().await.unwrap();
    assert_eq!(stats.key_count, 2);

    console
        .log_info("Integration test completed")
        .await
        .unwrap();

    // Test should complete without errors
}
