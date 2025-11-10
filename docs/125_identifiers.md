# ID Types

## Summary

This document describes the identifier type system in the Aura codebase, which uses **aura-core** as the single source of truth for all core ID definitions. The system provides consistent patterns for identifier creation, display, serialization, and effects-based generation for deterministic testing.

## Architecture Overview

### Core Principles

The Aura ID system maintains a **single source of truth** in `aura-core` with **consistent patterns** across all ID types. Extension traits enable **effects-based testing** with deterministic generation, while special privacy context types (`RelayId`, `GroupId`, `DkdContextId`) enforce message isolation. **Macro-generated types** reduce boilerplate while maintaining consistency.

## ID Type Categories

### 1. UUID-Based Identifiers (in aura-core/src/identifiers.rs)

These IDs use UUID v4 for random generation and are suitable for unique identification across distributed systems.

#### Core Protocol IDs
**SessionId**, **EventId**, and **OperationId** use UUID v4 with prefixed display formats (`"session-{uuid}"`, `"event-{uuid}"`, `"operation-{uuid}"`). These handle protocol session coordination, journal event sourcing, and cross-subsystem operation tracking respectively.

#### Identity IDs
**DeviceId**, **GuardianId**, and **AccountId** are system identifiers using raw UUID display without prefixes. DeviceId includes special byte conversion methods (`from_bytes([u8; 32])`, `to_bytes()`). These handle device authentication, guardian coordination, and account-level operations respectively.

### 2. String-Based Identifiers (in aura-core/src/identifiers.rs)

String-based IDs provide flexibility for human-readable identifiers and hierarchical naming. **MemberId** and **IndividualId** use prefixed display formats (`"member-{string}"`, `"individual-{string}"`), while **DataId** includes the prefix within its value. IndividualId provides specialized constructors from DeviceId and DKD contexts, while DataId supports both plain and encrypted variants with effects-based generation.

### 3. Privacy Context Identifiers (in aura-core/src/identifiers.rs)

These types implement the privacy partition model from the whole system design, ensuring that messages from different contexts cannot flow into each other without explicit bridges.

#### RelayId (Pairwise RID)
```rust
pub struct RelayId(pub [u8; 32]);
```
- Purpose: X25519-derived pairwise communication context
- Display: `"relay:{hex}"`
- Constructor: `RelayId::from_devices(device_a, device_b)` (deterministic, ordered)
- Used for: Two-party message contexts, unlinkable routing

#### GroupId (Threshold GID)
```rust
pub struct GroupId(pub [u8; 32]);
```
- Purpose: Threshold group communication context
- Display: `"group:{hex}"`
- Constructor: `GroupId::from_threshold_config(members, threshold)` (deterministic)
- Used for: Multi-party threshold protocols, group messaging

#### DkdContextId (Application DKD)
```rust
pub struct DkdContextId {
    pub app_label: String,
    pub fingerprint: [u8; 32],
}
```
- Purpose: Application-scoped deterministic key derivation context
- Display: `"dkd:{app_label}:{hex}"`
- Constructor: `DkdContextId::new(app_label, fingerprint)`
- Used for: Privacy-preserving key derivation, app isolation

#### MessageContext (Unified Enum)
```rust
pub enum MessageContext {
    Relay(RelayId),
    Group(GroupId),
    DkdContext(DkdContextId),
}
```
- Purpose: Unified message context for privacy partitions
- Key method: `is_compatible_with(&MessageContext) -> bool` (only returns true if identical)
- Enforces: **Context isolation invariant** - no cross-context message flow without bridges

### 4. Content Addressing Identifiers (in aura-core/src/content.rs)

#### Hash32
```rust
pub struct Hash32(pub [u8; 32]);
```
- Purpose: 32-byte Blake3 hash wrapper for content identification
- Display: `"hash32:{hex}"`
- Constructors: `from_bytes(&[u8])`, `new([u8; 32])`, `default()`
- Methods: `as_bytes()`, `to_hex()`, Blake3 hashing
- Used for: Foundational content addressing primitive

#### ContentId
```rust
pub struct ContentId {
    pub hash: Hash32,
    pub size: Option<u64>,
}
```
- Purpose: Content identifiers for high-level blobs (files, documents, CRDT state)
- Display: `"content:{hex}"` or `"content:{hex}:{size}"`
- Constructors: `new(Hash32)`, `with_size(Hash32, u64)`, `from_bytes(&[u8])`, `from_value<T: Serialize>(value)`
- Used for: Journal entries, user files, encrypted payloads

#### ChunkId
```rust
pub struct ChunkId {
    hash: Hash32,
    sequence: Option<u32>,
}
```
- Purpose: Storage-layer chunk identification for fixed/variable-size blocks
- Display: `"chunk:{hex}"` or `"chunk:{hex}:{seq}"`
- Constructors: `new(Hash32)`, `with_sequence(Hash32, u32)`, `from_bytes(&[u8])`
- Used for: Chunked upload/download, erasure coding, replication tracking

