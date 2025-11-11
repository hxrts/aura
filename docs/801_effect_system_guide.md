# Effect System Guide

Aura's effect system separates application logic from infrastructure implementation using algebraic effects. Effects define capabilities your application needs. Handlers provide concrete implementations for different environments.

This guide covers effect system architecture, handler composition patterns, testing strategies, and integration with choreographic protocols. You will learn to build maintainable applications that work across development, testing, and production environments.

See [Getting Started Guide](800_getting_started_guide.md) for basic concepts. See [Protocol Development Guide](803_protocol_development_guide.md) for choreographic integration patterns.

---

## Effect System Architecture

Aura's effect system is organized in clean architectural layers:

### Interface Layer (`aura-core`)

**Effect Traits** define capabilities without implementation details. Always import effect traits from `aura-core`:

```rust
// Import traits from aura-core
use aura_core::effects::{StorageEffects, CryptoEffects, NetworkEffects};

#[async_trait]
pub trait StorageEffects: Send + Sync {
    async fn store(&self, key: &str, data: &[u8]) -> Result<(), StorageError>;
    async fn load(&self, key: &str) -> Result<Vec<u8>, StorageError>;
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
}
```

### Implementation Layer (`aura-effects`)

**Standard Handlers** provide context-free implementations. Import implementations from `aura-effects`:

```rust
// Import handlers from aura-effects
use aura_effects::storage::{FilesystemStorageHandler, MemoryStorageHandler};
use aura_effects::crypto::{RealCryptoHandler, MockCryptoHandler};

// Standard implementations work in any execution context
let storage = FilesystemStorageHandler::new("/data".into())?;
let crypto = RealCryptoHandler::new();
```

### Orchestration Layer (`aura-protocol`) 

**Coordination Primitives** handle multi-party or stateful composition. Import coordination from `aura-protocol`:

```rust
// Import coordination from aura-protocol
use aura_protocol::handlers::{CompositeHandler, AuraHandlerAdapter};
use aura_protocol::effects::semilattice::CrdtCoordinator;
```

Effect traits use async methods to handle I/O operations. All effects implement `Send` and `Sync` for safe usage across async task boundaries.

**Effect Composition** combines multiple effect traits into unified interfaces. The `AuraEffectSystem` provides access to all platform capabilities through a single entry point.

```rust
pub struct AuraEffectSystem {
    storage: Arc<dyn StorageEffects>,
    crypto: Arc<dyn CryptoEffects>,
    journal: Arc<dyn JournalEffects>,
    network: Arc<dyn NetworkEffects>,
    time: Arc<dyn TimeEffects>,
}
```

Effect composition uses trait objects for runtime polymorphism. Applications receive effect systems through dependency injection rather than creating handlers directly.

**Handler Registry** manages effect implementations for different execution contexts. The registry selects appropriate handlers based on configuration and runtime environment.

```rust
pub enum ExecutionMode {
    Production,
    Testing,
    Simulation,
    Development,
}

impl AuraEffectSystem {
    pub fn for_mode(device_id: DeviceId, mode: ExecutionMode) -> Self {
        match mode {
            ExecutionMode::Production => Self::production_handlers(device_id),
            ExecutionMode::Testing => Self::testing_handlers(device_id),
            ExecutionMode::Simulation => Self::simulation_handlers(device_id),
            ExecutionMode::Development => Self::development_handlers(device_id),
        }
    }
}
```

Different execution modes provide different handler implementations. Testing modes use deterministic handlers while production modes use real infrastructure.

## Handler Patterns

### Standard Handlers (`aura-effects`)

**Production Handlers** from `aura-effects` implement effects using real infrastructure:

```rust
// Use standard handlers from aura-effects
use aura_effects::storage::FilesystemStorageHandler;
use aura_core::effects::StorageEffects;

// Standard implementation that works in any context
let storage = FilesystemStorageHandler::new("/data".into())?;

// All aura-effects handlers are context-free and stateless
let result = storage.store("key", b"data").await?;
```

**Testing Handlers** from `aura-effects` provide predictable behavior:

```rust
use aura_effects::storage::MemoryStorageHandler;
use aura_effects::crypto::MockCryptoHandler;

// Deterministic handlers for testing
let storage = MemoryStorageHandler::new();
let crypto = MockCryptoHandler::new();
```

### Coordination Handlers (`aura-protocol`)

**Multi-party Coordination** requires `aura-protocol` for stateful orchestration:

```rust
use aura_protocol::effects::semilattice::CrdtCoordinator;
use aura_protocol::handlers::CompositeHandler;

// Coordinates multiple CRDT handlers
let coordinator = CrdtCoordinator::new(device_id)
    .with_cv_handler(cv_state)
    .with_delta_handler(delta_threshold);
```

