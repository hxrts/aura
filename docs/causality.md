# Causality VM Integration

## Overview

Aura is a threshold identity and encrypted storage platform. It implements distributed protocols for threshold cryptography, deterministic key derivation, social recovery, and eventually-consistent state synchronization. Currently these protocols are implemented directly in Rust using session types and choreographic programming.

Causality is a verifiable computation VM based on symmetric monoidal closed category theory. It provides automatic protocol derivation from data access patterns, location-transparent operations, and zero-knowledge proof generation. The VM implements five fundamental instructions that unify computation and communication.

This document describes how Aura can use Causality VM as its execution substrate. The integration would move protocol implementations from native Rust into verifiable VM applications while maintaining Aura's core abstractions.

## The Role of Causality VM

Causality VM serves as a verifiable execution layer for Aura's distributed protocols. The VM provides three key capabilities that align with Aura's requirements.

First, it provides automatic session type derivation. When a protocol accesses data across multiple devices, the VM analyzes the access patterns and generates the communication protocol automatically. This eliminates the manual choreography definitions that Aura currently requires.

Second, it provides location transparency through computation-communication symmetry. The same transform operation works whether the source and destination are both local, both remote, or one of each. A function call and a remote procedure call differ only in their location parameters. This unification simplifies protocol implementation significantly.

Third, it provides verifiable execution through ZK-compatible compilation. Every protocol step compiles to a fixed-size circuit that can generate zero-knowledge proofs. This enables verification without revealing protocol internals.

The VM operates on content-addressed data. Every value, transform, and protocol has a deterministic identifier derived from its SSZ-serialized hash. This aligns perfectly with Aura's existing content-addressed storage layer.

## The Role of Aura

Aura provides the application-level abstractions and platform integration that run on top of the VM. The Aura system has several responsibilities that remain outside the VM boundary.

First, Aura manages device-side state and lifecycle. The agent crate handles device initialization, session establishment, presence management, and protocol initiation. These coordination tasks remain in native Rust but delegate protocol execution to the VM.

Second, Aura provides platform-specific integrations. Secure key storage uses platform APIs like Keychain on macOS, Keystore on Android, and Secret Service on Linux. These integrations implement algebraic effects that the VM invokes during protocol execution.

Third, Aura maintains the distributed ledger. The Journal provides eventually-consistent account state using Automerge CRDT. The VM sees the Journal as an immutable event log with a pure materialization function. Sync protocols execute in the VM but the CRDT infrastructure remains in Aura.

Fourth, Aura provides transport and networking. The transport layer handles peer discovery, connection management, and message delivery. The VM invokes network effects during protocol execution but does not implement the transport itself.

## Architecture Layers

The integration introduces three layers between Aura components and Causality VM.

### Effect Handler Layer

The effect handler layer provides the boundary between verifiable VM execution and trusted platform operations. The VM cannot call platform APIs directly because this would break its verification model. Instead, protocols declare effects they need and the runtime provides handlers.

```rust
pub trait EffectHandler: Send + Sync {
    fn effect_type(&self) -> EffectType;
    fn handle(&self, effect: EffectData) -> Result<EffectResult, EffectError>;
}

pub enum EffectType {
    SecureStorage,
    Randomness,
    Time,
    Network,
    Crypto,
}
```

Platform implementations provide concrete handlers. On macOS the SecureStorage handler calls Keychain Services. On Linux it calls Secret Service API. The handlers are registered at runtime initialization and the VM invokes them during protocol execution.

This design keeps the VM pure and verifiable while allowing protocols to interact with the real world. Effect handlers are the trusted boundary. Everything above this layer generates verifiable proofs. Everything below this layer is platform-specific and trusted.

### Type Bridge Layer

The type bridge layer converts between Aura types and Causality types. Every Aura type that enters the VM must be converted to a content-addressed Causality value. Every result from the VM must be converted back to an Aura type.

```rust
pub trait ToCausality {
    fn to_causality_type(&self) -> CausalityType;
    fn to_entity_id(&self) -> EntityId;
    fn to_causality_value(&self) -> CausalityValue;
}

pub trait FromCausality: Sized {
    fn from_causality_value(value: CausalityValue) -> Result<Self, ConversionError>;
}
```

