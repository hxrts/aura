# Effect System API Reference

Quick reference for Aura's algebraic effect system interfaces and implementations. Effect traits define capability interfaces while handlers provide concrete implementations for different environments.

## Architecture Overview

**Effect Traits** are defined in `aura-core` - import these when you need interface definitions:
```rust
use aura_core::effects::{StorageEffects, CryptoEffects, NetworkEffects};
```

**Standard Implementations** are provided by `aura-effects` - import these for testing and production:
```rust  
use aura_effects::storage::{FilesystemStorageHandler, MemoryStorageHandler};
use aura_effects::crypto::{RealCryptoHandler, MockCryptoHandler};
```

**Coordination Primitives** are provided by `aura-protocol` for multi-party operations:
```rust
use aura_protocol::handlers::{CompositeHandler, AuraHandlerAdapter};
```

**Runtime Composition** is provided by `aura-agent` for complete applications:
```rust
use aura_agent::AuraAgent;
```

See [Effect System Guide](801_effect_system_guide.md) for detailed patterns. See [CRDT Programming Guide](802_crdt_programming_guide.md) for effect integration.

---

## Core Effect Interfaces

### Storage Effects

**Interface** (defined in `aura-core`): File and content storage operations with encryption and access control.

```rust
// Import trait from aura-core  
use aura_core::effects::StorageEffects;

#[async_trait]
pub trait StorageEffects: Send + Sync {
    async fn store(&self, key: &str, data: &[u8]) -> Result<(), StorageError>;
    async fn load(&self, key: &str) -> Result<Vec<u8>, StorageError>;
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, StorageError>;
    async fn exists(&self, key: &str) -> Result<bool, StorageError>;
}
```

**Standard Implementations** (from `aura-effects`):
```rust
use aura_effects::storage::{FilesystemStorageHandler, MemoryStorageHandler};

// Production: persistent filesystem storage
let storage = FilesystemStorageHandler::new("/data".into())?;

// Testing: fast in-memory storage  
let storage = MemoryStorageHandler::new();
```

**Common Usage**:
```rust
// Store encrypted content
let content_id = ContentId::new();
storage.store(&content_id.to_string(), &encrypted_data).await?;

// Load and verify
let data = storage.load(&content_id.to_string()).await?;
```

### Network Effects

**Interface** (defined in `aura-core`): Message passing and peer communication with encryption and routing.

```rust
// Import trait from aura-core
use aura_core::effects::NetworkEffects;

#[async_trait] 
pub trait NetworkEffects: Send + Sync {
    async fn send_message(&self, peer: PeerId, message: Vec<u8>) -> Result<(), NetworkError>;
    async fn receive_messages(&self) -> Result<Vec<(PeerId, Vec<u8>)>, NetworkError>;
    async fn discover_peers(&self, context: &str) -> Result<Vec<PeerId>, NetworkError>;
    async fn establish_connection(&self, peer: PeerId) -> Result<ConnectionId, NetworkError>;
    async fn close_connection(&self, connection: ConnectionId) -> Result<(), NetworkError>;
}
```

**Standard Implementations** (from `aura-effects`):
```rust
use aura_effects::network::{TcpNetworkHandler, MockNetworkHandler};

// Production: real TCP networking
let network = TcpNetworkHandler::new("0.0.0.0:8080".parse()?)?;

// Testing: deterministic mock networking
let network = MockNetworkHandler::new();
```

**Common Usage**:
```rust
// Send choreography message  
let message = ChoreographyMessage::new(operation);
let serialized = bincode::serialize(&message)?;
network.send_message(target_peer, serialized).await?;
```

### Crypto Effects  

**Interface** (defined in `aura-core`): Cryptographic operations including single-party signatures and key derivation.

```rust
// Import trait from aura-core
use aura_core::effects::CryptoEffects;

#[async_trait]
pub trait CryptoEffects: Send + Sync {
    async fn sign(&self, data: &[u8], key_id: KeyId) -> Result<Signature, CryptoError>;
    async fn verify(&self, data: &[u8], signature: &Signature, public_key: &PublicKey) -> Result<bool, CryptoError>;
    async fn derive_key(&self, master_key: &[u8; 32], path: &[u32]) -> Result<[u8; 32], CryptoError>;
    async fn encrypt(&self, data: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Result<Vec<u8>, CryptoError>;
    async fn decrypt(&self, ciphertext: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Result<Vec<u8>, CryptoError>;
    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32];
}
```

