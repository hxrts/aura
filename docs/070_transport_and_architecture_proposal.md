# 070 · Transport Layer & Architecture Enhancement Proposal

**Date:** October 21, 2025  
**Status:** Proposal  
**Target:** Phase 1 Completion  
**Inspiration:** Causality project architecture review

---

## Executive Summary

This proposal addresses the transport layer implementation challenge and overall architectural clarity by adopting three key principles from the Causality project:

1. **Computation-Communication Symmetry**: Unify local and distributed operations
2. **Automatic Protocol Derivation**: Generate protocols from operation definitions
3. **Layered Architecture Clarity**: Explicit compilation path from high-level APIs to low-level operations

These changes will unblock Phase 1 MVP completion by making the transport layer implementation tractable and providing clear architectural boundaries for testing and optimization.

---

## Problem Statement

### Current Blockers

1. **Transport Layer is Stubbed**
   - `crates/transport/` contains only type definitions
   - Unclear how to implement CRDT sync, storage replication, guardian coordination
   - Manual protocol specification is error-prone and time-consuming

2. **Separate Code Paths for Local vs. Distributed**
   - DKD: Different code for single-device vs. threshold derivation
   - Storage: Different logic for local chunks vs. remote replicas
   - CRDT: Separate paths for local updates vs. remote merge
   - Result: Code duplication, difficult testing, unclear abstractions

3. **Unclear Architectural Layers**
   - Flat crate structure without clear dependencies
   - No explicit compilation story from high-level to low-level
   - Difficult to know where to add new functionality
   - Hard to test layers independently

### Impact

These issues prevent completing Phase 1:
- Cannot demonstrate end-to-end functionality without working transport
- Testing is fragmented across local/distributed modes
- New developers struggle to understand the architecture
- Optimization opportunities are unclear

---

## Proposed Solution

### Principle 1: Computation-Communication Symmetry

**Core Idea:** Computation and communication are unified transformations that differ only by location.

#### Mathematical Foundation

```
Transform: (A @ Location₁) → (B @ Location₂)

Where:
- Local computation: Location₁ = Location₂
- Remote communication: Location₁ ≠ Location₂
- Distributed coordination: Multiple locations involved
```

#### Unified Transform Model

```rust
/// Unified transform that works for local and distributed operations
pub trait Transform {
    type Input;
    type Output;
    type Error;
    
    fn apply(&self, input: Self::Input, location: TransformLocation) 
        -> Result<Self::Output, Self::Error>;
}

/// Location parameter determines execution mode
pub enum TransformLocation {
    /// Execute locally (direct function call)
    Local,
    
    /// Execute on single remote device
    Device(DeviceId),
    
    /// Execute with threshold coordination
    Threshold {
        participants: Vec<DeviceId>,
        threshold: u16,
    },
    
    /// Execute across peer network
    Peer(PeerId),
    
    /// Execute with replication
    Replicated(Vec<Location>),
}
```

#### Implementation Strategy

**Phase 1a: Define Unified APIs (Week 1)**

1. Create `crates/transform/` with core traits:

```rust
// crates/transform/src/lib.rs

/// Core transform trait implemented by all operations
pub trait Transform {
    type Input;
    type Output;
    type Error;
    
    /// Apply transform at specified location
    fn apply(&self, input: Self::Input, location: TransformLocation) 
        -> Result<Self::Output, Self::Error>;
    
    /// Get input/output types for protocol derivation
    fn input_type(&self) -> TypeDescriptor;
    fn output_type(&self) -> TypeDescriptor;
}

/// Location where transform executes
pub enum TransformLocation {
    Local,
    Device(DeviceId),
    Threshold { participants: Vec<DeviceId>, threshold: u16 },
    Peer(PeerId),
    Replicated(Vec<Location>),
}

/// Nested location type for complex scenarios
pub enum Location {
    Local,
    Remote(String),
    Distributed(Vec<Location>),
}
```

2. Refactor existing operations to implement `Transform`:

