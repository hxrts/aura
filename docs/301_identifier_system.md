# Identifier System

Aura's identifier system provides type-safe, privacy-preserving identification across all system components. The system ensures context isolation, prevents identifier confusion, and supports both human-readable and cryptographically secure identifiers.

All identifier types use `aura-core` as the single source of truth. The system provides consistent patterns for creation, display, serialization, and testing. Privacy context isolation prevents unauthorized cross-context communication.

See [Web of Trust](200_web_of_trust.md) for relationship identifiers. See [Authentication vs Authorization Architecture](101_authentication_system.md) for identity verification patterns.

---

## Identifier Design Principles

**Type Safety** prevents identifier confusion through distinct Rust types. Each identifier category has a unique type that cannot be accidentally mixed with other identifiers. The type system catches identifier misuse at compile time.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountId(Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(Uuid);

// Compile-time error: cannot assign DeviceId to AccountId variable
fn example_type_safety() {
    let device_id = DeviceId::new();
    let account_id: AccountId = device_id; // ← Compile error!
}
```

Distinct types prevent common bugs where identifiers are passed to wrong functions or stored in wrong fields. Type safety eliminates runtime identifier validation.

**Privacy Context Isolation** enforces communication boundaries between different privacy contexts. Messages cannot flow between contexts without explicit bridge operations. Context isolation prevents privacy leaks through identifier reuse.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MessageContext {
    Relay(RelayId),
    Group(GroupId),
    DKD(DkdContextId),
}

pub struct ContextBridge {
    source_context: MessageContext,
    target_context: MessageContext,
    bridge_policy: BridgePolicy,
}

impl MessageContext {
    pub fn can_communicate_with(&self, other: &MessageContext) -> bool {
        match (self, other) {
            (MessageContext::Relay(a), MessageContext::Relay(b)) => a == b,
            (MessageContext::Group(a), MessageContext::Group(b)) => a == b,
            (MessageContext::DKD(a), MessageContext::DKD(b)) => a == b,
            _ => false, // Cross-context communication requires explicit bridge
        }
    }
}
```

Context isolation ensures that messages intended for one privacy context cannot accidentally leak to another context. Bridge operations provide controlled cross-context communication.

**Deterministic Generation** enables reproducible identifier creation for testing and debugging. Effects-based generation allows mock implementations to produce predictable identifiers while production uses cryptographically secure randomness.

```rust
#[async_trait]
pub trait RandomEffects: Send + Sync {
    async fn random_uuid(&self) -> Uuid;
    async fn random_bytes(&self, length: usize) -> Vec<u8>;
}

impl DeviceId {
    pub async fn generate<R: RandomEffects>(effects: &R) -> Self {
        let uuid = effects.random_uuid().await;
        DeviceId(uuid)
    }
    
    // For testing with deterministic values
    pub fn from_seed(seed: u64) -> Self {
        let mut hasher = DefaultHasher::new();
        hasher.write_u64(seed);
        let hash = hasher.finish();
        
        let uuid = Uuid::from_bytes([
            (hash >> 56) as u8,
            (hash >> 48) as u8,
            (hash >> 40) as u8,
            (hash >> 32) as u8,
            (hash >> 24) as u8,
            (hash >> 16) as u8,
            (hash >> 8) as u8,
            hash as u8,
            0, 0, 0, 0, 0, 0, 0, 0, // Zero padding for UUID
        ]);
        
        DeviceId(uuid)
    }
}
```

Deterministic generation enables testing with predictable identifiers while maintaining production security through proper random generation.

## UUID-Based Identifiers

**System Identifiers** use UUID v4 for distributed unique identification. UUIDs provide sufficient entropy for collision resistance across large distributed systems without coordination.

```rust
impl DeviceId {
    pub fn new() -> Self {
        DeviceId(Uuid::new_v4())
    }
    
    pub fn from_bytes(bytes: [u8; 16]) -> Result<Self, IdentifierError> {
        let uuid = Uuid::from_bytes(bytes);
        Ok(DeviceId(uuid))
    }
    
    pub fn as_bytes(&self) -> [u8; 16] {
        *self.0.as_bytes()
    }
    
    pub fn to_base58(&self) -> String {
        bs58::encode(self.as_bytes()).into_string()
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}
```

System identifiers provide compact binary representation for storage efficiency. Base58 encoding enables human-readable display when needed.

**Session Identifiers** track protocol sessions and temporary communication contexts. Session identifiers include display prefixes for easy identification in logs and debugging.

```rust
impl SessionId {
    pub fn new() -> Self {
        SessionId(Uuid::new_v4())
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "session:{}", self.0.simple())
    }
}

// Usage examples for different session types
pub fn create_authentication_session() -> SessionId {
    SessionId::new() // Displays as "session:550e8400e29b41d4a716446655440000"
}

pub fn create_key_exchange_session() -> SessionId {
    SessionId::new() // Each session gets unique identifier
}
```

Session identifiers enable tracking distributed protocols across multiple participants. Prefixed display formats help identify session types in system logs.

**Event Identifiers** provide unique identification for journal events and operation tracking. Event identifiers enable deterministic ordering and efficient lookups in distributed logs.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(Uuid);

impl EventId {
    pub fn new() -> Self {
        EventId(Uuid::new_v4())
    }
    
    pub fn from_timestamp_and_device(timestamp: u64, device_id: DeviceId) -> Self {
        let mut hasher = DefaultHasher::new();
        hasher.write_u64(timestamp);
        hasher.write(device_id.as_bytes());
        let hash = hasher.finish();
        
        // Create deterministic UUID from hash
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes[0..8].copy_from_slice(&hash.to_le_bytes());
        uuid_bytes[8..16].copy_from_slice(&device_id.as_bytes()[0..8]);
        
        EventId(Uuid::from_bytes(uuid_bytes))
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "event:{}", self.0.simple())
    }
}
```

Event identifiers support both random generation and deterministic creation from timestamp and device. Deterministic creation enables consistent event identification across replicas.

## String-Based Identifiers

**Human-Readable Identifiers** provide meaningful names for user-facing concepts. String-based identifiers support hierarchical naming and application-specific formats.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemberId(String);

impl MemberId {
    pub fn new(name: impl Into<String>) -> Result<Self, IdentifierError> {
        let name = name.into();
        
        // Validate format: alphanumeric with hyphens and underscores
        if name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') &&
           name.len() >= 3 && name.len() <= 64 {
            Ok(MemberId(name))
        } else {
            Err(IdentifierError::InvalidFormat)
        }
    }
    
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MemberId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "member:{}", self.0)
    }
}
```