#### ContentSize
```rust
pub struct ContentSize(pub u64);
```
- Purpose: Content size tracking with human-readable display
- Display: Human-readable format (e.g., "1.5 MB")
- Methods: `bytes()`, `human_readable()`, size bucket classification
- Used for: Storage quota tracking, content filtering

### 5. Relationship & Context Types (in aura-core/src/relationships.rs)

#### RelationshipId
```rust
pub struct RelationshipId(pub [u8; 32]);
```
- Purpose: Identifies relationships between entities (Web of Trust edges)
- Display: `"relationship-{hex}"`
- Constructors: `from_entities(entity1, entity2)`, `random()`
- Used for: WoT graph edges, delegation chains

#### ContextId
```rust
pub struct ContextId(pub String);
```
- Purpose: Operation context identification
- Display: `"context:{string}"`
- Special methods: `hierarchical(parts)`, `parent()`, `is_child_of(parent)`
- Used for: Hierarchical context scoping, operation isolation

### 6. Capability & Session Types

#### CapabilityId (in aura-journal/src/ledger/capability.rs)
```rust
pub struct CapabilityId(pub Uuid);
```
- Purpose: Unique identifier for capability tokens (meet-semilattice elements)
- Display: `"cap-{uuid}"`
- Used for: Capability-based authorization, token tracking
- Note: Defined in journal crate (not core) due to capability system coupling

#### ParticipantId (in aura-core/src/session_epochs.rs)
```rust
pub enum ParticipantId {
    Device(DeviceId),
    Guardian(GuardianId),
}
```
- Purpose: Unified participant identifier for protocols
- Display: `"device-{uuid}"` or `"guardian-{uuid}"`
- Methods: `is_device()`, `is_guardian()`, `as_device()`, `as_guardian()`
- Used for: Protocol participation, session membership

### 7. Numeric Identifiers

#### EventNonce (in aura-core/src/identifiers.rs)
```rust
pub struct EventNonce(pub u64);
```
- Display: `"nonce-{u64}"`
- Method: `next() -> EventNonce` for sequencing

#### Epoch (in aura-core/src/session_epochs.rs)
```rust
pub struct Epoch(pub u64);
```
- Display: `"epoch-{u64}"`
- Methods: `initial()`, `next()`, monotonic sequencing
- Used for: Versioning, logical clocks, session epochs

#### ContentSize (in aura-store/src/content.rs)
```rust
pub struct ContentSize(pub u64);
```
- Display: Human-readable format (e.g., "1.5 MB")
- Method: `human_readable() -> String`
- Used for: Storage quota tracking, blob sizing

## Effects-Based ID Generation

### EffectsLike Trait

Extension traits enable deterministic ID generation for testing and simulation:

```rust
pub trait EffectsLike {
    fn gen_uuid(&self) -> Uuid;
}
```

### Extension Traits

```rust
// For UUID-based IDs
pub trait EventIdExt {
    fn new_with_effects(effects: &impl EffectsLike) -> Self;
}

pub trait DeviceIdExt {
    fn new_with_effects(effects: &impl EffectsLike) -> Self;
    fn from_string_with_effects(id_str: &str, effects: &impl EffectsLike) -> Self;
}

// Similar for GuardianIdExt, AccountIdExt

// For specialized IDs
pub trait IndividualIdExt {
    fn from_device(device_id: &DeviceId) -> Self;
    fn from_dkd_context(context: &str, fingerprint: &[u8; 32]) -> Self;
}
```

### Usage Pattern

```rust
// In tests or simulation
let effects = MockEffects::new(seed);
let device_id = DeviceId::new_with_effects(&effects);
let event_id = EventId::new_with_effects(&effects);

// In production
let device_id = DeviceId::new(); // Uses Uuid::new_v4()
```

## ID Type Generation Macros

The macro system in `aura-core/src/macros.rs` reduces boilerplate while ensuring consistency.

### Available Macros

#### 1. define_uuid_id!

```rust
// With display prefix
define_uuid_id!(SessionId, "session");

// Without prefix (system IDs)
define_uuid_id!(DeviceId);
```

Generates:
- Struct with UUID wrapper
- `new()`, `from_uuid(uuid)`, `uuid()` methods
- `Default` implementation
- `Display` with optional prefix
- `From<Uuid>` and `Into<Uuid>` conversions
- Full serde support

#### 2. define_string_id!

```rust
// With display prefix
define_string_id!(MemberId, "member");

// Without prefix
define_string_id!(ContextId);
```

Generates:
- Struct with String wrapper
- `new(impl Into<String>)`, `as_str()` methods
- `Display` with optional prefix
- `From<String>` and `From<&str>` conversions
- Full serde support