```rust
// Example: DKD Transform
pub struct DkdTransform {
    capsule: ContextCapsule,
}

impl Transform for DkdTransform {
    type Input = ();  // Capsule already in transform
    type Output = DerivedIdentity;
    type Error = AgentError;
    
    fn apply(&self, _input: (), location: TransformLocation) 
        -> Result<DerivedIdentity, AgentError> 
    {
        match location {
            TransformLocation::Local => {
                // Single-device derivation
                self.derive_local()
            }
            TransformLocation::Threshold { participants, threshold } => {
                // Threshold derivation with MPC
                self.derive_threshold(participants, threshold)
            }
            _ => Err(AgentError::UnsupportedLocation),
        }
    }
    
    fn input_type(&self) -> TypeDescriptor {
        TypeDescriptor::Unit
    }
    
    fn output_type(&self) -> TypeDescriptor {
        TypeDescriptor::DerivedIdentity
    }
}
```

**Phase 1b: Update Core Operations (Week 1-2)**

Refactor three key areas to use unified transform model:

1. **DKD Operations:**

```rust
// Before (separate APIs)
impl DeviceAgent {
    fn derive_local(&self, capsule: &ContextCapsule) -> Result<DerivedIdentity>;
    fn derive_threshold(&self, capsule: &ContextCapsule, devices: Vec<DeviceId>) 
        -> Result<DerivedIdentity>;
}

// After (unified API)
impl DeviceAgent {
    pub async fn derive_context_identity(
        &self,
        capsule: &ContextCapsule,
        location: TransformLocation,
    ) -> Result<DerivedIdentity> {
        let transform = DkdTransform { capsule: capsule.clone() };
        transform.apply((), location).await
    }
}

// Usage examples
let local = agent.derive_context_identity(
    &capsule, 
    TransformLocation::Local
).await?;

let threshold = agent.derive_context_identity(
    &capsule,
    TransformLocation::Threshold {
        participants: vec![device1, device2, device3],
        threshold: 2,
    }
).await?;
```

2. **Storage Operations:**

```rust
// Before (separate APIs)
impl StorageClient {
    fn store_local(&self, chunk: &[u8]) -> Result<Cid>;
    fn replicate(&self, chunk: &[u8], peers: Vec<PeerId>) -> Result<Vec<ReplicaTag>>;
}

// After (unified API)
impl StorageClient {
    pub async fn store_chunk(
        &self,
        chunk: &[u8],
        location: TransformLocation,
    ) -> Result<StorageResult> {
        let transform = StoreChunkTransform { data: chunk.to_vec() };
        transform.apply((), location).await
    }
}

// Usage examples
let local_cid = storage.store_chunk(
    chunk,
    TransformLocation::Local
).await?;

let replicated = storage.store_chunk(
    chunk,
    TransformLocation::Replicated(vec![peer1, peer2, peer3])
).await?;
```

3. **CRDT Operations:**

```rust
// Before (separate logic)
impl AccountLedger {
    fn apply_local(&mut self, event: Event) -> Result<()>;
    fn merge_remote(&mut self, peer: PeerId, state: AccountState) -> Result<()>;
}

// After (unified API)
impl AccountLedger {
    pub async fn apply_transform(
        &mut self,
        operation: CrdtOperation,
        location: TransformLocation,
    ) -> Result<()> {
        let transform = CrdtTransform { operation };
        transform.apply(&self.state, location).await
    }
}

// Usage examples
ledger.apply_transform(
    CrdtOperation::AddDevice(device),
    TransformLocation::Local
).await?;

ledger.apply_transform(
    CrdtOperation::MergeState(remote_state),
    TransformLocation::Peer(peer_id)
).await?;
```

**Benefits:**

- [x] Single API for all location modes
- [x] Easy to test (mock location parameter)
- [x] Natural composition across locations
- [x] Clear extension point for new location types

---

### Principle 2: Automatic Protocol Derivation

**Core Idea:** Communication protocols are generated automatically from operation type signatures, not manually specified.

#### Operation Definition Pattern