The conversions use SSZ serialization for deterministic encoding. A DeviceId becomes a Causality bytes value. A KeyShare becomes a Causality record with fields for identifier, signing share, and verifying key. A Journal event becomes a Causality record with event ID, parent IDs, payload, signatures, and timestamp.

All conversions are bidirectional. Types go into the VM as Causality values and come back out as Aura types. The bridge layer handles all the marshaling so protocol implementations work with native VM types.

### Protocol API Layer

The protocol API layer exposes high-level operations that Aura components invoke. Each protocol becomes a method on the VM runtime. The implementation creates an intent, compiles it to an executable protocol, executes it with effect handlers, and converts the result back.

```rust
impl AuraVmRuntime {
    pub async fn derive_key(
        &self,
        root_share: &KeyShare,
        app_id: &AppId,
        context: &str,
    ) -> Result<DerivedKey, ProtocolError>;
    
    pub async fn frost_sign(
        &self,
        key_share: &KeyShare,
        message: &[u8],
        participants: &[DeviceId],
        threshold: u32,
    ) -> Result<Signature, ProtocolError>;
    
    pub async fn recover_shares(
        &self,
        account_id: &AccountId,
        guardians: &[DeviceId],
        proof: &RecoveryProof,
    ) -> Result<Vec<KeyShare>, ProtocolError>;
    
    pub async fn sync_journal(
        &self,
        peer: DeviceId,
        local_journal: &Journal,
    ) -> Result<Journal, ProtocolError>;
}
```

These methods hide the VM machinery. Callers work with Aura types and get Aura types back. The protocol execution, session type derivation, and verification all happen inside the VM.

## Protocol Implementation Model

Protocols are implemented as Causality intents and transforms. An intent declares inputs, outputs, constraints, and effects. The VM compiler analyzes the intent and generates an executable protocol with derived session types.

### Pure Transforms

Deterministic Key Derivation is a pure transform. It takes a root key share, application ID, and context string. It produces a derived key. The computation is entirely local with no effects.

```rust
let dkd_transform = self.load_transform("dkd_v1").await?;

let result = self.vm.execute_transform(
    Location::Local(self.device_id),
    Location::Local(self.device_id),
    dkd_transform,
    vec![root_value, app_value, context_value],
).await?;
```

The transform implementation uses standard cryptographic operations. It derives a chain code from the root and app ID. It applies a KDF to the chain code and context. It returns the derived key material. The entire operation is deterministic and verifiable.

### Distributed Protocols

FROST signing is a distributed protocol. It requires multiple rounds of communication between participants. Each device generates commitments, exchanges them, generates signature shares, and then one device aggregates the shares into a final signature.

The protocol is declared as an intent with distributed sync constraints. The VM analyzes the data access patterns and derives the session protocol automatically.

```rust
let frost_intent = Intent::new("frost_sign")
    .with_input("key_share", key_share.to_causality_value())
    .with_input("message", CausalityValue::Bytes(message.to_vec()))
    .with_input("participants", participants_list)
    .with_constraint(TransformConstraint::DistributedSync {
        locations: participant_locations,
        sync_type: CausalityType::Base(BaseType::Bytes),
        consistency_model: ConsistencyModel::Strong,
    });

let protocol = self.vm.compile_intent(frost_intent).await?;
let result = self.vm.execute_protocol(protocol, self.device_id, &handlers).await?;
```

The VM generates a multi-round protocol. Round one sends commitment requests to all participants and collects responses. Round two sends signature share requests with the commitment list and collects shares. The final step aggregates shares locally. All communication happens through the Network effect handler.

### CRDT Synchronization

Journal sync is an eventually-consistent protocol. The Journal is an Automerge CRDT containing all account state. Each device has a local replica. Sync merges replicas by exchanging events.

The VM treats the Journal as an immutable event log. Sync is a protocol that exchanges events both devices are missing. Materialization is a pure transform that computes state from events.