**Testing Handlers** provide predictable behavior for unit tests. Mock handlers eliminate external dependencies and enable fast test execution.

```rust
pub struct InMemoryStorageHandler {
    data: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

#[async_trait]
impl StorageEffects for InMemoryStorageHandler {
    async fn store(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
        let mut storage = self.data.lock().await;
        storage.insert(key.to_string(), data.to_vec());
        Ok(())
    }

    async fn load(&self, key: &str) -> Result<Vec<u8>, StorageError> {
        let storage = self.data.lock().await;
        storage
            .get(key)
            .cloned()
            .ok_or(StorageError::NotFound)
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let mut storage = self.data.lock().await;
        storage.remove(key);
        Ok(())
    }
}
```

Testing handlers store state in memory for isolation between test runs. These handlers provide deterministic behavior that simplifies test assertions.

**Simulation Handlers** inject failures and delays for robustness testing. These handlers help validate application behavior under adverse conditions.

```rust
pub struct FlakyStorageHandler {
    inner: Arc<dyn StorageEffects>,
    failure_rate: f64,
    delay_range: Range<Duration>,
}

#[async_trait]
impl StorageEffects for FlakyStorageHandler {
    async fn store(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inject_delay().await;
        
        if self.should_fail() {
            return Err(StorageError::NetworkTimeout);
        }
        
        self.inner.store(key, data).await
    }

    async fn load(&self, key: &str) -> Result<Vec<u8>, StorageError> {
        self.inject_delay().await;
        
        if self.should_fail() {
            return Err(StorageError::CorruptedData);
        }
        
        self.inner.load(key).await
    }
}
```

Simulation handlers wrap other handlers to inject realistic failure scenarios. This enables testing application resilience without relying on actual infrastructure failures.

## Testing Strategies

**Effect Isolation** enables testing individual components without external dependencies. Mock effects provide controlled environments for unit testing.

```rust
#[tokio::test]
async fn test_user_registration() {
    let device_id = DeviceId::new();
    let effects = AuraEffectSystem::for_testing(device_id);
    
    let service = UserRegistrationService::new(effects);
    
    let user_id = service.register_user("alice", "alice@example.com").await.unwrap();
    
    assert!(service.user_exists(user_id).await.unwrap());
}
```

Testing effect systems eliminate external dependencies like databases and network services. Tests execute quickly and produce consistent results.

**Effect Verification** validates that applications use effects correctly. Test helpers capture effect calls for assertion in tests.

```rust
pub struct CapturingStorageHandler {
    calls: Arc<Mutex<Vec<StorageCall>>>,
    inner: Arc<dyn StorageEffects>,
}

#[derive(Debug, Clone)]
pub enum StorageCall {
    Store { key: String, data_len: usize },
    Load { key: String },
    Delete { key: String },
}

#[tokio::test]
async fn test_storage_usage_pattern() {
    let capturing_handler = CapturingStorageHandler::new();
    let effects = AuraEffectSystem::with_storage(capturing_handler.clone());
    
    let service = DataService::new(effects);
    service.save_data("key1", b"data1").await.unwrap();
    service.save_data("key2", b"data2").await.unwrap();
    
    let calls = capturing_handler.get_calls().await;
    assert_eq!(calls.len(), 2);
    assert!(matches!(calls[0], StorageCall::Store { key, .. } if key == "key1"));
}
```

Capturing handlers record effect usage patterns for validation in tests. This ensures applications use effects efficiently and correctly.

**Integration Testing** validates effect composition and real infrastructure integration. These tests use actual handlers with temporary resources.

```rust
#[tokio::test]
async fn test_end_to_end_data_flow() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = FileSystemStorageHandler::new(temp_dir.path());
    let effects = AuraEffectSystem::with_storage(Arc::new(storage));
    
    let service = DataService::new(effects);
    
    service.save_data("test_key", b"test_data").await.unwrap();
    let loaded_data = service.load_data("test_key").await.unwrap();
    
    assert_eq!(loaded_data, b"test_data");
}
```

Integration tests validate that production handlers work correctly with real infrastructure. These tests catch issues that unit tests with mocks might miss.

## Choreographic Integration

**Effect-Based Choreographies** use effects for communication and state management. Choreographic protocols define distributed coordination while effects handle implementation details.

```rust
choreography! {
    protocol DataSynchronization {
        roles: Initiator, Responder;
        
        Initiator -> Responder: DataRequest(request_id: u64);
        Responder -> Initiator: DataResponse(data: Vec<u8>);
    }
}

pub async fn execute_sync_initiator(
    effects: &AuraEffectSystem,
    responder_id: DeviceId,
    request_id: u64,
) -> Result<Vec<u8>, SyncError> {
    let request = DataRequest { request_id };
    
    effects
        .network_send_message(responder_id, request)
        .await
        .map_err(SyncError::Network)?;
        
    let response: DataResponse = effects
        .network_receive_message()
        .await
        .map_err(SyncError::Network)?;
        
    Ok(response.data)
}
```

