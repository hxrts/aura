# Protocol Architecture Refactoring

## Purpose

This document outlines refactoring work to improve Aura's protocol architecture through better separation of concerns and cleaner abstractions. The goal is to establish clear API boundaries that make protocols easier to test, maintain, and evolve independently.

The refactoring focuses on three objectives. First, extract protocol implementations from agent logic so they can be replaced independently. Second, establish clear interfaces between cryptographic operations and protocol coordination. Third, prepare the Journal for immutable event log semantics.

## Current Architecture Analysis

Aura currently has protocols implemented directly in Rust with manual choreography. The architecture has three main layers.

The agent layer lives in the agent crate. AgentCore holds device state and coordinates operations. AgentProtocol implements session-typed state machines with states like Uninitialized, Idle, Coordinating, and Failed. The agent initiates protocols and manages their lifecycle.

The protocol layer lives in the aura-protocol crate. Individual protocol implementations exist as lifecycle modules: DkdLifecycle, FrostLifecycle, RecoveryLifecycle, ResharingLifecycle, GroupLifecycle. Each lifecycle implements the ProtocolLifecycle trait with methods for initialization, execution, and completion. The LifecycleScheduler coordinates protocol execution.

The crypto layer lives in the crypto crate. It provides FROST threshold signatures, DKD key derivation, HPKE encryption, and other primitives. These are pure cryptographic operations with no coordination logic.

The Journal lives in the journal crate. It wraps Automerge CRDT for eventually-consistent account state. Events are applied through the AccountLedger wrapper. State queries go through AccountState.

## Problems with Current Structure

Several aspects of the current architecture limit flexibility and testability.

Protocol implementations are coupled to agent state. The agent directly calls protocol lifecycle methods. Protocol state is stored in agent fields. This coupling prevents replacing protocols with alternative implementations without modifying agent code.

Protocol logic mixes coordination and computation. DkdLifecycle contains both the DKD cryptographic operations and the coordination for exchanging commitments. These concerns should be separated. Pure cryptographic operations should be isolated from distributed coordination logic.

The Journal treats Automerge as mutable state. Code calls methods that modify the CRDT in place. An immutable event log model with pure materialization would provide better auditability and enable alternative storage backends.

Effect boundaries are implicit. Secure storage, network operations, and randomness are called directly from protocol code. Explicit effect declarations with handler interfaces would improve testability and platform portability.

Protocol results lack standard representation. Each protocol returns different types through custom result enums. Uniform protocol results would simplify verification and enable consistent serialization.

## Refactoring Strategy

The preparation work happens in six phases. Each phase produces a working system that can be tested and validated before proceeding. The phases build on each other but remain focused on API boundaries rather than implementation changes.

### Phase 1: Protocol Interface Extraction

Extract protocol implementations behind clean interfaces that the agent calls without knowing implementation details.

Create a Protocol trait that all protocols implement. The trait defines lifecycle methods: initialize, step, is_complete, get_result. Each protocol becomes a self-contained unit that manages its own state. The trait provides a uniform interface regardless of underlying implementation.

```rust
/// Unified protocol interface for all distributed protocols
pub trait Protocol: Send + Sync {
    /// Protocol-specific input data
    type Input: Send + Sync;
    
    /// Protocol-specific output data
    type Output: Send + Sync;
    
    /// Internal protocol state (opaque to caller)
    type State: Send + Sync;
    
    /// Initialize protocol with input data
    fn initialize(&self, input: Self::Input) -> Result<Self::State>;
    
    /// Execute one step of the protocol
    fn step(&self, state: Self::State) -> Result<ProtocolStep<Self::State, Self::Output>>;
    
    /// Check if protocol has completed
    fn is_complete(&self, state: &Self::State) -> bool;
    
    /// Extract final result from completed state
    fn get_result(&self, state: Self::State) -> Result<Self::Output>;
}

/// Result of executing one protocol step
pub enum ProtocolStep<S, O> {
    /// Protocol continues with new state
    Continue(S),
    
    /// Protocol completed with final output
    Complete(O),
    
    /// Protocol requires an effect to continue
    RequiresEffect(EffectRequest, S),
}

/// Request for external effect during protocol execution
pub struct EffectRequest {
    pub effect: Effect,
    pub continuation: Box<dyn FnOnce(EffectResult) -> Result<()>>,
}
```

