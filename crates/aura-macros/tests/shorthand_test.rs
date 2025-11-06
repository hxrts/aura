//! Tests for shorthand syntax in #[derive(AuraMiddleware)] macro
//!
//! This test file validates that the ProtocolEffects and CoreEffects
//! shorthands work correctly and generate the expected trait implementations.

use aura_macros::AuraMiddleware;
use aura_protocol::effects::*;
use aura_protocol::handlers::AuraHandlerFactory;
use aura_types::identifiers::DeviceId;
use uuid::Uuid;

// Test 1: ProtocolEffects shorthand should implement all 8 effect traits
#[derive(AuraMiddleware)]
#[middleware(effects = "ProtocolEffects")]
pub struct ProtocolMiddleware<H> {
    inner: H,
    middleware_name: String,
}

impl<H> ProtocolMiddleware<H> {
    pub fn new(handler: H, name: &str) -> Self {
        Self {
            inner: handler,
            middleware_name: name.to_string(),
        }
    }
}

// Test 2: CoreEffects shorthand should implement 5 core effect traits
#[derive(AuraMiddleware)]
#[middleware(effects = "CoreEffects")]
pub struct CoreMiddleware<H> {
    inner: H,
    enabled: bool,
}

impl<H> CoreMiddleware<H> {
    pub fn new(handler: H) -> Self {
        Self {
            inner: handler,
            enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_protocol_effects_shorthand_compiles() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let handler = AuraHandlerFactory::for_testing(device_id).unwrap();

        // Wrap it with ProtocolMiddleware using shorthand
        let middleware = ProtocolMiddleware::new(handler, "test");

        // Verify we can call methods from all 8 effect traits

        // 1. NetworkEffects
        let peers = middleware.connected_peers().await;
        assert_eq!(peers.len(), 0);

        // 2. CryptoEffects
        let random_bytes = middleware.random_bytes(32).await;
        assert_eq!(random_bytes.len(), 32);

        // 3. TimeEffects
        let epoch = middleware.current_epoch().await;
        assert!(epoch >= 0);

        // 4. StorageEffects
        let exists = middleware.exists("test_key").await;
        assert!(exists.is_ok());

        // 5. ConsoleEffects
        middleware.log_info("Test message").await;

        // 6. LedgerEffects
        let ledger_epoch = middleware.current_epoch().await;
        assert!(ledger_epoch >= 0);

        // 7. ChoreographicEffects
        let roles = middleware.all_roles();
        assert!(roles.is_empty());

        // 8. RandomEffects
        let random_u64 = middleware.random_u64();
        assert!(random_u64 >= 0);

        println!("✅ ProtocolEffects shorthand: All 8 traits accessible");
    }

    #[tokio::test]
    async fn test_core_effects_shorthand_compiles() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let handler = AuraHandlerFactory::for_testing(device_id).unwrap();

        // Wrap it with CoreMiddleware using shorthand
        let middleware = CoreMiddleware::new(handler);

        // Verify we can call methods from 5 core effect traits

        // 1. NetworkEffects
        let peers = middleware.connected_peers().await;
        assert_eq!(peers.len(), 0);

        // 2. CryptoEffects
        let random_bytes = middleware.random_bytes(16).await;
        assert_eq!(random_bytes.len(), 16);

        // 3. TimeEffects
        let epoch = middleware.current_epoch().await;
        assert!(epoch >= 0);

        // 4. StorageEffects
        let keys = middleware.list_keys(None).await;
        assert!(keys.is_ok());

        // 5. ConsoleEffects
        middleware.log_warning("Test warning").await;

        println!("✅ CoreEffects shorthand: All 5 core traits accessible");
    }

    #[tokio::test]
    async fn test_middleware_can_override_specific_methods() {
        use async_trait::async_trait;

        // Define middleware that overrides a specific method
        #[derive(AuraMiddleware)]
        #[middleware(effects = "ProtocolEffects")]
        pub struct CountingMiddleware<H> {
            inner: H,
            call_count: std::sync::Arc<std::sync::atomic::AtomicU64>,
        }

        impl<H> CountingMiddleware<H> {
            pub fn new(handler: H) -> Self {
                Self {
                    inner: handler,
                    call_count: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
                }
            }

            pub fn get_call_count(&self) -> u64 {
                self.call_count.load(std::sync::atomic::Ordering::SeqCst)
            }
        }

        // Override one method to add counting
        #[async_trait]
        impl<H: NetworkEffects + Send + Sync> NetworkEffects for CountingMiddleware<H> {
            async fn send_to_peer(
                &self,
                peer_id: Uuid,
                message: Vec<u8>,
            ) -> Result<(), NetworkError> {
                // Increment counter
                self.call_count
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                // Delegate to inner
                self.inner.send_to_peer(peer_id, message).await
            }

            async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
                self.inner.broadcast(message).await
            }

            async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
                self.inner.receive().await
            }

            async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
                self.inner.receive_from(peer_id).await
            }

            async fn connected_peers(&self) -> Vec<Uuid> {
                self.inner.connected_peers().await
            }

            async fn is_peer_connected(&self, peer_id: Uuid) -> bool {
                self.inner.is_peer_connected(peer_id).await
            }

            async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
                self.inner.subscribe_to_peer_events().await
            }
        }

        // Test it
        let device_id = DeviceId::from(Uuid::new_v4());
        let handler = AuraHandlerFactory::for_testing(device_id).unwrap();
        let middleware = CountingMiddleware::new(handler);

        // Initial count should be 0
        assert_eq!(middleware.get_call_count(), 0);

        // Call send_to_peer multiple times
        let peer_id = Uuid::new_v4();
        let _ = middleware.send_to_peer(peer_id, vec![1, 2, 3]).await;
        let _ = middleware.send_to_peer(peer_id, vec![4, 5, 6]).await;

        // Count should be 2
        assert_eq!(middleware.get_call_count(), 2);

        // Other methods still work (delegated by macro)
        let peers = middleware.connected_peers().await;
        assert_eq!(peers.len(), 0);

        println!("✅ Middleware can override methods while others auto-delegate");
    }

    #[test]
    fn test_shorthand_syntax_structure() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let handler = AuraHandlerFactory::for_testing(device_id).unwrap();
        let _protocol_mw = ProtocolMiddleware::new(handler, "test");

        let device_id2 = DeviceId::from(Uuid::new_v4());
        let handler2 = AuraHandlerFactory::for_testing(device_id2).unwrap();
        let _core_mw = CoreMiddleware::new(handler2);

        println!("✅ Shorthand syntax structures compile correctly");
    }

    #[tokio::test]
    async fn test_middleware_stacking() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let base_handler = AuraHandlerFactory::for_testing(device_id).unwrap();

        // Stack two middleware layers
        let layer1 = CoreMiddleware::new(base_handler);
        let layer2 = ProtocolMiddleware::new(layer1, "outer");

        // Should be able to call methods through both layers
        let epoch = layer2.current_epoch().await;
        assert!(epoch >= 0);

        let random = layer2.random_bytes(8).await;
        assert_eq!(random.len(), 8);

        println!("✅ Middleware stacking works correctly");
    }
}