```rust
let sync_intent = Intent::new("journal_sync")
    .with_input("local_events", event_list)
    .with_input("local_heads", head_list)
    .with_constraint(TransformConstraint::RemoteTransform {
        source_location: Location::Local(self.device_id),
        target_location: Location::Remote(peer),
        source_type: event_list_type,
        target_type: event_list_type,
        protocol: None, // Auto-derive
    })
    .with_consistency_model(ConsistencyModel::Eventual);
```

The VM derives a bidirectional sync protocol. Device A sends its event heads. Device B computes which events A is missing and sends them. Device B sends its heads. Device A computes which events B is missing and sends them. Both devices merge the new events into their local logs.

The materialization step is a separate pure transform. It takes an event log, sorts events by causal dependencies, and folds over them to compute the current state. This transform is deterministic and verifiable.

## Effect System Integration

Effects are the mechanism for protocols to interact with the outside world. When a protocol needs to store data securely, generate randomness, send a network message, or perform cryptographic operations, it declares an effect.

The effect declaration is part of the protocol definition. The VM validates that all required effects have handlers before execution begins. During execution the VM invokes handlers and provides results back to the protocol.

### Secure Storage Effects

Secure storage effects handle key material. Protocols use these to load key shares from platform secure storage and store derived keys.

```rust
pub enum SecureStorageOp {
    Store { key_id: String, data: Vec<u8> },
    Retrieve { key_id: String },
    Delete { key_id: String },
}
```

The handler implementation depends on the platform. On macOS it calls SecItemAdd and SecItemCopyMatching from Security framework. On Linux it calls libsecret. On Android it calls Android Keystore APIs. The protocol does not know which platform it runs on.

### Network Effects

Network effects handle message passing between devices. Protocols use these to send and receive data during distributed operations.

```rust
pub enum NetworkOp {
    Send { dest: DeviceId, message: Vec<u8> },
    Recv { timeout_ms: Option<u64> },
}
```

The handler delegates to Aura's transport layer. The transport handles connection management, peer discovery, and reliable delivery. The protocol just sends and receives opaque byte buffers.

### Cryptographic Effects

Cryptographic effects handle operations that require platform support or are performance-critical enough to warrant native implementation.

```rust
pub enum CryptoOp {
    Ed25519Sign { key: Vec<u8>, message: Vec<u8> },
    Ed25519Verify { pubkey: Vec<u8>, message: Vec<u8>, signature: Vec<u8> },
    HpkeEncrypt { recipient_key: Vec<u8>, plaintext: Vec<u8> },
    HpkeDecrypt { secret_key: Vec<u8>, ciphertext: Vec<u8> },
}
```

The handler uses Aura's crypto crate. Simple operations like signature verification might run inside the VM as pure transforms. Complex operations like HPKE encryption are better handled as effects.

## Content Addressing

Both Aura and Causality use content addressing. Every piece of data has an identifier derived from its content. This provides global deduplication, verifiable references, and cache-friendly protocols.

Causality uses SSZ serialization and SHA-256 hashing. An EntityId is the hash of the SSZ-encoded value. This matches Aura's content addressing scheme in the store crate.

Protocol transforms are content-addressed. The DKD transform has a fixed EntityId derived from its definition. All devices that load the DKD transform get the same code. Updates create new EntityIds rather than modifying existing transforms.

Journal events are content-addressed. Each event references parent events by their EntityIds. This creates a merkle DAG of causal dependencies. Sync protocols exchange events by their IDs and fetch missing content.

## Integration with Agent Layer

The agent layer uses the VM runtime to execute protocols. The agent maintains device-side state, manages the session lifecycle, and initiates protocols. Protocol execution happens in the VM.

```rust
pub struct AgentCore<T, S> {
    device_id: DeviceId,
    vm_runtime: AuraVmRuntime,
    transport: T,
    secure_store: S,
}

impl<T: Transport, S: SecureStore> AgentCore<T, S> {
    pub async fn derive_app_key(
        &self,
        app_id: &AppId,
        context: &str,
    ) -> Result<DerivedKey, AgentError> {
        let root_share = self.secure_store.get_key_share(&self.device_id).await?;
        self.vm_runtime.derive_key(&root_share, app_id, context).await
            .map_err(AgentError::Protocol)
    }
}
```