Define operations declaratively with typed inputs/outputs:

```rust
/// Declarative operation definition
pub struct Operation<I, O> {
    pub name: String,
    pub input_type: TypeDescriptor,
    pub output_type: TypeDescriptor,
    pub handler: Box<dyn Fn(I) -> Result<O>>,
}

/// Type descriptors for protocol derivation
pub enum TypeDescriptor {
    Unit,
    Bool,
    Int,
    String,
    Bytes,
    Struct(Vec<Field>),
    Enum(Vec<Variant>),
    Vec(Box<TypeDescriptor>),
    Option(Box<TypeDescriptor>),
}
```

#### Protocol Derivation Algorithm

```rust
/// Automatically derive session protocol from operation
pub trait DeriveProtocol {
    fn derive_protocol(&self) -> SessionType;
}

impl<I, O> DeriveProtocol for Operation<I, O> 
where
    I: TypeDescriptor,
    O: TypeDescriptor,
{
    fn derive_protocol(&self) -> SessionType {
        // Request-response pattern
        SessionType::Send(
            Box::new(self.input_type.clone()),
            Box::new(SessionType::Receive(
                Box::new(self.output_type.clone()),
                Box::new(SessionType::End)
            ))
        )
    }
}

/// Session type representation
pub enum SessionType {
    Send(Box<TypeDescriptor>, Box<SessionType>),
    Receive(Box<TypeDescriptor>, Box<SessionType>),
    Choice(Vec<(String, SessionType)>),
    End,
}
```

#### Implementation Strategy

**Phase 2a: Define Operation Types (Week 2)**

Create declarative operation definitions for three key subsystems:

1. **CRDT Operations:**

```rust
// crates/ledger/src/protocol.rs

/// Declarative CRDT operations
pub enum CrdtOperation {
    FetchState {
        since_version: u64,
    },
    
    ProposeEvent {
        event: Event,
        signature: ThresholdSignature,
    },
    
    MergeState {
        remote_state: AccountState,
    },
    
    QueryDevices {
        filter: Option<DeviceFilter>,
    },
}

impl TypedOperation for CrdtOperation {
    fn input_type(&self) -> TypeDescriptor {
        match self {
            CrdtOperation::FetchState { .. } => {
                TypeDescriptor::Struct(vec![
                    Field::new("since_version", TypeDescriptor::Int),
                ])
            }
            CrdtOperation::ProposeEvent { .. } => {
                TypeDescriptor::Struct(vec![
                    Field::new("event", TypeDescriptor::Event),
                    Field::new("signature", TypeDescriptor::Bytes),
                ])
            }
            // ... etc
        }
    }
    
    fn output_type(&self) -> TypeDescriptor {
        match self {
            CrdtOperation::FetchState { .. } => {
                TypeDescriptor::Vec(Box::new(TypeDescriptor::Event))
            }
            CrdtOperation::ProposeEvent { .. } => {
                TypeDescriptor::Bool  // Accepted or rejected
            }
            // ... etc
        }
    }
}
```

**Derived Protocol Example:**

```rust
// FetchState operation automatically derives:
// Send(FetchStateRequest { since_version: u64 }) →
// Receive(StateResponse { events: Vec<Event> }) →
// End

let protocol = CrdtOperation::FetchState { since_version: 42 }
    .derive_protocol();

assert_eq!(protocol, SessionType::Send(
    Box::new(TypeDescriptor::Struct(vec![
        Field::new("since_version", TypeDescriptor::Int)
    ])),
    Box::new(SessionType::Receive(
        Box::new(TypeDescriptor::Vec(Box::new(TypeDescriptor::Event))),
        Box::new(SessionType::End)
    ))
));
```

2. **Storage Operations:**