Choreographic implementations use effects for network communication and local state access. This enables testing choreographies with mock networks and deterministic behavior.

**Handler Adaptation** bridges choreographic protocols with effect systems. Adapters translate between protocol-specific interfaces and general effect traits.

```rust
pub struct ChoreographyEffectAdapter {
    effects: AuraEffectSystem,
    protocol_context: ProtocolContext,
}

impl ChoreographyEffectAdapter {
    pub async fn send_choreography_message<T: Serialize>(
        &self,
        recipient: DeviceId,
        message: T,
    ) -> Result<(), ChoreographyError> {
        let envelope = ProtocolEnvelope {
            protocol_id: self.protocol_context.protocol_id,
            sender: self.protocol_context.device_id,
            recipient,
            message: serde_json::to_vec(&message)?,
        };
        
        self.effects
            .network_send_envelope(envelope)
            .await
            .map_err(ChoreographyError::Network)
    }
}
```

Effect adapters provide choreography-specific interfaces while delegating to general effect implementations. This enables reusing effect handlers across different protocols.

**Testing Choreographies** validates distributed protocols using deterministic effect handlers. Mock networks enable testing protocol interactions without actual network communication.

```rust
#[tokio::test]
async fn test_data_synchronization_protocol() {
    let initiator_id = DeviceId::new();
    let responder_id = DeviceId::new();
    
    let network = MockNetwork::new();
    let initiator_effects = AuraEffectSystem::with_network(network.clone());
    let responder_effects = AuraEffectSystem::with_network(network.clone());
    
    let test_data = b"synchronized_data".to_vec();
    
    // Run initiator and responder concurrently
    let (initiator_result, responder_result) = tokio::join!(
        execute_sync_initiator(&initiator_effects, responder_id, 123),
        execute_sync_responder(&responder_effects, test_data.clone())
    );
    
    assert_eq!(initiator_result.unwrap(), test_data);
    assert!(responder_result.is_ok());
}
```

Mock networks provide deterministic message delivery for testing choreographic protocols. Tests validate protocol correctness without network complexity.

## Advanced Patterns

**Effect Middleware** adds cross-cutting concerns to effect handlers. Middleware can implement logging, metrics collection, caching, and retry logic.

```rust
pub struct LoggingMiddleware<T> {
    inner: T,
    logger: Logger,
}

#[async_trait]
impl<T: StorageEffects> StorageEffects for LoggingMiddleware<T> {
    async fn store(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
        self.logger.info("Storage operation started", &[("operation", "store"), ("key", key)]);
        
        let result = self.inner.store(key, data).await;
        
        match &result {
            Ok(_) => self.logger.info("Storage operation completed", &[]),
            Err(e) => self.logger.error("Storage operation failed", &[("error", &e.to_string())]),
        }
        
        result
    }
}
```

Middleware wraps effect handlers to add additional behavior without modifying core logic. This pattern enables composition of cross-cutting concerns.

**Effect Caching** improves performance by caching expensive operations. Cache middleware transparently adds caching to any effect handler.

```rust
pub struct CachingStorageHandler<T> {
    inner: T,
    cache: Arc<Mutex<LruCache<String, Vec<u8>>>>,
}

#[async_trait]
impl<T: StorageEffects> StorageEffects for CachingStorageHandler<T> {
    async fn load(&self, key: &str) -> Result<Vec<u8>, StorageError> {
        {
            let mut cache = self.cache.lock().await;
            if let Some(data) = cache.get(key) {
                return Ok(data.clone());
            }
        }
        
        let data = self.inner.load(key).await?;
        
        {
            let mut cache = self.cache.lock().await;
            cache.put(key.to_string(), data.clone());
        }
        
        Ok(data)
    }
}
```

Caching handlers improve performance for frequently accessed data. Cache invalidation can be handled through effect middleware or explicit cache management.

**Dynamic Effect Selection** chooses handlers at runtime based on configuration or environmental conditions. This enables adaptive behavior in different deployment environments.

```rust
pub fn create_storage_handler(config: &StorageConfig) -> Arc<dyn StorageEffects> {
    match config.storage_type {
        StorageType::FileSystem => Arc::new(FileSystemStorageHandler::new(&config.base_path)),
        StorageType::S3 => Arc::new(S3StorageHandler::new(&config.bucket_name)),
        StorageType::Memory => Arc::new(InMemoryStorageHandler::new()),
    }
}
```

Dynamic handler selection enables applications to adapt to different deployment environments without code changes. Configuration drives behavior rather than compile-time decisions.