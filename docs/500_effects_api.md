# Effects API

Quick reference for Aura's algebraic effect system interfaces and implementations.

## Core Effect Interfaces

### Storage Effects

File and content storage operations with encryption and access control.

```rust
use aura_core::effects::StorageEffects;

#[async_trait]
pub trait StorageEffects: Send + Sync {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError>;
    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError>;
    async fn remove(&self, key: &str) -> Result<bool, StorageError>;
    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError>;
    async fn exists(&self, key: &str) -> Result<bool, StorageError>;
    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError>;
    async fn retrieve_batch(&self, keys: &[String]) -> Result<HashMap<String, Vec<u8>>, StorageError>;
}
```

Storage provides key-value operations with batch support. The `retrieve` method returns `Option<Vec<u8>>` when a key is not found.

**Standard implementations**:
- `FilesystemStorageHandler` - Persistent filesystem storage
- `MemoryStorageHandler` - Fast in-memory storage for testing

### Network Effects

Message passing and peer communication with encryption and routing.

```rust
use aura_core::effects::NetworkEffects;

#[async_trait] 
pub trait NetworkEffects: Send + Sync {
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError>;
    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError>;
    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError>;
    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError>;
    async fn connected_peers(&self) -> Vec<Uuid>;
    async fn is_peer_connected(&self, peer_id: Uuid) -> bool;
    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError>;
}
```

Network provides both point-to-point messaging and broadcast communication. The `connected_peers` method returns direct values for efficiency.

**Standard implementations**:
- `TcpNetworkHandler` - Real TCP networking for production
- `MockNetworkHandler` - Deterministic mock networking for testing

### Crypto Effects  

Cryptographic operations including signatures, key derivation, and FROST threshold operations.

```rust
use aura_core::effects::CryptoEffects;

#[async_trait]
pub trait CryptoEffects: RandomEffects + Send + Sync {
    // Hash Functions
    async fn hash(&self, data: &[u8]) -> [u8; 32];
    async fn hmac(&self, key: &[u8], data: &[u8]) -> [u8; 32];
    
    // Key Derivation
    async fn derive_key(&self, master_key: &[u8], context: &KeyDerivationContext) -> Result<Vec<u8>, CryptoError>;
    
    // Ed25519 Signatures
    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError>;
    async fn ed25519_sign(&self, message: &[u8], private_key: &[u8]) -> Result<Vec<u8>, CryptoError>;
    async fn ed25519_verify(&self, message: &[u8], signature: &[u8], public_key: &[u8]) -> Result<bool, CryptoError>;
    
    // FROST Threshold Signatures
    async fn frost_generate_keys(&self, threshold: u16, max_signers: u16) -> Result<Vec<Vec<u8>>, CryptoError>;
    async fn frost_sign_share(&self, signing_package: &FrostSigningPackage, key_share: &[u8], nonces: &[u8]) -> Result<Vec<u8>, CryptoError>;
    async fn frost_aggregate_signatures(&self, signing_package: &FrostSigningPackage, signature_shares: &[Vec<u8>]) -> Result<Vec<u8>, CryptoError>;
    
    // Symmetric Encryption
    async fn chacha20_encrypt(&self, plaintext: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Result<Vec<u8>, CryptoError>;
    async fn chacha20_decrypt(&self, ciphertext: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Result<Vec<u8>, CryptoError>;
    
    fn is_simulated(&self) -> bool;
}
```

Crypto provides both single-party operations and building blocks for threshold operations. FROST operations require coordination across multiple devices using `aura-frost`.

**Standard implementations**:
- `RealCryptoHandler` - Real cryptography with Ed25519
- `MockCryptoHandler` - Deterministic mock crypto for testing

### Journal Effects

CRDT journal operations for distributed state management and flow control.

```rust
use aura_core::effects::JournalEffects;

#[async_trait]
pub trait JournalEffects: Send + Sync {
    async fn merge_facts(&self, target: &Journal, delta: &Journal) -> Result<Journal, AuraError>;
    async fn refine_caps(&self, target: &Journal, refinement: &Journal) -> Result<Journal, AuraError>;
    async fn get_journal(&self) -> Result<Journal, AuraError>;
    async fn persist_journal(&self, journal: &Journal) -> Result<(), AuraError>;
    async fn get_flow_budget(&self, context: &ContextId, peer: &DeviceId) -> Result<FlowBudget, AuraError>;
    async fn charge_flow_budget(&self, context: &ContextId, peer: &DeviceId, cost: u32) -> Result<FlowBudget, AuraError>;
}
```

Journal provides CRDT-based state management with semilattice operations for merging facts and refining capabilities. Flow budget operations enable rate limiting and resource management.

**Standard implementations**:
- `MemoryJournalHandler` - Basic in-memory journal for testing

### Time Effects

Time operations enabling deterministic testing and simulation.

```rust
use aura_core::effects::TimeEffects;

#[async_trait]
pub trait TimeEffects: Send + Sync {
    async fn current_epoch(&self) -> u64;
    async fn current_timestamp(&self) -> u64;
    async fn current_timestamp_millis(&self) -> u64;
    async fn sleep_ms(&self, ms: u64);
    async fn sleep_until(&self, epoch: u64);
    async fn delay(&self, duration: Duration);
    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError>;
    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle;
    
    fn is_simulated(&self) -> bool;
    fn resolution_ms(&self) -> u64;
}
```

Time provides multiple timestamp formats and sleep operations. The `yield_until` method enables event-driven coordination.

**Standard implementations**:
- `RealTimeHandler` - System time for production
- `SimulatedTimeHandler` - Controllable time for testing

### Random Effects

Random number generation with deterministic testing support.