Refactor existing lifecycle implementations to implement this trait. DkdLifecycle becomes DkdProtocol implementing Protocol. The protocol owns its state rather than storing it in the agent. Each protocol implementation manages its coordination logic, network communication, and state transitions internally.

Update the agent to work with the Protocol trait. The agent holds Box<dyn Protocol> for active protocols. It calls trait methods without knowing which protocol is running. This decoupling allows protocol implementations to be swapped, replaced, or upgraded independently of agent code.

Create a ProtocolRegistry that maps protocol names to constructors. The agent looks up protocols by name rather than importing specific types. This enables dynamic protocol loading and selection at runtime.

```rust
/// Registry for protocol implementations
pub struct ProtocolRegistry {
    constructors: HashMap<String, Box<dyn ProtocolConstructor>>,
}

pub trait ProtocolConstructor: Send + Sync {
    fn construct(&self) -> Box<dyn Protocol>;
}

impl ProtocolRegistry {
    pub fn register(&mut self, name: &str, constructor: Box<dyn ProtocolConstructor>) {
        self.constructors.insert(name.to_string(), constructor);
    }
    
    pub fn get(&self, name: &str) -> Option<Box<dyn Protocol>> {
        self.constructors.get(name).map(|c| c.construct())
    }
}
```

Expected changes: Modify aura-protocol crate protocol implementations. Update agent protocol coordination code. Add new Protocol trait and ProtocolRegistry. No changes to crypto or journal crates yet.

Duration: 2 weeks.

### Phase 2: Effect Boundary Formalization

Make all side effects explicit through an effect system that matches the VM model.

Define Effect types for all side effects protocols use. SecureStorageEffect for key storage. NetworkEffect for message passing. RandomnessEffect for entropy. CryptoEffect for expensive operations. TimeEffect for timestamps. Each effect is a data structure describing an operation to be performed.

```rust
/// Side effects that protocols can request
pub enum Effect {
    SecureStorage(SecureStorageOp),
    Network(NetworkOp),
    Randomness(RandomnessOp),
    Crypto(CryptoOp),
    Time(TimeOp),
}

/// Secure storage operations
pub enum SecureStorageOp {
    Store { key: String, value: Vec<u8> },
    Retrieve { key: String },
    Delete { key: String },
}

/// Network communication operations
pub enum NetworkOp {
    Send { dest: DeviceId, message: Vec<u8> },
    Receive { timeout_ms: Option<u64> },
}

/// Randomness generation operations
pub enum RandomnessOp {
    RandomBytes { count: usize },
    RandomScalar,
}

/// Cryptographic operations
pub enum CryptoOp {
    Ed25519Sign { key: Vec<u8>, message: Vec<u8> },
    Ed25519Verify { pubkey: Vec<u8>, message: Vec<u8>, signature: Vec<u8> },
    HpkeEncrypt { recipient_key: Vec<u8>, plaintext: Vec<u8> },
    HpkeDecrypt { secret_key: Vec<u8>, ciphertext: Vec<u8> },
}

/// Time operations
pub enum TimeOp {
    Now,
    Sleep { duration_ms: u64 },
}

/// Results from effect execution
pub enum EffectResult {
    Unit,
    Bytes(Vec<u8>),
    Bool(bool),
    Timestamp(u64),
    Message { from: DeviceId, data: Vec<u8> },
}
```

Create EffectHandler trait for implementing effects. Each platform provides concrete handlers. The effect system routes effect requests to registered handlers. Handlers are stateless and side-effect free from the protocol perspective.