String-based identifiers include format validation to ensure consistency. Validation rules prevent problematic characters that could cause issues in different contexts.

**Data Identifiers** provide flexible identification for application data with support for encryption and effects-based generation.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataId {
    Plain(String),
    Encrypted { ciphertext: Vec<u8>, nonce: [u8; 12] },
}

impl DataId {
    pub async fn generate_encrypted<E: EncryptionEffects>(
        plaintext: &str,
        encryption_key: &[u8; 32],
        effects: &E,
    ) -> Result<Self, IdentifierError> {
        let nonce = effects.random_nonce().await;
        let ciphertext = effects.encrypt(plaintext.as_bytes(), encryption_key, &nonce).await?;
        
        Ok(DataId::Encrypted { ciphertext, nonce })
    }
    
    pub async fn decrypt<E: EncryptionEffects>(
        &self,
        decryption_key: &[u8; 32],
        effects: &E,
    ) -> Result<String, IdentifierError> {
        match self {
            DataId::Plain(data) => Ok(data.clone()),
            DataId::Encrypted { ciphertext, nonce } => {
                let plaintext = effects.decrypt(ciphertext, decryption_key, nonce).await?;
                String::from_utf8(plaintext).map_err(|_| IdentifierError::InvalidEncoding)
            }
        }
    }
}
```

Encrypted data identifiers protect sensitive information while maintaining identifier functionality. Encryption uses effects-based interfaces for testability.

## Privacy Context Identifiers

**Relay Communication Context** provides pairwise communication channels with cryptographic derivation from participant identities. Relay contexts prevent message confusion between different communication pairs.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RelayId([u8; 32]);

impl RelayId {
    pub fn derive_from_devices(device_a: DeviceId, device_b: DeviceId) -> Self {
        // Deterministic ordering ensures same RelayId regardless of parameter order
        let (first, second) = if device_a.as_bytes() < device_b.as_bytes() {
            (device_a, device_b)
        } else {
            (device_b, device_a)
        };
        
        use aura_core::hash::hasher;
        let mut h = hasher();
        h.update(b"aura.relay_context:");
        h.update(first.as_bytes());
        h.update(second.as_bytes());
        
        RelayId(h.finalize())
    }
    
    pub fn participants(&self) -> Result<(DeviceId, DeviceId), IdentifierError> {
        // For privacy, participants cannot be derived from RelayId
        // This method would require additional context or storage lookup
        Err(IdentifierError::PrivacyConstraint)
    }
}
```