```rust
// crates/storage/src/protocol.rs

/// Declarative storage operations
pub enum ReplicationOperation {
    PushChunk {
        chunk_cid: Cid,
        chunk_data: Vec<u8>,
        metadata: ChunkMetadata,
    },
    
    FetchChunk {
        chunk_cid: Cid,
        priority: Priority,
    },
    
    VerifyPresence {
        chunk_cid: Cid,
        challenge: [u8; 32],
    },
    
    ListChunks {
        filter: ChunkFilter,
    },
}

impl TypedOperation for ReplicationOperation {
    fn input_type(&self) -> TypeDescriptor {
        match self {
            ReplicationOperation::PushChunk { .. } => {
                TypeDescriptor::Struct(vec![
                    Field::new("chunk_cid", TypeDescriptor::Bytes),
                    Field::new("chunk_data", TypeDescriptor::Bytes),
                    Field::new("metadata", TypeDescriptor::ChunkMetadata),
                ])
            }
            // ... etc
        }
    }
    
    fn output_type(&self) -> TypeDescriptor {
        match self {
            ReplicationOperation::PushChunk { .. } => {
                TypeDescriptor::Struct(vec![
                    Field::new("replica_tag", TypeDescriptor::Bytes),
                    Field::new("stored_at", TypeDescriptor::Int),
                ])
            }
            // ... etc
        }
    }
}
```

**Derived Protocol Example:**

```rust
// PushChunk operation automatically derives:
// Send(ChunkData { cid, data, metadata }) →
// Receive(ReplicaConfirmation { replica_tag, stored_at }) →
// End

let protocol = ReplicationOperation::PushChunk {
    chunk_cid: cid,
    chunk_data: data,
    metadata: metadata,
}.derive_protocol();
```

3. **Guardian Operations:**

```rust
// crates/agent/src/guardian_protocol.rs

/// Declarative guardian operations
pub enum GuardianOperation {
    RequestApproval {
        recovery_request: RecoveryRequest,
        cooldown_duration: Duration,
    },
    
    SubmitApproval {
        request_id: RequestId,
        approval: RecoveryApproval,
        guardian_signature: Signature,
    },
    
    DistributeShare {
        guardian_id: GuardianId,
        share_envelope: RecoveryShareEnvelope,
    },
    
    QueryRecoveryStatus {
        request_id: RequestId,
    },
}

impl TypedOperation for GuardianOperation {
    fn input_type(&self) -> TypeDescriptor {
        match self {
            GuardianOperation::RequestApproval { .. } => {
                TypeDescriptor::Struct(vec![
                    Field::new("recovery_request", TypeDescriptor::RecoveryRequest),
                    Field::new("cooldown_duration", TypeDescriptor::Int),
                ])
            }
            // ... etc
        }
    }
    
    fn output_type(&self) -> TypeDescriptor {
        match self {
            GuardianOperation::RequestApproval { .. } => {
                TypeDescriptor::Option(Box::new(TypeDescriptor::ApprovalToken))
            }
            // ... etc
        }
    }
}
```

**Derived Protocol Example:**

```rust
// RequestApproval operation automatically derives:
// Send(RecoveryRequest { user_id, device_info, cooldown }) →
// Receive(Option<ApprovalToken>) →
// End

let protocol = GuardianOperation::RequestApproval {
    recovery_request: request,
    cooldown_duration: Duration::hours(48),
}.derive_protocol();
```

**Phase 2b: Protocol Optimization (Week 2-3)**

Implement batching and optimization:

```rust
// crates/transport/src/optimizer.rs

/// Optimize multiple operations into efficient protocols
pub struct ProtocolOptimizer;

impl ProtocolOptimizer {
    /// Batch multiple operations to same location
    pub fn batch_operations(ops: Vec<Box<dyn TypedOperation>>) -> SessionType {
        // Analyze access patterns
        let same_target = self.group_by_target(&ops);
        
        if same_target.len() == 1 {
            // All operations target same location → batch
            self.create_batch_protocol(ops)
        } else {
            // Multiple targets → parallel protocols
            self.create_parallel_protocols(same_target)
        }
    }
    
    fn create_batch_protocol(&self, ops: Vec<Box<dyn TypedOperation>>) -> SessionType {
        // Send batch request
        let batch_input = TypeDescriptor::Vec(
            Box::new(TypeDescriptor::Variant(
                ops.iter().map(|op| op.input_type()).collect()
            ))
        );
        
        // Receive batch response
        let batch_output = TypeDescriptor::Vec(
            Box::new(TypeDescriptor::Variant(
                ops.iter().map(|op| op.output_type()).collect()
            ))
        );
        
        SessionType::Send(
            Box::new(batch_input),
            Box::new(SessionType::Receive(
                Box::new(batch_output),
                Box::new(SessionType::End)
            ))
        )
    }
}
```