```rust
/// Handler for executing effects
pub trait EffectHandler: Send + Sync {
    fn handle(&self, effect: Effect) -> Result<EffectResult>;
}

/// Runtime for executing effects
pub struct EffectRuntime {
    handlers: HashMap<EffectType, Box<dyn EffectHandler>>,
}

impl EffectRuntime {
    pub fn register_handler(&mut self, effect_type: EffectType, handler: Box<dyn EffectHandler>) {
        self.handlers.insert(effect_type, handler);
    }
    
    pub fn execute(&self, effect: Effect) -> Result<EffectResult> {
        let effect_type = effect.effect_type();
        let handler = self.handlers.get(&effect_type)
            .ok_or(Error::NoHandlerForEffect)?;
        handler.handle(effect)
    }
}

/// Types of effects
pub enum EffectType {
    SecureStorage,
    Network,
    Randomness,
    Crypto,
    Time,
}
```

Refactor protocols to declare effects rather than calling platform APIs directly. When a protocol needs randomness it returns RequiresEffect(RandomnessEffect). The runtime handles the effect and returns control to the protocol with the result. This separation makes protocols testable with mock handlers and portable across platforms.

Update secure storage implementations to implement EffectHandler. KeychainHandler for macOS, SecretServiceHandler for Linux. These adapt the existing platform code to the effect interface.

Expected changes: Add effect types to aura-protocol crate. Modify protocol implementations to use effects. Create effect handlers for each platform in agent crate. Update agent to run an EffectRuntime.

Duration: 3 weeks.

### Phase 3: Crypto Operation Isolation

Separate pure cryptographic operations from protocol coordination.

Extract crypto transforms into pure functions. The DKD cryptographic operations should be a single function that takes inputs and returns outputs with no side effects or coordination. Pure functions are deterministic, testable, and composable.

```rust
/// Pure deterministic key derivation
pub fn dkd_derive(
    root_share: &KeyShare,
    app_id: &AppId,
    context: &str,
) -> DerivedKey {
    // Chain code derivation
    let chain_code = compute_chain_code(root_share, app_id);
    
    // Key derivation function
    let key_material = kdf(chain_code, context);
    
    // Return derived key
    DerivedKey::new(key_material)
}
```

The FROST operations should be split into distinct phases. Round 1 generates commitments. Round 2 generates signature shares. Aggregation combines shares. Each is a pure function that operates on data without side effects.

```rust
/// FROST round 1: generate commitment
pub fn frost_round1(
    key_share: &KeyShare,
    message: &[u8],
    nonce: &Scalar,
) -> Commitment {
    let commitment_point = nonce * G;
    let binding_factor = hash(key_share.identifier, message);
    Commitment::new(commitment_point, binding_factor)
}

/// FROST round 2: generate signature share
pub fn frost_round2(
    key_share: &KeyShare,
    message: &[u8],
    commitments: &[Commitment],
) -> SignatureShare {
    let group_commitment = aggregate_commitments(commitments);
    let challenge = compute_challenge(message, group_commitment);
    let response = key_share.signing_share * challenge + key_share.nonce;
    SignatureShare::new(key_share.identifier, response)
}

/// FROST aggregation: combine signature shares
pub fn frost_aggregate(
    message: &[u8],
    shares: &[SignatureShare],
    commitments: &[Commitment],
) -> Result<Signature> {
    validate_shares(shares, commitments)?;
    let aggregated_response = sum_responses(shares);
    let aggregated_commitment = aggregate_commitments(commitments);
    Ok(Signature::new(aggregated_response, aggregated_commitment))
}
```

Move coordination logic out of crypto crate. The crypto crate should contain only pure cryptographic operations. The protocol implementations in aura-protocol contain the coordination for exchanging data between participants and managing protocol state.

Create a transforms module in aura-protocol that wraps crypto operations as protocol steps. These are thin wrappers that prepare inputs and process outputs. The separation between computation and coordination makes both easier to understand and test.