**Standard Implementations** (from `aura-effects`):
```rust
use aura_effects::crypto::{RealCryptoHandler, MockCryptoHandler};

// Production: real cryptography with Ed25519
let crypto = RealCryptoHandler::new();

// Testing: deterministic mock crypto  
let crypto = MockCryptoHandler::new();
```

**Multi-party Coordination** (FROST threshold signatures require `aura-protocol` or `aura-frost`):
```rust
// FROST threshold signatures need coordination - use aura-frost
use aura_frost::execute_threshold_ceremony;

// Not available as single-party effect - requires choreographic coordination
```

**Common Usage**:
```rust
// Single-party Ed25519 signature
let signature = crypto.sign(&message_hash, &key_id).await?;

// Key derivation (DKD)
let derived_key = crypto.derive_key(&master_key, &[0, 1, 2]).await?;
```

### Journal Effects

**Interface** (defined in `aura-core`): CRDT journal operations for distributed state management.

```rust
// Import trait from aura-core
use aura_core::effects::JournalEffects;

#[async_trait]
pub trait JournalEffects: Send + Sync {
    async fn append_fact(&self, fact: Fact) -> Result<FactId, JournalError>;
    async fn get_facts(&self, query: &FactQuery) -> Result<Vec<Fact>, JournalError>;
    async fn create_intent(&self, operation: Operation) -> Result<IntentId, JournalError>;
    async fn support_intent(&self, intent_id: IntentId, supporter: DeviceId) -> Result<bool, JournalError>;
    async fn commit_intent(&self, intent_id: IntentId) -> Result<FactId, JournalError>;
    async fn get_current_state(&self) -> Result<JournalState, JournalError>;
}
```

**Standard Implementations** (from `aura-effects`):
```rust
use aura_effects::journal::MemoryJournalHandler;

// Basic in-memory journal for testing and simple use cases
let journal = MemoryJournalHandler::new();
```

**Note**: Most journal operations require multi-party coordination. Use `aura-agent` for complete journal functionality with CRDT synchronization.

**Common Usage**:
```rust
// Record device operation
let fact = Fact::device_operation(device_id, operation, timestamp);
let fact_id = journal.append_fact(fact).await?;
```

### Time Effects

**Interface**: Time operations enabling deterministic testing and simulation.

```rust
#[async_trait]
pub trait TimeEffects: Send + Sync {
    async fn current_timestamp(&self) -> u64;
    async fn sleep(&self, duration: Duration);
    async fn timeout<F, T>(&self, duration: Duration, future: F) -> Result<T, TimeoutError>
    where
        F: Future<Output = T> + Send,
        T: Send;
    async fn schedule(&self, delay: Duration, callback: Box<dyn Fn() + Send>) -> Result<ScheduleId, TimeError>;
}
```

**Common Usage**:
```rust
// Operation with timeout
let result = effects.timeout(
    Duration::from_secs(30),
    expensive_operation()
).await?;
```

### Random Effects

**Interface**: Random number generation with deterministic testing support.

```rust
#[async_trait]
pub trait RandomEffects: Send + Sync {
    async fn random_bytes(&self, length: usize) -> Vec<u8>;
    async fn random_uuid(&self) -> Uuid;
    async fn random_u64(&self) -> u64;
    async fn random_nonce(&self) -> [u8; 12];
    async fn random_element<T>(&self, collection: &[T]) -> Option<&T>;
}
```

**Common Usage**:
```rust
// Generate secure identifier
let device_id = DeviceId(effects.random_uuid().await);

// Create encryption nonce  
let nonce = effects.random_nonce().await;
```

## Handler Implementations

### Real Handlers

**Production implementations** using actual system resources.

```rust
pub struct RealStorageHandler {
    storage_path: PathBuf,
    encryption_key: [u8; 32],
}

impl RealStorageHandler {
    pub fn new(storage_path: PathBuf, encryption_key: [u8; 32]) -> Self {
        Self { storage_path, encryption_key }
    }
}

#[async_trait]
impl StorageEffects for RealStorageHandler {
    async fn store(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
        let file_path = self.storage_path.join(key);
        let encrypted_data = encrypt_data(data, &self.encryption_key)?;
        tokio::fs::write(file_path, encrypted_data).await?;
        Ok(())
    }
    
    // ... other methods
}
```

### Mock Handlers

**Testing implementations** with controlled behavior.