#### 3. define_numeric_id!

```rust
define_numeric_id!(EventNonce, u64, "nonce");
```

Generates:
- Struct with numeric wrapper
- `new(value)`, `value()` methods
- `Display` with prefix
- `From<T>` and `Into<T>` conversions
- Full serde support

## Display Format Conventions

The system uses two main display formats: **prefixed format** for user-facing identifiers (logs, debugging, UI) and **raw format** for system identifiers used in internal operations. Prefixed types include SessionId (`"session-{uuid}"`), EventId, OperationId, MemberId, IndividualId, ChunkId, CapabilityId, RelationshipId, EventNonce, and Epoch. Raw format types include the core system identifiers DeviceId, GuardianId, AccountId (plain UUIDs), plus privacy context types like RelayId (`"relay:{hex}"`), GroupId, and DkdContextId (`"dkd:{app_label}:{hex}"`).

## Integration with Whole System Model

The ID type system implements key aspects of the whole system design:

### Privacy Partition Enforcement

**Context Isolation Invariant**: `MessageContext` types ensure `κ₁ ≠ κ₂` prevents cross-context flow

```rust
impl MessageContext {
    pub fn is_compatible_with(&self, other: &MessageContext) -> bool {
        self == other  // Only identical contexts are compatible
    }
}
```

This enforces the invariant: "No Msg<Ctx1, …> flows into Ctx2 without explicit bridge protocol"

### Deterministic Derivation

IDs support both random and deterministic generation:
- **Random**: `DeviceId::new()` for production
- **Deterministic**: `RelayId::from_devices(a, b)` for privacy contexts
- **Content-addressed**: `ChunkId::from_content(bytes)` for storage

### Session Type Integration

ParticipantId enum enables type-safe protocol coordination:

```rust
match participant {
    ParticipantId::Device(device_id) => {
        // Device-specific protocol logic
    }
    ParticipantId::Guardian(guardian_id) => {
        // Guardian-specific protocol logic
    }
}
```

## Usage Examples

```rust
// Import from aura-core
use aura_core::{
    DeviceId, GuardianId, AccountId,
    SessionId, EventId, OperationId,
    MemberId, IndividualId, ContextId,
    RelayId, GroupId, DkdContextId, MessageContext,
    ParticipantId, Epoch, EventNonce,
    Hash32, ContentId, ChunkId, ContentSize,
};

// Production usage
let device_id = DeviceId::new();
let session_id = SessionId::new();

// Privacy context usage
let relay_id = RelayId::from_devices(&device_a, &device_b);
let context = MessageContext::Relay(relay_id);

// Content addressing usage
let content_data = b"Hello, world!";
let content_id = ContentId::from_bytes(content_data);
let chunk_id = ChunkId::from_bytes(content_data);
let hash = Hash32::from_bytes(content_data);
```

## Complete ID Type Reference

### aura-core/src/identifiers.rs
- SessionId, EventId, OperationId (UUID with prefix)
- DeviceId, GuardianId, AccountId (UUID without prefix)
- MemberId, IndividualId (String with prefix)
- DataId (String, self-prefixed)
- EventNonce (u64 with prefix)
- RelayId, GroupId (byte array, privacy contexts)
- DkdContextId (struct with app_label + fingerprint)
- MessageContext (enum over privacy contexts)

### aura-core/src/session_epochs.rs
- ParticipantId (enum: Device | Guardian)
- Epoch (u64, monotonic versioning)

### aura-core/src/relationships.rs
- RelationshipId (byte array for WoT edges)
- ContextId (String, hierarchical contexts)
- RelationshipType, RelationshipStatus (enums)
- TrustLevel (enum with ordering)

### aura-core/src/content.rs
- Hash32 (32-byte Blake3 hash wrapper)
- ContentId (high-level content with hash + optional size)
- ChunkId (storage-layer chunks with hash + optional sequence)
- ContentSize (u64, human-readable display)

### aura-journal/src/ledger/capability.rs
- CapabilityId (UUID, capability tokens)


## References

### Implementation
- `aura-core/src/identifiers.rs` - Core UUID and String ID definitions
- `aura-core/src/macros.rs` - ID type generation macros
- `aura-core/src/content.rs` - Content addressing (Cid)
- `aura-core/src/relationships.rs` - RelationshipId and ContextId
- `aura-core/src/session_epochs.rs` - ParticipantId and Epoch
- `aura-store/src/content.rs` - Storage layer (ChunkId, ContentSize)
- `aura-journal/src/ledger/capability.rs` - CapabilityId

### Design Documents
- `docs/000_overview.md` - Architecture overview
- `docs/001_theoretical_foundations.md` - Privacy context model and invariants
- `docs/002_system_architecture.md` - Session type integration