Expected changes: Refactor crypto crate to expose pure functions. Move any coordination code from crypto to aura-protocol. Update protocol implementations to call pure crypto functions.

Duration: 2 weeks.

### Phase 4: Protocol Result Standardization

Establish uniform protocol result types for consistent handling.

Define a standard ProtocolResult type that all protocols return. The result contains output data, execution metadata, and verification information. Standardization enables uniform handling, serialization, and verification across all protocols.

```rust
/// Standard result type for all protocols
pub struct ProtocolResult {
    /// Name of the protocol that produced this result
    pub protocol_name: String,
    
    /// Protocol-specific output data
    pub output: ProtocolOutput,
    
    /// Execution metadata for verification
    pub metadata: ExecutionMetadata,
    
    /// Optional execution trace for debugging
    pub trace: Option<ExecutionTrace>,
}

/// Protocol output variants
pub enum ProtocolOutput {
    DerivedKey(DerivedKey),
    Signature(Signature),
    RecoveredShares(Vec<KeyShare>),
    JournalState(JournalState),
}

/// Metadata about protocol execution
pub struct ExecutionMetadata {
    /// When protocol execution started
    pub started_at: u64,
    
    /// When protocol execution completed
    pub completed_at: u64,
    
    /// Number of participants involved
    pub participant_count: usize,
    
    /// Number of communication rounds
    pub round_count: usize,
    
    /// Hash of all inputs for verification
    pub input_hash: [u8; 32],
    
    /// Signatures from participants
    pub participant_signatures: Vec<Signature>,
}

/// Execution trace for debugging
pub struct ExecutionTrace {
    pub steps: Vec<ProtocolStepTrace>,
}

pub struct ProtocolStepTrace {
    pub step_number: usize,
    pub step_type: String,
    pub timestamp: u64,
    pub duration_micros: u64,
}
```

Update all protocol implementations to return ProtocolResult. The protocol-specific output goes in the ProtocolOutput enum variant.

Add serialization for protocol results. Results should serialize to CBOR for storage and transmission. This prepares for VM results which will also be CBOR-encoded.

Create result verification helpers. Given a ProtocolResult, verify that signatures are valid, participants match expectations, and execution completed correctly.

Expected changes: Add ProtocolResult types to aura-protocol crate. Update all protocol implementations to return this type. Add serialization implementations. Update agent to handle standard results.

Duration: 1 week.

### Phase 5: Journal Event Log Preparation

Restructure Journal to support immutable event log semantics while maintaining current Automerge integration.

Define explicit Journal event types. Currently events are opaque Automerge operations. Create concrete types for each state change. Explicit events provide auditability, enable alternative storage backends, and make state transitions inspectable.

```rust
/// Journal event types representing state changes
pub enum JournalEvent {
    AccountCreated {
        threshold: u32,
        participants: Vec<DeviceId>,
    },
    DeviceAdded {
        device_id: DeviceId,
        verifying_key: VerifyingKey,
    },
    DeviceRemoved {
        device_id: DeviceId,
        reason: String,
    },
    CapabilityDelegated {
        from: DeviceId,
        to: DeviceId,
        capability: Capability,
    },
    GuardianConfigured {
        guardian_id: GuardianId,
        encrypted_share: Vec<u8>,
    },
    EpochIncremented {
        new_epoch: u64,
        reason: String,
    },
}

/// Signed event with causal dependencies
pub struct SignedEvent {
    /// Content-addressed event identifier
    pub event_id: EventId,
    
    /// Parent event IDs (causal dependencies)
    pub parent_ids: Vec<EventId>,
    
    /// Event payload
    pub payload: JournalEvent,
    
    /// Threshold signature from M-of-N devices
    pub signatures: ThresholdSignature,
    
    /// Timestamp of event creation
    pub timestamp: u64,
}

impl SignedEvent {
    /// Compute event ID from content
    pub fn compute_id(&self) -> EventId {
        let serialized = serialize_event(&self.payload, &self.parent_ids, self.timestamp);
        EventId::from_hash(blake3::hash(&serialized))
    }
    
    /// Verify threshold signatures
    pub fn verify_signatures(&self, group_key: &VerifyingKey) -> Result<bool> {
        let message = self.signing_message();
        self.signatures.verify(&message, group_key)
    }
}
```