```rust
pub struct MockStorageHandler {
    storage: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    fail_operations: Arc<RwLock<HashSet<String>>>,
}

impl MockStorageHandler {
    pub fn new() -> Self {
        Self {
            storage: Arc::new(RwLock::new(HashMap::new())),
            fail_operations: Arc::new(RwLock::new(HashSet::new())),
        }
    }
    
    pub fn set_failure(&self, key: &str) {
        self.fail_operations.write().unwrap().insert(key.to_string());
    }
}

#[async_trait]
impl StorageEffects for MockStorageHandler {
    async fn store(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
        if self.fail_operations.read().unwrap().contains(key) {
            return Err(StorageError::OperationFailed);
        }
        
        self.storage.write().unwrap().insert(key.to_string(), data.to_vec());
        Ok(())
    }
    
    // ... other methods
}
```

### Simulation Handlers

**Simulation implementations** with controllable properties.

```rust
pub struct SimulationNetworkHandler {
    message_delay: Duration,
    packet_loss_rate: f64,
    partition_groups: Vec<HashSet<PeerId>>,
}

impl SimulationNetworkHandler {
    pub fn with_properties(delay: Duration, loss_rate: f64) -> Self {
        Self {
            message_delay: delay,
            packet_loss_rate: loss_rate,
            partition_groups: vec![],
        }
    }
    
    pub fn create_partition(&mut self, group_a: HashSet<PeerId>, group_b: HashSet<PeerId>) {
        self.partition_groups = vec![group_a, group_b];
    }
}
```

## Effect System Composition

### Handler Registry

**Central registry** for effect handler management and composition.

```rust
pub struct EffectRegistry {
    storage_handler: Arc<dyn StorageEffects>,
    network_handler: Arc<dyn NetworkEffects>, 
    crypto_handler: Arc<dyn CryptoEffects>,
    journal_handler: Arc<dyn JournalEffects>,
    time_handler: Arc<dyn TimeEffects>,
    random_handler: Arc<dyn RandomEffects>,
}

impl EffectRegistry {
    pub fn new() -> Self {
        Self {
            storage_handler: Arc::new(MockStorageHandler::new()),
            network_handler: Arc::new(MockNetworkHandler::new()),
            crypto_handler: Arc::new(MockCryptoHandler::new()),
            journal_handler: Arc::new(MockJournalHandler::new()),
            time_handler: Arc::new(MockTimeHandler::new()),
            random_handler: Arc::new(MockRandomHandler::new()),
        }
    }
    
    pub fn with_real_handlers(config: &Config) -> Self {
        Self {
            storage_handler: Arc::new(RealStorageHandler::new(&config.storage_path, config.encryption_key)),
            network_handler: Arc::new(RealNetworkHandler::new(&config.network_config)),
            crypto_handler: Arc::new(RealCryptoHandler::new()),
            journal_handler: Arc::new(RealJournalHandler::new(&config.journal_config)),
            time_handler: Arc::new(RealTimeHandler::new()),
            random_handler: Arc::new(RealRandomHandler::new()),
        }
    }
}
```

### Effect Injection

**Dependency injection** patterns for effect system integration.

```rust
pub struct DeviceAgent {
    device_id: DeviceId,
    effects: EffectRegistry,
}

impl DeviceAgent {
    pub fn new(device_id: DeviceId, effects: EffectRegistry) -> Self {
        Self { device_id, effects }
    }
    
    pub async fn register_device(&self) -> Result<(), AgentError> {
        // Use crypto effects for key generation
        let device_key = self.effects.crypto_handler.generate_keypair().await?;
        
        // Use journal effects to record registration
        let registration_fact = Fact::device_registration(
            self.device_id,
            device_key.public_key,
            self.effects.time_handler.current_timestamp().await,
        );
        
        self.effects.journal_handler.append_fact(registration_fact).await?;
        
        Ok(())
    }
}
```

## Testing Patterns

### Effect Mocking

**Mock effect configuration** for unit tests.

```rust
#[tokio::test]
async fn test_device_registration() {
    let mut mock_registry = EffectRegistry::new();
    
    // Configure mock crypto handler
    let mock_crypto = MockCryptoHandler::new();
    mock_crypto.set_keypair_result(Ok(test_keypair()));
    mock_registry.crypto_handler = Arc::new(mock_crypto);
    
    // Configure mock journal handler
    let mock_journal = MockJournalHandler::new();
    mock_journal.expect_append_fact().returning(|_| Ok(FactId::new()));
    mock_registry.journal_handler = Arc::new(mock_journal);
    
    // Test device registration
    let device = DeviceAgent::new(DeviceId::new(), mock_registry);
    let result = device.register_device().await;
    
    assert!(result.is_ok());
}
```

