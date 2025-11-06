//! Tests for #[derive(AuraMiddleware)] macro

use async_trait::async_trait;
use aura_macros::AuraMiddleware;
use aura_protocol::effects::*;
use std::collections::HashMap;
use uuid::Uuid;

// Test helper: Simple mock handler implementing all effects
struct MockHandler;

#[async_trait]
impl NetworkEffects for MockHandler {
    async fn send_to_peer(&self, _peer_id: Uuid, _message: Vec<u8>) -> Result<(), NetworkError> {
        Ok(())
    }

    async fn broadcast(&self, _message: Vec<u8>) -> Result<(), NetworkError> {
        Ok(())
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        Ok((Uuid::new_v4(), vec![1, 2, 3]))
    }

    async fn receive_from(&self, _peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        Ok(vec![4, 5, 6])
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        vec![]
    }

    async fn is_peer_connected(&self, _peer_id: Uuid) -> bool {
        false
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        Err(NetworkError::Protocol {
            message: "Not implemented".to_string(),
        })
    }
}

#[async_trait]
impl CryptoEffects for MockHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        vec![42; len]
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        [42; 32]
    }

    async fn random_range(&self, range: std::ops::Range<u64>) -> u64 {
        range.start
    }

    async fn blake3_hash(&self, _data: &[u8]) -> [u8; 32] {
        [0; 32]
    }

    async fn sha256_hash(&self, _data: &[u8]) -> [u8; 32] {
        [1; 32]
    }

    async fn ed25519_sign(
        &self,
        _data: &[u8],
        _key: &ed25519_dalek::SigningKey,
    ) -> Result<ed25519_dalek::Signature, CryptoError> {
        Err(CryptoError::OperationFailed {
            operation: "signing not implemented".to_string(),
        })
    }

    async fn ed25519_verify(
        &self,
        _data: &[u8],
        _signature: &ed25519_dalek::Signature,
        _public_key: &ed25519_dalek::VerifyingKey,
    ) -> Result<bool, CryptoError> {
        Ok(true)
    }

    async fn ed25519_generate_keypair(
        &self,
    ) -> Result<(ed25519_dalek::SigningKey, ed25519_dalek::VerifyingKey), CryptoError> {
        Err(CryptoError::KeyGenerationFailed {
            reason: "Not implemented".to_string(),
        })
    }

    async fn ed25519_public_key(
        &self,
        key: &ed25519_dalek::SigningKey,
    ) -> ed25519_dalek::VerifyingKey {
        key.verifying_key()
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        a == b
    }

    fn secure_zero(&self, data: &mut [u8]) {
        data.fill(0);
    }
}

#[async_trait]
impl TimeEffects for MockHandler {
    async fn current_epoch(&self) -> u64 {
        1000
    }

    async fn sleep_ms(&self, _ms: u64) {}

    async fn sleep_until(&self, _epoch: u64) {}

    async fn yield_until(&self, _condition: WakeCondition) -> Result<(), TimeError> {
        Ok(())
    }

    async fn set_timeout(&self, _timeout_ms: u64) -> TimeoutHandle {
        TimeoutHandle(Uuid::new_v4())
    }

    async fn cancel_timeout(&self, _handle: TimeoutHandle) -> Result<(), TimeError> {
        Ok(())
    }

    fn is_simulated(&self) -> bool {
        true
    }

    fn register_context(&self, _context_id: Uuid) {}

    fn unregister_context(&self, _context_id: Uuid) {}

    async fn notify_events_available(&self) {}

    fn resolution_ms(&self) -> u64 {
        10
    }
}

#[async_trait]
impl StorageEffects for MockHandler {
    async fn store(&self, _key: &str, _value: Vec<u8>) -> Result<(), StorageError> {
        Ok(())
    }

    async fn retrieve(&self, _key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(Some(vec![7, 8, 9]))
    }

    async fn remove(&self, _key: &str) -> Result<bool, StorageError> {
        Ok(true)
    }

    async fn list_keys(&self, _prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        Ok(vec![])
    }

    async fn exists(&self, _key: &str) -> Result<bool, StorageError> {
        Ok(false)
    }

    async fn store_batch(&self, _pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        Ok(())
    }

    async fn retrieve_batch(
        &self,
        _keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        Ok(HashMap::new())
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        Ok(StorageStats {
            key_count: 0,
            total_size: 0,
            available_space: Some(1000),
            backend_type: "memory".to_string(),
        })
    }
}

// Test 1: Basic middleware with single effect trait
#[derive(AuraMiddleware)]
#[middleware(effects = "[NetworkEffects]")]
struct SimpleNetworkMiddleware<H> {
    inner: H,
}

#[tokio::test]
async fn test_simple_middleware_network() {
    let handler = MockHandler;
    let middleware = SimpleNetworkMiddleware { inner: handler };

    // Test delegation works
    let peer_id = Uuid::new_v4();
    let result = middleware.send_to_peer(peer_id, vec![1, 2, 3]).await;
    assert!(result.is_ok());

    let (_from, msg) = middleware.receive().await.unwrap();
    assert_eq!(msg, vec![1, 2, 3]);
}

// Test 2: Middleware with multiple effect traits
#[derive(AuraMiddleware)]
#[middleware(effects = "[NetworkEffects, CryptoEffects, TimeEffects]")]
struct MultiEffectMiddleware<H> {
    inner: H,
    device_id: Uuid,
}

#[tokio::test]
async fn test_multi_effect_middleware() {
    let handler = MockHandler;
    let middleware = MultiEffectMiddleware {
        inner: handler,
        device_id: Uuid::new_v4(),
    };

    // Test NetworkEffects
    let result = middleware.broadcast(vec![1, 2, 3]).await;
    assert!(result.is_ok());

    // Test CryptoEffects
    let random = middleware.random_bytes(10).await;
    assert_eq!(random.len(), 10);
    assert_eq!(random, vec![42; 10]);

    // Test TimeEffects
    let epoch = middleware.current_epoch().await;
    assert_eq!(epoch, 1000);
}