**Example Optimization:**

```rust
// Multiple field accesses...
let ops = vec![
    CrdtOperation::QueryDevices { filter: None },
    CrdtOperation::FetchState { since_version: 0 },
    CrdtOperation::QueryGuardians { filter: None },
];

// ...optimized into single batch protocol:
// Send(BatchRequest([QueryDevices, FetchState, QueryGuardians])) →
// Receive(BatchResponse([DeviceList, StateSnapshot, GuardianList])) →
// End

let optimized = ProtocolOptimizer::batch_operations(ops);
```

**Benefits:**

- [x] No manual protocol specification
- [x] Type-safe protocol generation
- [x] Automatic optimization
- [x] Protocol evolution follows operation changes

---

### Principle 3: Layered Architecture Clarity

**Core Idea:** Explicit three-layer architecture with clear compilation path from high-level APIs to low-level operations.

#### Proposed Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ Layer 3: Application APIs                                   │
│  - DeviceAgent (derive_identity, add_guardian, recover)    │
│  - StorageClient (store_encrypted, fetch_encrypted)        │
│  - What developers directly interact with                  │
└──────────────▲──────────────────────────────────────────────┘
               │ compiles to
┌──────────────┴──────────────────────────────────────────────┐
│ Layer 2: Orchestration & Coordination                       │
│  - Threshold coordination (DKG, signing, resharing)        │
│  - Policy evaluation (Cedar → Biscuit)                     │
│  - CRDT coordination (propose, validate, merge)            │
│  - Protocol derivation                                     │
└──────────────▲──────────────────────────────────────────────┘
               │ compiles to
┌──────────────┴──────────────────────────────────────────────┐
│ Layer 1: Execution Layer                                    │
│  - Cryptographic operations (sign, verify, encrypt)        │
│  - Transport operations (send, receive)                    │
│  - Storage operations (write, read)                        │
│  - Atomic primitives                                       │
└──────────────▲──────────────────────────────────────────────┘
               │
┌──────────────┴──────────────────────────────────────────────┐
│ Cross-Cutting Concerns                                      │
│  - Content addressing (applied at all layers)              │
│  - Audit logging (all operations logged)                   │
│  - Error propagation (consistent error types)              │
└─────────────────────────────────────────────────────────────┘
```

#### Implementation Strategy

**Phase 3a: Document Architecture (Week 3)**

Create architecture documentation:

1. **Overall Architecture (docs/070_layered_architecture.md)** - This document
2. **Layer 3 Documentation (docs/071_application_apis.md):**

```markdown
# 071 · Layer 3: Application APIs

## Purpose
Provides high-level APIs that application developers interact with.

## Components

### DeviceAgent
- `derive_simple_identity(app_id, context)` → compiles to DKD orchestration
- `add_guardian(contact)` → compiles to threshold event + CRDT merge
- `initiate_recovery()` → compiles to guardian coordination

### StorageClient
- `store_encrypted(payload, opts)` → compiles to encryption + replication
- `fetch_encrypted(cid, opts)` → compiles to retrieval + decryption

## Compilation Path
Each API call compiles to Layer 2 orchestration operations.

[Detailed API documentation...]
```

3. **Layer 2 Documentation (docs/072_orchestration.md):**

```markdown
# 072 · Layer 2: Orchestration & Coordination

## Purpose
Coordinates distributed operations and policy enforcement.

## Operations

### Threshold Coordination
- DKG: Coordinate distributed key generation
- Signing: Collect threshold signatures
- Resharing: Redistribute shares