The agent loads the root key share from secure storage using platform APIs. It passes the share to the VM runtime along with the derivation parameters. The VM executes the DKD transform and returns the derived key. The agent can then use the key for application-specific operations.

This separation keeps platform integration in the agent while protocol logic runs in the verifiable VM. The agent handles bootstrap, presence, and coordination. The VM handles cryptographic protocols and distributed state management.

## Migration Path

Moving from native Rust protocols to VM protocols can happen incrementally. Each protocol can migrate independently without affecting the others.

Start with DKD because it is the simplest. It is a pure local computation with no effects and no distribution. Implement the transform, test it against the native implementation, benchmark performance, and switch over.

Next migrate the recovery protocol. It involves multiple devices but has a simple request-response pattern. Each guardian independently decides whether to approve and sends back an encrypted share. The automatic session derivation should handle this easily.

Then migrate Journal sync. It is more complex because it involves bidirectional communication and eventual consistency. The CRDT merge logic must be expressed as a pure materialization function. This validates that the immutable event log model works correctly.

Finally migrate FROST signing. It is the most complex protocol with multiple rounds and threshold coordination. The VM must correctly derive the multi-party session protocol. This is the validation that the VM can handle Aura's most demanding protocols.

During migration the native implementations remain available. Feature flags control which implementation runs. Tests validate equivalence between native and VM implementations. Performance benchmarks ensure the VM overhead is acceptable.

## Benefits and Tradeoffs

The VM integration provides several benefits over native protocol implementations.

Automatic protocol derivation eliminates manual choreography. The current approach requires defining session types by hand and implementing state machines for each protocol step. The VM derives protocols from data access patterns automatically.

Location transparency reduces code duplication. The current approach has separate code paths for local and remote operations. The VM unifies them through computation-communication symmetry.

Verifiable execution enables zero-knowledge proofs. The current approach runs in native Rust which cannot generate proofs. The VM compiles to fixed-size circuits suitable for ZK systems.

Content addressing enables global optimization. The current approach recompiles protocols per device. The VM can cache compiled protocols by their content IDs and share them across devices.

The tradeoffs are additional complexity and performance overhead.

The VM is a new dependency with its own learning curve. Developers must understand category theory concepts and the effect system. Protocol definitions use a Lisp-like DSL rather than Rust.

The VM adds interpretation overhead. Pure transforms compiled to native code will run faster than VM execution. Network-bound protocols may not notice the difference but local operations will be slower.

The effect boundary is a trust assumption. The VM can verify everything inside its execution model but effect handlers are trusted code. Platform integrations like Keychain access cannot be verified.

The immutable event log model for Journal requires restructuring. The current Automerge integration treats the CRDT as mutable state. Moving to immutable events plus materialization is a significant change.

## Performance Overhead Analysis

The VM introduces several sources of overhead compared to native Rust execution. Understanding these helps predict which protocols will see significant slowdown and which will not.

The VM uses a register machine with five fundamental instructions. Each high-level operation compiles to a sequence of these instructions. The transform instruction applies morphisms. The alloc instruction creates linear resources. The consume instruction destroys resources. The compose instruction chains operations. The tensor instruction combines resources in parallel.

A native Rust function call becomes a transform instruction in the VM. Simple operations like addition or comparison compile to single instructions. Complex operations like cryptographic functions compile to many instructions. The VM interpreter executes instructions sequentially with dispatch overhead per instruction.

For local cryptographic operations the overhead is measurable. Consider DKD which derives a key from a root share. The native implementation calls HMAC-SHA256 twice and some bit manipulation. This takes roughly 10 microseconds on modern hardware. The VM implementation would compile to approximately 50-100 instructions including hash operations as transforms. With interpretation overhead of 100-500 nanoseconds per instruction the total time becomes 15-60 microseconds. This represents 1.5x to 6x slowdown.