Add an event log alongside the existing Automerge document. When state changes occur, create both an Automerge operation and a SignedEvent. Store signed events in a content-addressed event store.

Implement event materialization as a pure function. Given a list of signed events in causal order, compute the current Journal state. This can run alongside Automerge initially for validation. Pure materialization is deterministic and enables state reconstruction from events at any point in time.

```rust
impl Journal {
    /// Materialize Journal state from event log
    pub fn from_events(events: &[SignedEvent]) -> Result<JournalState> {
        // Start with empty state
        let mut state = JournalState::empty();
        
        // Topologically sort events by causal dependencies
        let ordered = topological_sort(events)?;
        
        // Apply each event in order
        for event in ordered {
            state.apply_event(&event)?;
        }
        
        Ok(state)
    }
}

impl JournalState {
    /// Apply single event to state
    fn apply_event(&mut self, event: &SignedEvent) -> Result<()> {
        // Verify event signatures
        event.verify_signatures(&self.group_key)?;
        
        // Apply event based on type
        match &event.payload {
            JournalEvent::AccountCreated { threshold, participants } => {
                self.threshold = *threshold;
                self.participants = participants.clone();
            }
            JournalEvent::DeviceAdded { device_id, verifying_key } => {
                self.devices.insert(*device_id, Device::new(*verifying_key));
            }
            JournalEvent::DeviceRemoved { device_id, reason } => {
                self.devices.get_mut(device_id)?.mark_removed(reason);
            }
            JournalEvent::CapabilityDelegated { from, to, capability } => {
                self.capabilities.delegate(*from, *to, capability.clone())?;
            }
            JournalEvent::GuardianConfigured { guardian_id, encrypted_share } => {
                self.guardians.insert(*guardian_id, encrypted_share.clone());
            }
            JournalEvent::EpochIncremented { new_epoch, reason } => {
                self.epoch = *new_epoch;
            }
        }
        
        Ok(())
    }
}

/// Topologically sort events by causal dependencies
fn topological_sort(events: &[SignedEvent]) -> Result<Vec<SignedEvent>> {
    let mut sorted = Vec::new();
    let mut visited = HashSet::new();
    let event_map: HashMap<EventId, &SignedEvent> = 
        events.iter().map(|e| (e.event_id, e)).collect();
    
    for event in events {
        visit_event(event, &event_map, &mut visited, &mut sorted)?;
    }
    
    Ok(sorted)
}

fn visit_event(
    event: &SignedEvent,
    event_map: &HashMap<EventId, &SignedEvent>,
    visited: &mut HashSet<EventId>,
    sorted: &mut Vec<SignedEvent>,
) -> Result<()> {
    if visited.contains(&event.event_id) {
        return Ok(());
    }
    
    // Visit parents first
    for parent_id in &event.parent_ids {
        if let Some(parent) = event_map.get(parent_id) {
            visit_event(parent, event_map, visited, sorted)?;
        }
    }
    
    visited.insert(event.event_id);
    sorted.push(event.clone());
    Ok(())
}
```

Add event validation logic. Each event must have valid threshold signatures. Events must reference valid parents. Event application must not violate invariants.

Run both paths in parallel. The Automerge path continues working as before. The event log path runs alongside and results are compared. This validates the event log model before migration.

Expected changes: Add event types to journal crate. Implement event log storage. Add materialization function. Update state mutation code to emit events. No disruption to existing Journal users.

Duration: 3 weeks.

### Phase 6: Transport Abstraction