### Effect Simulation

**Controlled simulation** for integration testing.

```rust
#[tokio::test] 
async fn test_network_partition_recovery() {
    let mut sim_registry = EffectRegistry::simulation();
    
    // Create network partition
    let mut network_handler = SimulationNetworkHandler::new();
    network_handler.create_partition(
        hashset![peer_1, peer_2],
        hashset![peer_3, peer_4],
    );
    sim_registry.network_handler = Arc::new(network_handler);
    
    // Test choreography under partition
    let choreography = TestChoreography::new(sim_registry);
    let result = choreography.execute_with_partition().await;
    
    // Verify graceful handling
    assert!(matches!(result, Err(ChoreographyError::NetworkPartition)));
}
```

### Effect Injection Testing

**Testing with different effect combinations** for comprehensive coverage.

```rust
pub fn create_test_registry(scenario: TestScenario) -> EffectRegistry {
    match scenario {
        TestScenario::AllMock => EffectRegistry::new(),
        TestScenario::RealCrypto => {
            let mut registry = EffectRegistry::new();
            registry.crypto_handler = Arc::new(RealCryptoHandler::new());
            registry
        }
        TestScenario::SimulatedNetwork => {
            let mut registry = EffectRegistry::new();
            registry.network_handler = Arc::new(SimulationNetworkHandler::new());
            registry
        }
        TestScenario::Production => EffectRegistry::production(),
    }
}
```

## Error Handling

### Effect Error Types

**Standardized error handling** across all effect types.

```rust
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("File not found: {path}")]
    FileNotFound { path: String },
    
    #[error("Permission denied: {operation}")]
    PermissionDenied { operation: String },
    
    #[error("Encryption failed: {reason}")]
    EncryptionFailed { reason: String },
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    #[error("Connection failed to peer: {peer_id}")]
    ConnectionFailed { peer_id: PeerId },
    
    #[error("Message too large: {size} bytes")]
    MessageTooLarge { size: usize },
    
    #[error("Network partition detected")]
    NetworkPartition,
    
    #[error("Timeout after {duration:?}")]
    Timeout { duration: Duration },
}
```

### Error Propagation

**Effect error handling** in application code.

```rust
pub async fn store_with_retry<S: StorageEffects>(
    effects: &S,
    key: &str,
    data: &[u8],
    max_retries: usize,
) -> Result<(), StorageError> {
    let mut attempts = 0;
    
    loop {
        match effects.store(key, data).await {
            Ok(()) => return Ok(()),
            Err(StorageError::PermissionDenied { .. }) => {
                // Don't retry permission errors
                return Err(StorageError::PermissionDenied { 
                    operation: format!("store({})", key) 
                });
            }
            Err(err) if attempts < max_retries => {
                attempts += 1;
                tokio::time::sleep(Duration::from_millis(100 * attempts as u64)).await;
            }
            Err(err) => return Err(err),
        }
    }
}
```

## Common Patterns

### Effect Composition

**Combining multiple effects** for complex operations.

```rust
pub async fn secure_message_send<N: NetworkEffects, C: CryptoEffects>(
    network: &N,
    crypto: &C,
    recipient: PeerId,
    message: &[u8],
    sender_key: KeyId,
) -> Result<(), MessageError> {
    // Sign message
    let signature = crypto.sign(message, sender_key).await?;
    
    // Create authenticated message
    let auth_message = AuthenticatedMessage {
        payload: message.to_vec(),
        signature,
        sender_key_id: sender_key,
    };
    
    // Serialize and send
    let serialized = bincode::serialize(&auth_message)?;
    network.send_message(recipient, serialized).await?;
    
    Ok(())
}
```

### Effect Caching

**Caching layer** for expensive effect operations.

```rust
pub struct CachedStorageHandler<S: StorageEffects> {
    inner: S,
    cache: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

#[async_trait]
impl<S: StorageEffects> StorageEffects for CachedStorageHandler<S> {
    async fn load(&self, key: &str) -> Result<Vec<u8>, StorageError> {
        // Check cache first
        if let Some(data) = self.cache.read().unwrap().get(key) {
            return Ok(data.clone());
        }
        
        // Load from storage and cache
        let data = self.inner.load(key).await?;
        self.cache.write().unwrap().insert(key.to_string(), data.clone());
        
        Ok(data)
    }
    
    // ... other methods
}
```