### Policy Evaluation
- Cedar evaluation → decision
- Biscuit generation → capability token

### CRDT Coordination
- Propose event → collect signatures → merge
- Fetch state → validate → apply
- Merge remote state → resolve conflicts

## Compilation Path
Each orchestration operation compiles to Layer 1 primitive operations.

[Detailed orchestration documentation...]
```

4. **Layer 1 Documentation (docs/073_execution.md):**

```markdown
# 073 · Layer 1: Execution Layer

## Purpose
Provides atomic cryptographic, transport, and storage operations.

## Primitive Operations

### Cryptographic
- `sign(message, key)` → signature
- `verify(message, signature, pubkey)` → bool
- `encrypt(plaintext, key)` → ciphertext
- `decrypt(ciphertext, key)` → plaintext

### Transport
- `send(peer, message)` → confirmation
- `receive(peer, timeout)` → message
- `broadcast(peers, message)` → confirmations

### Storage
- `write(path, data)` → success
- `read(path)` → data
- `delete(path)` → success

## No Further Compilation
These are atomic operations that execute directly.

[Detailed primitive documentation...]
```

5. **Compilation Pipeline (docs/074_compilation_pipeline.md):**

```markdown
# 074 · Compilation Pipeline

## How Operations Compile Through Layers

### Example: add_guardian()

#### Layer 3 (Application API)
```rust
agent.add_guardian(contact).await?
```

#### Compiles to Layer 2 (Orchestration)
```rust
// 1. Create event
let event = Event::AddGuardian(guardian_entry);

// 2. Collect threshold signatures
let signatures = orchestrator.collect_signatures(&event, threshold).await?;

// 3. Create threshold signature
let threshold_sig = orchestrator.aggregate_signatures(signatures)?;

// 4. Propose to CRDT
ledger.propose_event(event, threshold_sig).await?;

// 5. Generate guardian envelope
let envelope = generate_recovery_share(guardian_id)?;

// 6. Distribute envelope
guardian_coordinator.distribute_share(guardian_id, envelope).await?;
```

#### Compiles to Layer 1 (Execution)
```rust
// From orchestrator.collect_signatures()
for device in participants {
    let msg = transport.send(device, SignRequest { event }).await?;
    let sig = transport.receive(device, timeout).await?;
    signatures.push(sig);
}

// From ledger.propose_event()
let event_bytes = serialize(&event)?;
let hash = crypto.hash(&event_bytes);
let stored = storage.write(format!("/events/{}", hash), event_bytes)?;

// From distribute_share()
let encrypted = crypto.encrypt(&share_envelope, guardian_pubkey)?;
let sent = transport.send(guardian_device, encrypted).await?;
```

[More examples...]
```

**Phase 3b: Refactor Crates (Week 3-4)**

Reorganize crates to match layers:

```
crates/
  # Layer 3: Application APIs
  agent/              # DeviceAgent, GuardianAgent
  storage/            # StorageClient
  
  # Layer 2: Orchestration
  orchestration/      # Threshold coordination, DKG
  ledger/             # CRDT coordination
  policy/             # Policy evaluation
  transform/          # Transform abstraction (NEW)
  
  # Layer 1: Execution
  crypto/             # Cryptographic primitives
  transport/          # Transport primitives (NEW: actual implementation)
  persistence/        # Storage primitives (NEW: split from storage/)
  
  # Cross-cutting
  types/              # Common types (NEW)
  audit/              # Audit logging (NEW: split from ledger/)
```

**Phase 3c: Implement Transport (Week 4-6)**

With clear architecture and protocol derivation, implement actual transport:

```rust
// crates/transport/src/lib.rs

/// Transport implementation using derived protocols
pub struct Transport {
    local_device: DeviceId,
    peer_connections: HashMap<PeerId, Connection>,
    protocol_registry: ProtocolRegistry,
}