Separate protocol message passing from transport implementation.

Define a message envelope type for protocol messages. The envelope contains routing information, protocol identifier, and serialized payload. This provides uniform message structure across all protocols.

```rust
/// Protocol message envelope
pub struct ProtocolMessage {
    /// Unique protocol identifier
    pub protocol_id: ProtocolId,
    
    /// Sender device
    pub from: DeviceId,
    
    /// Recipient device
    pub to: DeviceId,
    
    /// Protocol round number
    pub round: u32,
    
    /// Message sequence number (for ordering)
    pub sequence: u64,
    
    /// Serialized message payload
    pub payload: Vec<u8>,
    
    /// Message timestamp
    pub timestamp: u64,
}

pub struct ProtocolId {
    pub name: String,
    pub session_id: SessionId,
}
```

Create a ProtocolTransport trait for sending and receiving protocol messages. This abstracts over the actual transport implementation. The trait enables testing with mock transports and supports alternative network backends.

```rust
/// Transport abstraction for protocol messages
pub trait ProtocolTransport: Send + Sync {
    /// Send message to recipient
    async fn send(&self, message: ProtocolMessage) -> Result<()>;
    
    /// Receive next message with optional timeout
    async fn receive(&self, timeout: Option<Duration>) -> Result<ProtocolMessage>;
    
    /// Broadcast message to multiple recipients
    async fn broadcast(&self, recipients: &[DeviceId], message: ProtocolMessage) -> Result<()> {
        for recipient in recipients {
            let mut msg = message.clone();
            msg.to = *recipient;
            self.send(msg).await?;
        }
        Ok(())
    }
}
```

Implement ProtocolTransport using existing Aura transport. This is an adapter that bridges protocol messages to transport envelopes. The adapter handles serialization, routing, and connection management.

```rust
/// Adapter from protocol transport to Aura transport
pub struct AuraTransportAdapter {
    transport: Arc<dyn Transport>,
    device_id: DeviceId,
}

impl ProtocolTransport for AuraTransportAdapter {
    async fn send(&self, message: ProtocolMessage) -> Result<()> {
        let envelope = self.protocol_message_to_envelope(message)?;
        self.transport.send(envelope).await
    }
    
    async fn receive(&self, timeout: Option<Duration>) -> Result<ProtocolMessage> {
        let envelope = self.transport.receive(timeout).await?;
        self.envelope_to_protocol_message(envelope)
    }
}
```

Update protocol implementations to use ProtocolTransport. Protocols no longer import the transport crate directly. They receive a ProtocolTransport instance and call its methods. This decoupling enables protocol portability and simplifies testing with mock transports.

Expected changes: Add message types to aura-protocol crate. Create ProtocolTransport trait. Implement adapter in transport crate. Update protocols to use abstraction.

Duration: 1 week.

## Post-Refactoring Architecture

After these six phases the architecture will have clear boundaries that improve maintainability and flexibility.

Protocols will be self-contained implementations behind the Protocol trait. The agent coordinates protocols without knowing their implementation. Swapping a protocol for an alternative implementation requires only implementing the Protocol trait. This enables experimentation with different protocol designs.

Effects will be explicit with handler interfaces. Protocols declare effects rather than calling platform APIs. The effect runtime routes requests to handlers. Testing becomes simpler with mock handlers. Platform porting requires only new effect handler implementations.

Crypto operations will be pure functions. The crypto crate provides well-defined transforms. Protocol coordination is separate from cryptographic computation. Pure functions are easier to test, audit, and optimize.

Protocol results will be uniform. All protocols return ProtocolResult with standard metadata. Results serialize consistently for storage and verification. Uniform results simplify logging, debugging, and result verification.

The Journal will support event log semantics. Events are explicit and content-addressed. Materialization is a pure function. The event log enables audit trails and state reconstruction. The Automerge integration remains for backward compatibility during transition.

