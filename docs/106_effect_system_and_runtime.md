# Effect System and Runtime

This document describes the effect system and runtime architecture in Aura. It defines effect traits, handler design, context propagation, lifecycle management, and integration across crates. It also describes testing modes and performance considerations.

## 1. Effect Traits and Categories

Aura defines effect traits as abstract interfaces for system capabilities. Core traits expose essential functionality. Extended traits expose coordinated or system-wide behaviors. Each trait is independent and does not assume global state.

Core traits include `CryptoEffects`, `NetworkEffects`, `StorageEffects`, time domain traits (`PhysicalTimeEffects`, `LogicalClockEffects`, `OrderClockEffects`, `TimeAttestationEffects`), `RandomEffects`, and `JournalEffects`. Extended traits include `SystemEffects`, `LedgerEffects`, `ChoreographicEffects`, and `AgentEffects`.

```rust
#[async_trait]
pub trait CryptoEffects {
    async fn hash(&self, data: &[u8]) -> [u8; 32];
    async fn hmac(&self, key: &[u8], data: &[u8]) -> [u8; 32];
}
```

This example shows a core effect trait. Implementations provide cryptographic operations. Traits contain async methods for compatibility with async runtimes.

### 1.1 Unified Time Traits

The legacy monolithic `TimeEffects` trait is replaced by domain-specific traits:

- `PhysicalTimeEffects` – returns `PhysicalTime { ts_ms, uncertainty }` and `sleep_ms` for wall-clock operations.
- `LogicalClockEffects` – advances and reads causal vector clocks and Lamport scalars.
- `OrderClockEffects` – produces opaque, privacy-preserving total order tokens without temporal meaning.
- `TimeAttestationEffects` – wraps physical claims in provenance proofs when consensus/peer attestation is required.

Callers select the domain appropriate to their semantics (guards/transport use physical, CRDT uses logical, privacy-preserving ordering uses order tokens). Cross-domain comparisons are explicit via `TimeStamp::compare(policy)`. Direct `SystemTime::now()` or chrono usage is forbidden outside effect implementations; testkit and simulator provide deterministic handlers for all four traits.

### 1.2 When to Create Effect Traits

Create new effect traits when:
- Abstracting OS or external system integration (files, network, time)
- Defining domain-specific operations that multiple implementations might provide
- Isolating side effects for testing and simulation
- Enabling deterministic simulation of complex behaviors

### 1.3 When NOT to Create Effect Traits