For FROST signing the overhead is less significant. The protocol has two network rounds where each participant exchanges data. Each round trip takes 50-200 milliseconds depending on network conditions. The local computation between rounds takes a few milliseconds. Adding 10-50 microseconds of VM overhead per device is negligible compared to network latency. The protocol completes in the same wall clock time.

For Journal materialization the overhead depends on event count. Each event application involves reading fields, updating state, and writing back. The native implementation processes roughly 10000 events per second. The VM implementation would likely achieve 2000-5000 events per second due to interpretation overhead. For typical Journals with hundreds of events this adds 50-200 milliseconds. For large Journals with tens of thousands of events this could add seconds.

The type conversion layer adds additional overhead. Converting between Aura types and Causality values requires SSZ serialization and hashing. A DeviceId conversion takes about 1 microsecond. A KeyShare conversion takes 5-10 microseconds. A Journal event conversion takes 10-20 microseconds depending on payload size. For protocols that process many values this overhead accumulates.

The VM compilation phase adds one-time cost. When a protocol runs for the first time the VM must compile the intent into an executable protocol. This involves constraint solving, session type derivation, and code generation. Compilation takes 10-100 milliseconds depending on protocol complexity. Compiled protocols are cached by their content IDs so subsequent executions skip this step.

For interactive operations like deriving an app key the overhead matters. Users expect key derivation to complete in under 100 milliseconds. Native execution achieves this easily. VM execution with 50 microseconds overhead plus 50 microseconds conversion overhead still completes well within the budget. The difference is not user-perceptible.

For background operations like Journal sync the overhead matters less. Sync happens opportunistically when devices connect. Whether it takes 200 milliseconds or 500 milliseconds does not affect user experience. The automatic protocol derivation benefit outweighs the performance cost.

For high-frequency operations the overhead becomes problematic. If an application derives thousands of keys per second the VM overhead dominates. Such applications should keep performance-critical code in native Rust and use the VM only for distributed coordination.

The VM provides optimization opportunities that native code lacks. Protocol transforms are content-addressed so identical operations across devices can share compiled code. The VM can cache execution traces for common patterns. Future optimizations like JIT compilation or specialized instruction implementations could reduce overhead significantly.

The most important optimization is native operation recognition. The VM can recognize when a Causality transform is isomorphic to a native implementation and dispatch directly to native code instead of interpreting. This eliminates interpretation overhead for common operations while maintaining verification properties.

Consider Ed25519 signature verification. The Causality Lisp definition describes the mathematical operations: point multiplication, hash computation, and curve arithmetic. The VM recognizes this pattern matches the native ed25519-dalek implementation. Instead of interpreting the transform, it calls the native function directly. The result is identical but performance matches native code.

This recognition happens at compile time. When the VM compiles an intent it analyzes each transform. If the transform structure matches a registered native implementation the compiler substitutes a native call instruction. The native implementations are verified separately to ensure they match their Causality specifications.

For cryptographic operations this optimization is essential. HMAC-SHA256, Ed25519 signing, scalar multiplication, and HPKE encryption all have highly optimized native implementations. The VM definitions serve as specifications and enable verification but execution uses native code. This reduces the DKD overhead from 6x to near-native performance.

The optimization extends beyond cryptography. JSON serialization, base64 encoding, UUID generation, and other common operations can have recognized native implementations. The VM maintains a registry of isomorphisms between Causality transforms and native functions.

```rust
pub struct NativeOperationRegistry {
    isomorphisms: HashMap<EntityId, Box<dyn NativeOperation>>,
}

pub trait NativeOperation: Send + Sync {
    fn transform_id(&self) -> EntityId;
    fn verify_isomorphism(&self) -> bool;
    fn execute(&self, inputs: Vec<CausalityValue>) -> Result<CausalityValue, ExecutionError>;
}

impl NativeOperationRegistry {
    pub fn register_crypto_operations(&mut self) {
        self.register(Ed25519SignOperation::new());
        self.register(Ed25519VerifyOperation::new());
        self.register(HmacSha256Operation::new());
        self.register(HpkeEncryptOperation::new());
    }
}
```