Transport will be abstracted. Protocols use ProtocolTransport for messaging. The transport implementation is pluggable. Testing with mock transports becomes straightforward. Alternative network backends can be added without changing protocol code.

## Benefits of Refactored Architecture

The refactored architecture provides several concrete benefits.

Testing becomes simpler. Protocols can be tested in isolation with mock effect handlers. Mock transports enable testing without real network connections. Pure crypto functions are easy to property test. Effect isolation enables deterministic testing.

Platform porting is easier. Adding a new platform requires implementing effect handlers for that platform. Protocol code remains unchanged. The effect system provides a clean integration point for platform-specific code.

Protocol evolution is safer. Protocols are self-contained behind the Protocol trait. Changing a protocol implementation does not affect other protocols or the agent. Protocol versions can coexist in the registry.

Debugging is clearer. Execution traces show protocol steps. Effect requests are explicit and logged. Pure functions are deterministic and reproducible. The separation of concerns makes problems easier to isolate.

Code is more maintainable. Pure crypto functions are simple and focused. Protocol coordination is separate from computation. Effects have clear interfaces. Each component has a single responsibility.

## Validation Strategy

Each preparation phase produces testable changes that maintain correctness.

For Phase 1, validate that protocols work through the Protocol trait. Run existing integration tests. Verify that DKD, FROST, and recovery still produce correct results.

For Phase 2, validate that effects work correctly. Run protocols with mock effect handlers. Verify secure storage effects access correct platform APIs. Test network effects with simulated transport.

For Phase 3, validate that extracted crypto operations produce identical results. Property test that crypto transforms match original implementations on thousands of random inputs.

For Phase 4, validate that protocol results serialize and deserialize correctly. Test that result verification catches invalid signatures and incorrect execution.

For Phase 5, validate that event materialization produces identical state to Automerge. Run both paths on production data. Compare results. Find and fix any discrepancies.

For Phase 6, validate that transport abstraction works correctly. Send messages through ProtocolTransport adapter. Verify they arrive at correct destinations with correct payloads.

The validation ensures preparation work does not break existing functionality. Each phase is merged only after tests pass.

## Timeline and Effort

The total refactoring work spans approximately 12 weeks with one developer.

Phase 1 protocol interface extraction takes 2 weeks. Phase 2 effect boundary formalization takes 3 weeks. Phase 3 crypto operation isolation takes 2 weeks. Phase 4 protocol result standardization takes 1 week. Phase 5 Journal event log preparation takes 3 weeks. Phase 6 transport abstraction takes 1 week.

Some phases can overlap. Phase 3 crypto work can start while Phase 2 effect work continues. Phase 6 transport work is independent and can happen in parallel with other phases.

With careful planning the critical path is approximately 10 weeks. This delivers a codebase ready for VM integration with minimal risk of breaking existing functionality.

## Success Criteria

The refactoring succeeds when these conditions are met.

Protocols implement the Protocol trait and work through the registry. The agent coordinates protocols without importing specific protocol types. Protocol implementations are interchangeable.

All side effects go through the effect system. Protocols declare effects. Handlers satisfy effect requests. No protocol code calls platform APIs directly. Mock handlers work correctly in tests.

Crypto operations are pure functions in the crypto crate. Protocol coordination lives in aura-protocol. The separation is clean and testable. Crypto functions have no side effects.

All protocols return ProtocolResult. Results serialize consistently. Verification works uniformly across protocol types. Results can be logged and inspected easily.

The Journal maintains an event log with pure materialization. Event log results match Automerge results on all test cases. State reconstruction from events works correctly.

Protocols use ProtocolTransport for messaging. The transport implementation is pluggable. Messages route correctly. Mock transport works in tests.

All existing tests pass. Integration tests validate DKD, FROST, recovery, and Journal sync. No regressions in functionality or security properties. Test coverage improves due to better abstractions.

These criteria validate that the refactoring has achieved its goals. The API boundaries are clean. The abstractions are appropriate. The code is more maintainable and testable.