// Test 3: Middleware with storage effects
#[derive(AuraMiddleware)]
#[middleware(effects = "[StorageEffects]")]
struct StorageMiddleware<H> {
    inner: H,
}

#[tokio::test]
async fn test_storage_middleware() {
    let handler = MockHandler;
    let middleware = StorageMiddleware { inner: handler };

    // Test store
    let result = middleware.store("test_key", vec![1, 2, 3]).await;
    assert!(result.is_ok());

    // Test retrieve
    let value = middleware.retrieve("test_key").await.unwrap();
    assert_eq!(value, Some(vec![7, 8, 9]));

    // Test remove
    let removed = middleware.remove("test_key").await.unwrap();
    assert!(removed);

    // Test stats
    let stats = middleware.stats().await.unwrap();
    assert_eq!(stats.key_count, 0);
}

// Test 4: Middleware with all effect traits
#[derive(AuraMiddleware)]
#[middleware(effects = "[NetworkEffects, CryptoEffects, TimeEffects, StorageEffects]")]
struct FullEffectsMiddleware<H> {
    inner: H,
    config: String,
}

#[tokio::test]
async fn test_full_effects_middleware() {
    let handler = MockHandler;
    let middleware = FullEffectsMiddleware {
        inner: handler,
        config: "test".to_string(),
    };

    // Test all effects work
    assert!(middleware.broadcast(vec![1]).await.is_ok());
    assert_eq!(middleware.random_bytes(5).await.len(), 5);
    assert_eq!(middleware.current_epoch().await, 1000);
    assert!(middleware.store("key", vec![1]).await.is_ok());
}

// Test 5: Nested middleware composition
#[derive(AuraMiddleware)]
#[middleware(effects = "[CryptoEffects]")]
struct InnerMiddleware<H> {
    inner: H,
}

#[derive(AuraMiddleware)]
#[middleware(effects = "[CryptoEffects]")]
struct OuterMiddleware<H> {
    inner: H,
}

#[tokio::test]
async fn test_nested_middleware() {
    let handler = MockHandler;
    let inner_mw = InnerMiddleware { inner: handler };
    let outer_mw = OuterMiddleware { inner: inner_mw };

    // Test nested delegation
    let random = outer_mw.random_bytes(8).await;
    assert_eq!(random.len(), 8);
    assert_eq!(random, vec![42; 8]);
}

// Test 6: Middleware with custom fields
#[derive(AuraMiddleware)]
#[middleware(effects = "[TimeEffects]")]
struct CustomFieldsMiddleware<H> {
    inner: H,
    device_id: Uuid,
    service_name: String,
    counter: u64,
}

#[tokio::test]
async fn test_custom_fields_middleware() {
    let handler = MockHandler;
    let middleware = CustomFieldsMiddleware {
        inner: handler,
        device_id: Uuid::new_v4(),
        service_name: "test_service".to_string(),
        counter: 42,
    };

    // Fields don't interfere with effect delegation
    assert_eq!(middleware.current_epoch().await, 1000);
    assert!(middleware.is_simulated());
}

// Test 7: Async/await handling
#[derive(AuraMiddleware)]
#[middleware(effects = "[NetworkEffects]")]
struct AsyncMiddleware<H> {
    inner: H,
}

#[tokio::test]
async fn test_async_await() {
    let handler = MockHandler;
    let middleware = AsyncMiddleware { inner: handler };

    // All methods are properly async
    let peer_id = Uuid::new_v4();

    let send_future = middleware.send_to_peer(peer_id, vec![1, 2, 3]);
    let result = send_future.await;
    assert!(result.is_ok());

    let recv_future = middleware.receive();
    let (_, msg) = recv_future.await.unwrap();
    assert_eq!(msg.len(), 3);
}

// Test 8: Error propagation
#[derive(AuraMiddleware)]
#[middleware(effects = "[NetworkEffects]")]
struct ErrorMiddleware<H> {
    inner: H,
}

#[tokio::test]
async fn test_error_propagation() {
    let handler = MockHandler;
    let middleware = ErrorMiddleware { inner: handler };

    // Errors from inner handler propagate correctly
    let result = middleware.subscribe_to_peer_events().await;
    assert!(result.is_err());

    match result {
        Err(NetworkError::Protocol { message }) => {
            assert_eq!(message, "Not implemented");
        }
        _ => panic!("Expected Protocol error"),
    }
}

// Test 9: Trait bounds compilation
#[derive(AuraMiddleware)]
#[middleware(effects = "[CryptoEffects]")]
struct BoundedMiddleware<H> {
    inner: H,
}

// This test verifies that the macro generates correct trait bounds
fn _assert_bounds<H: CryptoEffects + Send + Sync>() {
    fn check<T: CryptoEffects>() {}
    check::<BoundedMiddleware<H>>();
}

// Test 10: Lifetime handling
#[derive(AuraMiddleware)]
#[middleware(effects = "[StorageEffects]")]
struct LifetimeMiddleware<H> {
    inner: H,
}

#[tokio::test]
async fn test_lifetime_handling() {
    let handler = MockHandler;
    let middleware = LifetimeMiddleware { inner: handler };

    // References in method signatures work correctly
    let key = "test_key";
    let exists = middleware.exists(key).await.unwrap();
    assert!(!exists);

    let keys = vec!["key1".to_string(), "key2".to_string()];
    let batch = middleware.retrieve_batch(&keys).await.unwrap();
    assert!(batch.is_empty());
}