The verification step ensures the native implementation actually matches the Causality specification. This can use property testing to check that both implementations produce identical results for many random inputs. For cryptographic operations formal verification methods can prove equivalence.

This approach provides the best of both worlds. Protocol logic runs in the verifiable VM with automatic session derivation and location transparency. Performance-critical operations dispatch to native code with zero overhead. The Causality definitions serve as executable specifications that document what the native code must implement.

The optimization is transparent to protocol authors. They write transforms in Causality Lisp without worrying about performance. The VM automatically recognizes opportunities for native dispatch. If no native implementation exists the VM interprets the transform. Adding native implementations is a deployment optimization not a protocol change.

## Trust Boundary Clarification

The effect handler trust boundary is not actually a drawback compared to current Aura. Both approaches require trusting platform integration code. The difference is how the boundary is structured.

In current Aura the platform integration code is scattered throughout protocol implementations. The device_secure_store module calls Keychain APIs directly from within protocol logic. The transport adapters mix message serialization with connection management. The crypto operations are function calls inline with business logic. This makes it difficult to isolate what must be trusted from what could be verified.

With the VM the trust boundary becomes explicit and minimal. Everything inside the VM is verifiable. Everything outside the VM in effect handlers is trusted. The boundary is a clean interface with well-defined types. This does not reduce the amount of trusted code but it makes the trusted code easier to audit and reason about.

The effect handler interface is simpler than the current platform integration code. A SecureStorage effect handler has three operations: store, retrieve, and delete. The current device_secure_store module has many more methods with complex interactions. Reducing the interface surface reduces the attack surface.

The effect handlers are stateless and side-effect free from the VM perspective. The VM sees them as pure functions that take effect data and return results. The handlers themselves have side effects but those effects are contained. This isolation makes handlers easier to test and verify independently.

The VM enables gradual verification. Initially all effect handlers are trusted. As verification techniques improve certain handlers can be replaced with verified implementations. For example, cryptographic operations could move from trusted effects to verified VM transforms. The platform integration becomes progressively smaller over time.

The effect boundary also enables easier platform porting. Adding Android support means implementing Android-specific effect handlers. The protocol logic in the VM does not change. The current approach requires modifying protocol code to handle platform differences. The clean boundary reduces porting effort.

The effect handlers can be mocked for testing. Protocol tests run with mock handlers that return deterministic results. This enables testing protocol logic without real platform APIs. The current approach makes testing harder because platform code is intertwined with logic.

The VM enables protocol replay and debugging. Because effect handlers are explicit the VM can record all effect invocations during execution. Replaying a protocol means feeding the same effect results back. This makes debugging distributed protocols much easier than the current approach where side effects are implicit.

The trust assumption is the same but the structure is better. Both approaches trust platform code. The VM approach makes that trust explicit, minimal, and isolated. This is an improvement not a drawback.

## Journal Restructuring Details

Moving Journal from mutable CRDT to immutable event log requires changes in several areas. The restructuring affects how events are stored, how state is computed, and how sync works.

Currently the Journal wraps an Automerge document. The document contains account state like device list, guardian configuration, capability delegations, and metadata. Code modifies the document by calling Automerge methods. The document tracks operations internally and provides merge semantics. Sync exchanges Automerge sync messages which contain compressed operation logs.

The new model treats Journal as a pure event log. Each event is immutable and content-addressed. Events reference parent events by hash creating a DAG structure. The log contains only events. State is computed by folding over events in causal order. This is a pure function with no side effects.

The first change is event definition. Currently events are Automerge operations which are opaque. The new model needs explicit event types for each state change. Adding a device becomes a DeviceAdded event. Delegating a capability becomes a CapabilityDelegated event. Each event type has a schema defining its fields.

```rust
pub enum JournalEventPayload {
    AccountCreated { threshold: u32, participants: Vec<DeviceId> },
    DeviceAdded { device_id: DeviceId, verifying_key: VerifyingKey },
    DeviceRemoved { device_id: DeviceId, reason: String },
    CapabilityDelegated { from: DeviceId, to: DeviceId, capability: CapabilityToken },
    GuardianConfigured { guardian_id: DeviceId, encrypted_share: Vec<u8> },
}
```