Follow YAGNI (You Aren't Gonna Need It) principles:

**Defer abstraction when:**
- Only one implementation exists and will likely remain single
- The abstraction adds complexity without clear benefit
- You're abstracting "just in case" without concrete need
- Testing can be achieved through higher-level mocking

**Example: Threshold Signatures**

Aura uses FROST directly without a `ThresholdSigEffects` trait because:
- Only FROST is needed currently
- No plans for alternative threshold signature schemes
- Direct usage is simpler and clearer
- Testing happens at the consensus level, not crypto level

See `crates/aura-core/src/crypto/README.md` for the detailed threshold signature deferral decision. We maintain a clear YAGNI gate: introduce the trait only when a second scheme is required or FROST needs replacement.

## 2. Handler Design

Effect handlers implement effect traits. Stateless handlers execute operations without internal state. Stateful handlers coordinate multiple effects or maintain internal caches.

Typed handlers implement concrete effect traits. Type-erased handlers allow dynamic dispatch through the effect executor. Both designs share the same execution interface.

Handlers do not store global state. All required inputs flow through method parameters. This avoids hidden dependencies.

```rust
pub struct RealCryptoHandler;

#[async_trait]
impl CryptoEffects for RealCryptoHandler {
    async fn hash(&self, data: &[u8]) -> [u8; 32] {
        aura_core::hash::hash(data)
    }

    async fn hmac(&self, key: &[u8], data: &[u8]) -> [u8; 32] {
        // HMAC implementation
        unimplemented!()
    }
}
```

This code block defines a stateless handler. It uses synchronous hashing from `aura_core::hash` for deterministic behavior.

## 3. Context Model

The effect system propagates an `EffectContext` through async tasks. The context carries tracing data, flow budget, metadata, and deadlines. The context is explicit. No ambient state exists.

```rust
pub struct EffectContext {
    pub request_id: Uuid,
    pub flow_budget: FlowBudget,
    pub deadline: Option<Instant>,
    pub metadata: HashMap<String, String>,
}
```

This structure defines the effect context. The context flows through all effect calls. Tracing integrates with this structure. Flow budget enforcement uses the context values.

Context propagation uses scoped execution. A task local stores the current context. Nested tasks inherit the context. This ensures consistent behavior across async boundaries.

## 4. Lifecycle Management

Aura defines a lifecycle manager for initialization and shutdown. Each handler may perform startup tasks. Each handler may also perform cleanup on shutdown.

Handlers register with a lifecycle manager. The lifecycle manager executes initialization in order. The lifecycle manager executes shutdown in reverse order.

```rust
pub struct LifecycleManager {
    state: Arc<AtomicU8>,
    components: Arc<RwLock<Vec<Arc<dyn LifecycleAware>>>>,
}
```

This type defines the lifecycle manager. It tracks registered components. It provides explicit methods for transitioning between lifecycle phases.

Lifecycle phases include initialization, ready, shutting down, and shutdown. Health checks monitor handler availability.

## 5. Layers and Crates

The effect system spans several crates. Each crate defines a specific role in the architecture. These crates maintain strict dependency boundaries.

`aura-core` defines effect traits, identifiers, and core data structures. It contains no implementations.

`aura-effects` contains stateless and single-party effect handlers. It provides default implementations for cryptography, storage, networking, and randomness.

`aura-protocol` contains orchestrated and multi-party behavior. It bridges session types to effect calls. It implements the [guard chain](109_authorization.md), journal coupling, and consensus integrations.

`aura-agent` assembles handlers into runnable systems. It configures effect pipelines for production environments.

`aura-simulator` provides deterministic execution. It implements simulated time, simulated networking, and controlled failure injection.

## 6. Testing and Simulation

The effect system supports deterministic testing. Mock handlers implement predictable behavior. A simulated runtime provides control over time and network behavior. The simulator exposes primitives to inject delays or failures.

Tests use deterministic time control. Tests use in-memory storage. Tests use mock network. These components allow protocol execution without side effects.

```rust
let system = TestRuntime::new()
    .with_mock_crypto()
    .with_deterministic_time()
    .build();
```

This snippet creates a test runtime. The runtime uses mock handlers for all effects. It provides deterministic time and network control.

## 7. Performance Considerations

Aura includes several performance optimizations. Parallel initialization reduces startup time. Caching handlers reduce repeated computation. Buffer pools reduce memory allocation.

The effect system avoids OS threads for WASM compatibility. It uses async tasks and cooperative scheduling. Lazy initialization creates handlers on first use.

```rust
let builder = EffectSystemBuilder::new()
    .with_handler(Arc::new(RealCryptoHandler))
    .with_parallel_init();
```

This snippet shows parallel initialization of handlers. Parallel initialization increases startup throughput.

## 8. Guard Chain and Leakage Integration

The effect runtime enforces the guard-chain sequencing defined in [Authorization](109_authorization.md) and the leakage contract from [Privacy and Information Flow](003_information_flow_contract.md) using pure guard evaluation plus asynchronous interpretation. Each projected choreography message expands to:

1. **Snapshot preparation (async)** – gather capability frontier, budget headroom, leakage metadata, and randomness into a `GuardSnapshot` via `AuthorizationEffects`, `FlowBudgetEffects`, and cache state.
2. **Pure guard evaluation (sync)** – `CapGuard → FlowGuard → JournalCoupler` runs over the snapshot and request, producing a `GuardOutcome` that describes the authorization decision plus the `Vec<EffectCommand>` commands that need to execute next.
3. **Command interpretation (async)** – an `EffectInterpreter` executes each `EffectCommand` using `FlowBudgetEffects`, `LeakageEffects`, `JournalEffects`, and `TransportEffects`, preserving charge-before-send.

Handlers that implement `LeakageEffects` must surface both production-grade implementations (wired into the agent runtime) and deterministic versions for the simulator so privacy tests can assert leakage bounds. Because the executor orchestrates snapshots, pure evaluation, and interpretation explicitly, no transport observable can occur unless the preceding guards succeed, preserving the semantics laid out in the theoretical model.

### 8.1 GuardSnapshot

The runtime prepares a `GuardSnapshot` immediately before entering the guard chain. It contains every stable datum a guard may inspect while remaining read-only.

```rust
pub struct GuardSnapshot {
    pub now: TimeStamp,
    pub caps: Cap,
    pub budgets: FlowBudgetView,
    pub metadata: MetadataView,
    pub rng_seed: [u8; 32],
}
```

Guards evaluate synchronously against this snapshot and the incoming request. They cannot mutate state or perform I/O. That keeps guard evaluation deterministic, replayable, and WASM-compatible.

### 8.2 EffectCommands

Guards do not execute side effects directly. Instead, they return `EffectCommand` items for the interpreter to run. Each command is a minimal, domain-agnostic description of work such as charging budgets or appending facts:

```rust
pub enum EffectCommand {
    ChargeBudget { authority: AuthorityId, amount: u32 },
    AppendJournal { entry: JournalEntry },
    RecordLeakage { bits: u32 },
    StoreMetadata { key: String, value: String },
    SendEnvelope { to: Address, envelope: Vec<u8> },
    GenerateNonce { bytes: usize },
}
```

This vocabulary keeps the guard interface simple: commands describe *what* happened, not *how*. Interpreters can batch, cache, or reorder commands so long as the semantics remain intact.

### 8.3 EffectInterpreter

The `EffectInterpreter` trait encapsulates the async execution of commands. Production runtimes hook it to `aura-effects` handlers, while the simulator or tests hook deterministic interpreters that record events instead of hitting the network.

```rust
#[async_trait]
pub trait EffectInterpreter {
    async fn exec(&self, cmd: EffectCommand) -> Result<EffectResult>;
}
```

`ProductionEffectInterpreter` performs real I/O (storage, transport, journal) and keeps connection to the handler registry. `SimulationEffectInterpreter` records deterministic `SimulationEvent`s, consumes simulated time, and replays guard commands during tests. Borrowed or mock interpreters simplify protocol-level unit testing.

### 8.4 Why This Matters

Pure guard evaluation over `GuardSnapshot` avoids blocking sync/async bridges, prevents WASM deadlocks, and ensures simulation/production share identical logic. Effects become algebraic data, making them observable, testable, and replayable across deterministic runs. This design lets the guard chain enforce authorization, flow budgets, leakage budgets, and journal coupling without leaking implementation details into protocol handlers.

## 9. Session Management and Choreography Execution

The effect system provides the framework for managing the lifecycle of distributed protocols. Choreographies define the logic of a protocol, while a **session** represents a single, stateful execution of that choreography. The runtime uses the effect system to create, manage, and execute these sessions.

### 9.1. The Session Management Interface

The abstract interface for all session-related operations is the `SessionManagementEffects` trait defined in `aura-core`. This trait provides the API for creating sessions, joining them, sending and receiving messages within a session's context, and querying their status.

```rust
// Defined in aura-core::effects::agent
pub trait SessionManagementEffects: Send + Sync {
    /// Create new choreographic session with participants and roles
    async fn create_choreographic_session(
        &self,
        session_type: SessionType,
        participants: Vec<ParticipantInfo>,
    ) -> Result<SessionId>;

    /// Send choreographic message within a session context
    async fn send_choreographic_message(
        &self,
        session_id: SessionId,
        message: Vec<u8>,
    ) -> Result<()>;

    // ... other methods for joining, leaving, and status checks
}
```

By abstracting session management into an effect, the application logic remains decoupled from the underlying implementation (e.g., in-memory vs. persistent session state).

### 9.2. Session Handlers and State

Concrete implementations of `SessionManagementEffects`, such as the `MemorySessionHandler` in `aura-protocol`, act as the engine for the session system. This handler maintains the state of all active sessions, including:
- **`SessionId`**: A unique identifier for the session instance.
- **`SessionStatus`**: The current phase of the session's lifecycle (e.g., `Initializing`, `Active`, `Completed`).
- **`SessionEpoch`**: A version number for the session's state. It is incremented to coordinate significant state changes and invalidate old credentials or messages, which is critical for security and preventing replay attacks.
- **Participants**: The list of devices involved in the choreography.

The creation and lifecycle of sessions are themselves managed as a choreographic protocol (`SessionLifecycleChoreography` in `aura-protocol`) to ensure consistency across all participants.

### 9.3. Execution Flow

The relationship between the runtime, effects, sessions, and choreographies is as follows:

1.  **Request**: An event triggers the need to execute a distributed protocol (e.g., FROST signing).
2.  **Session Creation**: The `aura-agent` runtime calls `create_choreographic_session` via the effect system. The handler creates a new session instance with a unique `SessionId` and an initial `SessionEpoch`.
3.  **Execution**: The session becomes the stateful context for executing the choreography. The agent uses the `SessionId` to route messages and drive the protocol's state machine as defined by its session type.
4.  **State Management**: The handler updates the `SessionStatus` as the choreography progresses. If the protocol requires it, the `SessionEpoch` can be incremented to securely evolve the session state.
5.  **Completion**: Once the choreography finishes, the handler transitions the session to a terminal state (`Completed` or `Failed`), and resources are cleaned up.

In essence, the session system is the generic, stateful **executor**, and a choreography is the specific, verifiable **script** that the executor runs.

## 10. Summary

The effect system provides abstract interfaces and concrete handlers. The runtime assembles these handlers into working systems. Context propagation ensures consistent execution. Lifecycle management coordinates initialization and shutdown. Crate boundaries enforce separation. Testing and simulation provide deterministic behavior. Performance optimizations improve scalability and responsiveness.