impl Transport {
    /// Execute operation at specified location
    pub async fn execute<Op: TypedOperation>(
        &mut self,
        operation: Op,
        location: TransformLocation,
    ) -> Result<Op::Output> {
        // Derive protocol for operation
        let protocol = operation.derive_protocol();
        
        match location {
            TransformLocation::Local => {
                // Direct local execution
                operation.execute_locally()
            }
            
            TransformLocation::Device(device_id) => {
                // Execute on remote device
                let conn = self.get_connection(device_id)?;
                self.execute_remote(operation, protocol, conn).await
            }
            
            TransformLocation::Threshold { participants, threshold } => {
                // Coordinate threshold operation
                self.execute_threshold(operation, protocol, participants, threshold).await
            }
            
            TransformLocation::Peer(peer_id) => {
                // Execute on peer
                let conn = self.get_peer_connection(peer_id)?;
                self.execute_remote(operation, protocol, conn).await
            }
            
            TransformLocation::Replicated(locations) => {
                // Replicate across multiple locations
                self.execute_replicated(operation, protocol, locations).await
            }
        }
    }
    
    /// Execute remote operation following derived protocol
    async fn execute_remote<Op: TypedOperation>(
        &mut self,
        operation: Op,
        protocol: SessionType,
        conn: &mut Connection,
    ) -> Result<Op::Output> {
        match protocol {
            SessionType::Send(input_type, continuation) => {
                // Serialize input according to type
                let input_bytes = self.serialize_typed(&operation.get_input(), &input_type)?;
                
                // Send request
                conn.send(&input_bytes).await?;
                
                // Process continuation (typically Receive)
                match *continuation {
                    SessionType::Receive(output_type, _) => {
                        // Receive response
                        let response_bytes = conn.receive().await?;
                        
                        // Deserialize according to type
                        let output = self.deserialize_typed(&response_bytes, &output_type)?;
                        
                        Ok(output)
                    }
                    _ => Err(TransportError::InvalidProtocol),
                }
            }
            _ => Err(TransportError::UnsupportedProtocol),
        }
    }
}
```

**Benefits:**

- [x] Clear testing boundaries (test each layer independently)
- [x] Obvious where to add functionality
- [x] Natural optimization opportunities (each layer)
- [x] Better documentation and onboarding

---

## Implementation Timeline

### Week 1: Unified Transform Model
- [ ] Create `crates/transform/` with core traits
- [ ] Define `Transform` trait and `TransformLocation` enum
- [ ] Refactor DKD to implement `Transform`
- [ ] Update DeviceAgent API to use unified calls
- [ ] Write tests for both Local and Threshold modes

### Week 2: Protocol Derivation
- [ ] Define `TypedOperation` trait and `TypeDescriptor` enum
- [ ] Create CRDT operation definitions
- [ ] Implement `DeriveProtocol` for CRDT operations
- [ ] Create storage operation definitions
- [ ] Implement `DeriveProtocol` for storage operations

### Week 3: Architecture Documentation
- [ ] Write architecture overview (this document)
- [ ] Document Layer 3 (Application APIs)
- [ ] Document Layer 2 (Orchestration)
- [ ] Document Layer 1 (Execution)
- [ ] Document compilation pipeline with examples

### Week 4: Protocol Optimization
- [ ] Implement `ProtocolOptimizer` for batching
- [ ] Add batching tests
- [ ] Define guardian operation definitions
- [ ] Implement `DeriveProtocol` for guardian operations
- [ ] Integration tests for multi-operation batching

### Week 5-6: Transport Implementation
- [ ] Create `crates/transport/` implementation (not just types)
- [ ] Implement `Transport::execute()` with protocol execution
- [ ] Implement connection management
- [ ] Add HTTPS relay adapter (MVP transport)
- [ ] End-to-end integration tests

### Week 7: Integration & Testing
- [ ] Integration tests across all three layers
- [ ] End-to-end scenarios (add device, add guardian, recovery)
- [ ] Performance testing
- [ ] Documentation review
- [ ] Phase 1 completion demos

---

## Success Criteria

### Phase 1 MVP Completion

1. **Working Transport Layer**
   - [x] Can sync CRDT state between devices
   - [x] Can replicate storage chunks to peers
   - [x] Can coordinate guardian operations
   - [x] Protocols automatically derived from operations

2. **Unified APIs**
   - [x] Single API works for local and distributed modes
   - [x] Location parameter determines execution mode
   - [x] Easy to test with mocked locations

3. **Clear Architecture**
   - [x] Three layers explicitly documented
   - [x] Compilation path clear for each operation
   - [x] Testing boundaries well-defined
   - [x] New developers can navigate codebase

4. **End-to-End Functionality**
   - [x] Can add device via threshold coordination
   - [x] Can add guardian with share distribution
   - [x] Can execute recovery with cooldown
   - [x] Can store and replicate encrypted data

---

## Migration Path

### Backward Compatibility

Old APIs remain during transition:

```rust
impl DeviceAgent {
    /// New unified API (preferred)
    pub async fn derive_context_identity(
        &self,
        capsule: &ContextCapsule,
        location: TransformLocation,
    ) -> Result<DerivedIdentity> {
        // New implementation
    }
    