Relay identifiers use deterministic derivation to ensure consistent identification across participants. Privacy constraints prevent reverse-engineering participant identities from relay identifiers.

**Group Communication Context** provides multi-party communication channels with deterministic construction from group membership configuration.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupId([u8; 32]);

impl GroupId {
    pub fn derive_from_members(members: &[DeviceId], group_policy: &GroupPolicy) -> Self {
        let mut sorted_members = members.to_vec();
        sorted_members.sort_by_key(|id| id.as_bytes());
        
        use aura_core::hash::hasher;
        let mut h = hasher();
        h.update(b"aura.group_context:");
        
        for member in &sorted_members {
            h.update(member.as_bytes());
        }
        
        h.update(&bincode::serialize(group_policy).unwrap());
        
        GroupId(h.finalize())
    }
    
    pub fn is_member(&self, device_id: DeviceId, group_config: &GroupConfig) -> bool {
        // Check membership requires access to group configuration
        let derived_id = Self::derive_from_members(&group_config.members, &group_config.policy);
        derived_id == *self && group_config.members.contains(&device_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupPolicy {
    pub threshold: usize,
    pub expiration: Option<u64>,
    pub capabilities: CapabilitySet,
}
```

Group identifiers include policy information in their derivation to ensure policy changes create new group contexts. This prevents confusion between different group configurations.

**Key Derivation Context** provides application-scoped deterministic key derivation with application label and fingerprint combination.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DkdContextId {
    application_label: String,
    fingerprint: [u8; 16],
}

impl DkdContextId {
    pub fn new(application_label: impl Into<String>, context_data: &[u8]) -> Self {
        let application_label = application_label.into();
        
        let mut h = aura_core::hash::hasher();
        h.update(b"aura.dkd_context:");
        h.update(application_label.as_bytes());
        h.update(context_data);
        
        let hash = h.finalize();
        let fingerprint = hash[0..16].try_into().unwrap();
        
        DkdContextId {
            application_label,
            fingerprint,
        }
    }
    
    pub fn application(&self) -> &str {
        &self.application_label
    }
    
    pub fn derive_key<K: KeyDerivationEffects>(
        &self,
        master_key: &[u8; 32],
        key_path: &[u32],
        effects: &K,
    ) -> Result<[u8; 32], IdentifierError> {
        let mut context = Vec::new();
        context.extend_from_slice(self.application_label.as_bytes());
        context.extend_from_slice(&self.fingerprint);
        
        for component in key_path {
            context.extend_from_slice(&component.to_le_bytes());
        }
        
        effects.derive_key(master_key, &context)
            .map_err(IdentifierError::KeyDerivation)
    }
}
```

DKD context identifiers enable application-specific key derivation while maintaining isolation between different applications and contexts.

## Content Addressing Identifiers

**Cryptographic Hashes** provide tamper-evident identification for content and data structures. Blake3 hashing ensures security and performance for content addressing.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hash32([u8; 32]);

impl Hash32 {
    pub fn hash_data(data: &[u8]) -> Self {
        Hash32(aura_core::hash::hash(data))
    }
    
    pub fn hash_structured<T: Serialize>(data: &T) -> Result<Self, IdentifierError> {
        let serialized = bincode::serialize(data)
            .map_err(|_| IdentifierError::SerializationError)?;
        Ok(Self::hash_data(&serialized))
    }
    
    pub fn verify_data(&self, data: &[u8]) -> bool {
        self.0 == aura_core::hash::hash(data)
    }
    
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
    
    pub fn from_hex(hex_str: &str) -> Result<Self, IdentifierError> {
        let bytes = hex::decode(hex_str)
            .map_err(|_| IdentifierError::InvalidEncoding)?;
        
        if bytes.len() != 32 {
            return Err(IdentifierError::InvalidLength);
        }
        
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&bytes);
        Ok(Hash32(hash))
    }
}
```

Cryptographic hashes provide content verification and enable efficient deduplication. Hash-based identifiers prevent content tampering and support content-addressed storage.

**Content Identification** combines hash-based identification with metadata for comprehensive content management.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentId {
    hash: Hash32,
    size: Option<u64>,
    content_type: Option<String>,
}

impl ContentId {
    pub fn from_data(data: &[u8], content_type: Option<String>) -> Self {
        ContentId {
            hash: Hash32::hash_data(data),
            size: Some(data.len() as u64),
            content_type,
        }
    }
    
    pub fn hash_only(hash: Hash32) -> Self {
        ContentId {
            hash,
            size: None,
            content_type: None,
        }
    }
    
    pub fn verify_content(&self, data: &[u8]) -> Result<bool, IdentifierError> {
        // Verify hash
        if !self.hash.verify_data(data) {
            return Ok(false);
        }
        
        // Verify size if available
        if let Some(expected_size) = self.size {
            if data.len() as u64 != expected_size {
                return Ok(false);
            }
        }
        
        Ok(true)
    }
}
```

Content identifiers enable efficient verification of data integrity and support rich metadata for content management applications.

## Usage Examples and Best Practices

**Identifier Creation Patterns** demonstrate proper usage across different scenarios with appropriate error handling and validation.

```rust
// System initialization with proper error handling
pub async fn initialize_device<R: RandomEffects>(
    effects: &R,
    device_name: &str,
) -> Result<(DeviceId, AccountId), InitializationError> {
    let device_id = DeviceId::generate(effects).await;
    let account_id = AccountId::generate(effects).await;
    
    // Store device metadata
    let member_id = MemberId::new(device_name)?;
    
    Ok((device_id, account_id))
}

// Privacy-preserving communication setup
pub fn create_private_channel(
    device_a: DeviceId,
    device_b: DeviceId,
) -> (RelayId, MessageContext) {
    let relay_id = RelayId::derive_from_devices(device_a, device_b);
    let context = MessageContext::Relay(relay_id);
    
    (relay_id, context)
}

// Content addressing with verification
pub async fn store_verified_content<S: StorageEffects>(
    content: &[u8],
    content_type: String,
    storage: &S,
) -> Result<ContentId, StorageError> {
    let content_id = ContentId::from_data(content, Some(content_type));
    
    storage.store_content(content_id.clone(), content).await?;
    
    Ok(content_id)
}
```

Usage patterns demonstrate proper error handling, effects-based generation, and privacy-preserving identifier creation.