```rust
use aura_core::effects::RandomEffects;

#[async_trait]
pub trait RandomEffects: Send + Sync {
    async fn random_bytes(&self, len: usize) -> Vec<u8>;
    async fn random_bytes_32(&self) -> [u8; 32];
    async fn random_u64(&self) -> u64;
    async fn random_range(&self, min: u64, max: u64) -> u64;
}
```

Random provides basic random number generation. All methods return values directly without error handling for simplicity.

**Standard implementations**:
- `RealRandomHandler` - Cryptographically secure random
- `MockRandomHandler` - Deterministic random for testing

### Console Effects

Console output operations for logging and user interaction.

```rust
use aura_core::effects::ConsoleEffects;

#[async_trait]
pub trait ConsoleEffects: Send + Sync {
    async fn log_info(&self, message: &str);
    async fn log_warn(&self, message: &str);
    async fn log_error(&self, message: &str);
    async fn log_debug(&self, message: &str);
}
```

Console provides structured logging capabilities for different message severity levels.

**Standard implementations**:
- `RealConsoleHandler` - System console output
- `MockConsoleHandler` - Captured output for testing

## Effect Composition

### Sealed Supertraits

Define protocol-specific effect interfaces using sealed supertraits:

```rust
/// Protocol effects for data synchronization
pub trait DataSyncEffects: NetworkEffects + StorageEffects + CryptoEffects {}
impl<T> DataSyncEffects for T where T: NetworkEffects + StorageEffects + CryptoEffects {}

pub async fn execute_data_sync<E: DataSyncEffects>(
    effects: &E,
    data: SyncData,
    peers: Vec<aura_core::DeviceId>,
) -> Result<SyncResult, SyncError> {
    let hash = effects.hash(&data.content).await?;
    
    for peer in peers {
        let sync_message = SyncMessage { data: data.clone(), hash };
        let serialized = bincode::serialize(&sync_message)?;
        effects.send_to_peer(peer.into(), serialized).await?;
    }
    
    Ok(SyncResult::Success)
}
```

Sealed supertraits provide clean type signatures and better error messages. They enable protocol-specific extensions while maintaining flexibility.

## Handler Registration

### Stateless Effect System

Unified effect system with stateless handler composition:

```rust
use aura_protocol::AuraEffectSystem;
use aura_protocol::effects::EffectSystemConfig;
use aura_core::DeviceId;

// Production configuration
let device_id = DeviceId::new();
let config = EffectSystemConfig::for_production(device_id)
    .expect("Failed to create production configuration");
let effect_system = AuraEffectSystem::new(config)
    .expect("Failed to initialize effect system");

// All effect operations go through the unified system
let hash = effect_system.hash(b"data").await?;
effect_system.store("key", b"value".to_vec()).await?;
effect_system.send_to_peer(peer_id, message).await?;
```

The stateless effect system eliminates shared mutable state and provides deadlock-free coordination through isolated state services. All handlers are context-free and operate without device-specific state.

### Testing Configuration

Use testing configuration for deterministic tests:

```rust
use aura_protocol::effects::EffectSystemConfig;

// Testing configuration with mock handlers
let device_id = DeviceId::new();
let config = EffectSystemConfig::for_testing(device_id);
let effect_system = AuraEffectSystem::new(config)
    .expect("Failed to initialize test effect system");

// Testing operations are deterministic and isolated
let test_data = b"test data";
let hash1 = effect_system.hash(test_data).await?;
let hash2 = effect_system.hash(test_data).await?;
assert_eq!(hash1, hash2); // Deterministic in testing mode
```

Testing configuration provides mock handlers that eliminate external dependencies and enable deterministic test execution. All state services use in-memory implementations for fast test cycles.

## Common Patterns

### Effect Combination

Combining multiple effects for complex operations:

```rust
pub async fn secure_message_send<N: NetworkEffects, C: CryptoEffects>(
    network: &N,
    crypto: &C,
    recipient: aura_core::DeviceId,
    message: &[u8],
    private_key: &[u8],
) -> Result<(), MessageError> {
    // Sign message
    let signature = crypto.ed25519_sign(message, private_key).await?;
    
    // Create authenticated message
    let auth_message = AuthenticatedMessage {
        payload: message.to_vec(),
        signature,
    };
    
    // Serialize and send
    let serialized = bincode::serialize(&auth_message)?;
    network.send_to_peer(recipient.into(), serialized).await?;
    
    Ok(())
}
```

Effect combination enables complex operations while maintaining clean separation of concerns. Each effect provides a specific capability boundary.

### Error Handling

Standardized error handling across effect types:

```rust
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("File not found: {path}")]
    FileNotFound { path: String },
    
    #[error("Permission denied: {operation}")]
    PermissionDenied { operation: String },
    
    #[error("Encryption failed")]
    EncryptionFailed,
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub async fn store_with_retry<S: StorageEffects>(
    effects: &S,
    key: &str,
    data: Vec<u8>,
    max_retries: usize,
) -> Result<(), StorageError> {
    for attempt in 0..=max_retries {
        match effects.store(key, data.clone()).await {
            Ok(()) => return Ok(()),
            Err(StorageError::PermissionDenied { .. }) => return Err(StorageError::PermissionDenied { 
                operation: format!("store({})", key) 
            }),
            Err(err) if attempt < max_retries => {
                tokio::time::sleep(Duration::from_millis(100 * (attempt + 1) as u64)).await;
                continue;
            }
            Err(err) => return Err(err),
        }
    }
    
    unreachable!()
}
```

Error types provide structured error information. Retry logic handles transient failures while avoiding retry on permanent errors.

See [Core Systems Guide](802_core_systems_guide.md) for effect system architecture patterns. See [Coordination Systems Guide](803_coordination_systems_guide.md) for multi-party effect coordination.