    /// Legacy API (deprecated, calls new API)
    #[deprecated(note = "Use derive_context_identity with TransformLocation::Local")]
    pub async fn derive_simple_identity(
        &self,
        app_id: &str,
        context_label: &str,
    ) -> Result<(DerivedIdentity, PresenceTicket)> {
        let capsule = ContextCapsule::simple(app_id, context_label);
        let identity = self.derive_context_identity(&capsule, TransformLocation::Local).await?;
        let ticket = self.issue_presence_ticket(&identity).await?;
        Ok((identity, ticket))
    }
}
```

### Gradual Migration

1. Week 1-2: New APIs available, old APIs still work
2. Week 3-4: Update internal code to use new APIs
3. Week 5-6: Update examples and documentation
4. Week 7+: Deprecate old APIs (but still functional)

---

## Risks and Mitigations

### Risk 1: Protocol Derivation Too Complex

**Mitigation:**
- Start with simple request-response patterns
- Add complexity incrementally as needed
- Manual protocol override available if needed

### Risk 2: Performance Overhead from Abstraction

**Mitigation:**
- Zero-cost abstraction for local operations (compile-time resolved)
- Remote operations already have network overhead (abstraction cost negligible)
- Profile and optimize hot paths

### Risk 3: Breaking Changes During Refactor

**Mitigation:**
- Keep old APIs working during transition
- Comprehensive test suite before refactoring
- Feature flags for gradual rollout

---

## Future Extensions

### Phase 2+

Once Phase 1 is complete, these enhancements become natural:

1. **Content-Addressed Architecture**
   - Systematic EntityId for all components
   - Global content store
   - Automatic deduplication

2. **Row Type Capabilities**
   - Account state as extensible row type
   - Static capability analysis
   - Schema versioning via content addressing

3. **Hybrid Value Model**
   - Size-based storage optimization
   - Inline small values, reference large values
   - Automatic threshold selection

4. **Additional Transports**
   - BitChat BLE mesh
   - WebRTC peer-to-peer
   - QUIC-based transport

---

## Conclusion

Adopting computation-communication symmetry, automatic protocol derivation, and layered architecture clarity will:

1. **Unblock Phase 1** by making transport implementation tractable
2. **Simplify development** with unified APIs and clear boundaries
3. **Enable testing** with natural mocking points
4. **Support evolution** with automatic protocol generation

This proposal provides a concrete path from current state to Phase 1 MVP completion, informed by proven architectural patterns from Causality.

The key insight: **unification and automatic derivation** dramatically reduce complexity. Rather than manually implementing separate code paths and protocols, make differences parameters and let the system handle the details.

---

## References

- Causality architecture documentation (timewave/causality/docs/)
- Aura specification (docs/020_architecture.md, docs/030_identity_spec.md)
- Current implementation review (work/00_review.md)
- Causality inspiration analysis (work/CAUSALITY_INSPIRATION.md)