The second change is storage format. Currently Automerge stores its internal representation which is optimized for merge operations. The new model stores events as individual content-addressed blobs. Each event is SSZ-serialized and stored by its hash. This enables deduplication and selective sync.

The third change is state computation. Currently state is the Automerge document itself. The new model computes state by applying events in order. This requires an apply function for each event type.

```rust
impl JournalState {
    fn apply_event(&mut self, event: &JournalEvent) -> Result<(), ApplyError> {
        match &event.payload {
            JournalEventPayload::DeviceAdded { device_id, verifying_key } => {
                self.devices.insert(*device_id, Device::new(*verifying_key));
            }
            JournalEventPayload::CapabilityDelegated { from, to, capability } => {
                self.capabilities.delegate(*from, *to, capability.clone())?;
            }
            // ... other event types
        }
        Ok(())
    }
}
```

The fourth change is materialization caching. Computing state from thousands of events is expensive. The system needs to cache materialized state and incrementally update it as new events arrive. This requires tracking which events have been applied to the cached state.

```rust
pub struct MaterializedJournal {
    events: BTreeMap<EventId, JournalEvent>,
    heads: BTreeSet<EventId>,
    state_cache: JournalState,
    state_cache_heads: BTreeSet<EventId>,
}

impl MaterializedJournal {
    pub fn materialize(&mut self) -> Result<&JournalState, MaterializeError> {
        if self.heads == self.state_cache_heads {
            return Ok(&self.state_cache);
        }
        
        let new_events = self.get_events_since(&self.state_cache_heads)?;
        for event in new_events {
            self.state_cache.apply_event(&event)?;
        }
        self.state_cache_heads = self.heads.clone();
        
        Ok(&self.state_cache)
    }
}
```

The fifth change is sync protocol. Currently sync uses Automerge sync protocol which exchanges compressed operation logs. The new model exchanges events by hash. Device A sends its head event hashes. Device B computes which events A is missing by traversing the DAG backwards from its heads. Device B sends the missing events. Device A applies them and updates its heads.

The sixth change is event validation. Currently Automerge handles operation validation internally. The new model must validate events explicitly. Each event must have valid threshold signatures. Events must reference valid parent events. Event application must not violate invariants.

The seventh change is conflict resolution. Currently Automerge handles conflicts through CRDT merge semantics. The new model must define explicit conflict resolution for concurrent events. For example, if two devices concurrently add different guardians both events are valid and both guardians are added. If two devices concurrently remove the same device only one remove event is needed.

The eighth change is schema evolution. Currently adding new state fields means updating Automerge document structure. The new model means defining new event types. Old devices must be able to ignore unknown event types gracefully. This requires forward compatibility in the apply logic.

The restructuring is significant but not overwhelming. The Journal crate is relatively small. The event types, apply logic, and materialization can be implemented in a few thousand lines. The sync protocol becomes simpler because it just exchanges events. The verification properties become stronger because events are explicit and immutable.

The migration path is to run both implementations in parallel temporarily. New events are written to both the Automerge document and the immutable log. State is computed from both and compared for equivalence. Once confidence is high the Automerge dependency is removed. Existing Journals can be converted by extracting their operation history and converting to explicit events.

## Conclusion

Causality VM provides a clean execution model for Aura's distributed protocols. The automatic protocol derivation, location transparency, and verifiable execution align well with Aura's goals. The effect system provides a principled boundary between verifiable logic and trusted platform integration.

The three-layer API design separates concerns cleanly. Effect handlers manage platform integration. Type bridges handle marshaling. Protocol APIs provide high-level operations. Aura components work with familiar types while the VM handles protocol execution.

The integration can proceed incrementally. Start with simple protocols, validate correctness and performance, then move to complex protocols. The native implementations remain as fallbacks during migration.

The benefits are substantial for protocols that need verifiability and automatic derivation. The tradeoffs are acceptable for a threshold identity system where correctness and security matter more than raw performance.
