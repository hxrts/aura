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

Callers select the domain appropriate to their semantics. Guards and transport use physical time. CRDT operations use logical clocks. Privacy-preserving ordering uses order tokens. Cross-domain comparisons are explicit via `TimeStamp::compare(policy)`.

Direct `SystemTime::now()` or chrono usage is forbidden outside effect implementations. The testkit and simulator provide deterministic handlers for all four traits.

### 1.2 When to Create Effect Traits

Create new effect traits when:
- Abstracting OS or external system integration (files, network, time)
- Defining domain-specific operations that multiple implementations might provide
- Isolating side effects for testing and simulation
- Enabling deterministic simulation of complex behaviors

### 1.3 When NOT to Create Effect Traits

Follow YAGNI (You Aren't Gonna Need It) principles. Defer abstraction when only one implementation exists. Avoid abstractions that add complexity without clear benefit. Do not abstract "just in case" without concrete need.

#### Threshold Signatures

Aura provides a unified `ThresholdSigningEffects` trait in `aura-core/src/effects/threshold.rs` for all threshold signing scenarios. This abstraction enables:

- **Multi-device personal signing** – User's own devices collaborating on threshold operations
- **Guardian recovery approvals** – Guardians assisting with account recovery
- **Group operation approvals** – Multi-party group decisions

The trait uses a unified `SigningContext` that pairs a `SignableOperation` (what is being signed) with an `ApprovalContext` (why the signature is requested). This design allows the same FROST signing machinery to handle all scenarios with proper audit/display context.

Key components:
- `ThresholdSigningEffects` trait – Async interface for bootstrap, sign, and query operations
- `ThresholdSigningService` in `aura-agent` – Production implementation using FROST
- `SigningContext`, `SignableOperation`, `ApprovalContext` – Context types in `aura-core/src/threshold/`
- `AppCore.sign_tree_op()` – High-level signing API returning `AttestedOp`

See [Cryptography](116_crypto.md) for the detailed threshold signature architecture.

#### Application-Specific Effect Traits

Application-specific effect traits (like `CliEffects`, `ConfigEffects`, `OutputEffects` in `aura-terminal`) should remain in their application layer (Layer 7). Do not move them to `aura-core` (Layer 1) when the traits compose core effects into application-specific operations. The same applies when only one implementation exists per application.

This follows proper layer separation. The `aura-core` crate provides infrastructure effects such as `ConsoleEffects`, `StorageEffects`, and `PhysicalTimeEffects`. Application layers compose these into domain-specific abstractions.

### 1.4 DatabaseEffects Organization

Database operations integrate consensus transparently through coordinated effect traits.

`JournalEffects` in `aura-core` provides `insert_fact()` for monotone operations (0 RTT) and `insert_relational_fact()` for cross-authority facts.

`DatabaseWriteEffects` in `aura-core` provides `transact()` which coordinates the CRDT vs Consensus path. It returns a `TransactionReceipt` indicating which coordination was used.

`DatabaseSubscriptionEffects` in `aura-core` provides `subscribe_query()` for reactive queries with isolation levels. It returns `Dynamic<T>` that updates on fact changes.

The `transact()` method routes operations by two orthogonal dimensions. The first is authority scope: single vs cross-authority. The second is agreement level: monotone (CRDT, 0 RTT) vs non-monotone (Consensus, 1-3 RTT).

This enables four coordination quadrants. Monotone with single authority uses direct fact insertion. Monotone with cross-authority uses CRDT merge via anti-entropy. Consensus with single authority uses single-authority consensus. Consensus with cross-authority uses federated consensus.

See [Database](113_database.md) and the reactive design document for details.

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

## 4. ReactiveEffects and Signal-Based State Management

The `ReactiveEffects` trait provides type-safe, signal-based state management for UI and inter-component communication. It enables FRP (Functional Reactive Programming) patterns where state changes automatically propagate to subscribers.

### 4.1 Signal<T> Type

Signals are phantom-typed identifiers that reference reactive state:

```rust
pub struct Signal<T> {
    id: SignalId,
    _phantom: PhantomData<T>,
}

// Define application signals
pub static CHAT_SIGNAL: LazyLock<Signal<ChatState>> =
    LazyLock::new(|| Signal::new("app:chat"));
pub static CONNECTION_STATUS_SIGNAL: LazyLock<Signal<ConnectionStatus>> =
    LazyLock::new(|| Signal::new("app:connection_status"));
```

The phantom type `T` ensures type safety at compile time. The `SignalId` is a string identifier used for runtime signal lookup.

### 4.2 ReactiveEffects Trait

The trait defines four core operations:

```rust
#[async_trait]
pub trait ReactiveEffects: Send + Sync {
    /// Read the current value of a signal
    async fn read<T>(&self, signal: &Signal<T>) -> Result<T, ReactiveError>
    where T: Clone + Send + Sync + 'static;

    /// Emit a new value to a signal
    async fn emit<T>(&self, signal: &Signal<T>, value: T) -> Result<(), ReactiveError>
    where T: Clone + Send + Sync + 'static;

    /// Subscribe to signal changes
    fn subscribe<T>(&self, signal: &Signal<T>) -> SignalStream<T>
    where T: Clone + Send + Sync + 'static;

    /// Register a new signal with an initial value
    async fn register<T>(&self, signal: &Signal<T>, initial: T) -> Result<(), ReactiveError>
    where T: Clone + Send + Sync + 'static;
}
```

### 4.3 Usage Pattern

The typical usage pattern follows Intent → ViewState → Signal → UI:

```rust
// 1. Register signals on startup (in AppCore::init_signals)
app.register(&*CHAT_SIGNAL, ChatState::default()).await?;

// 2. UI reads current state from ViewState
let chat = app_core.views().snapshot().chat;

// 3. Intent handler updates ViewState (auto-forwards to signals)
app_core.add_chat_message(new_message);
// Signal forwarding automatically emits to CHAT_SIGNAL

// 4. UI automatically receives updates via subscription
let mut stream = app_core.subscribe(&*CHAT_SIGNAL);
while let Ok(state) = stream.recv().await {
    render_chat_view(&state);
}
```

**Important**: Domain signals (CHAT_SIGNAL, CONTACTS_SIGNAL, etc.) are driven by the `ReactiveScheduler` in `aura-agent/src/reactive/`. Journal facts committed to the runtime are published to the scheduler, which batches them and updates registered signal views. The signal views (`ChatSignalView`, `ContactsSignalView`, `InvitationsSignalView`) process facts and emit full state snapshots to their respective signals.

### 4.4 Implementation

The `ReactiveHandler` in `aura-effects` implements `ReactiveEffects` using a `SignalGraph`:

- **SignalGraph**: Manages signal storage, type-erased values, and broadcast channels
- **AnyValue**: Type-erased wrapper using `Arc<dyn Any>` for runtime type storage
- **Broadcast Channels**: Each signal has a `broadcast::Sender<AnyValue>` for notifying subscribers

The handler is thread-safe via `Arc` and `RwLock`. Multiple handlers can share the same underlying graph.

### 4.5 Error Handling

`ReactiveError` covers common failure modes:

```rust
pub enum ReactiveError {
    SignalNotFound { id: String },
    TypeMismatch { id: String, expected: String, actual: String },
    SubscriptionClosed { id: String },
    Internal { reason: String },
}
```

Signal operations return `Result<T, ReactiveError>` for explicit error handling.

## 5. QueryEffects and Unified Handler

The `QueryEffects` trait provides typed Datalog queries with capability-based authorization. Combined with `ReactiveEffects`, it enables query-bound signals that automatically update when underlying facts change.

### 5.1 Query Trait

Queries implement the `Query` trait which defines typed access to journal facts:

```rust
pub trait Query: Send + Sync + Clone + 'static {
    type Result: Clone + Send + Sync + Default + 'static;

    /// Convert query to Datalog program
    fn to_datalog(&self) -> DatalogProgram;

    /// Required capabilities for this query
    fn required_capabilities(&self) -> Vec<QueryCapability>;

    /// Fact predicates this query depends on (for invalidation)
    fn dependencies(&self) -> Vec<FactPredicate>;

    /// Parse Datalog bindings into typed result
    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError>;

    /// Unique ID for this query instance
    fn query_id(&self) -> String;
}
```

### 5.2 QueryEffects Trait

The trait defines query operations with authorization:

```rust
#[async_trait]
pub trait QueryEffects: Send + Sync {
    /// Execute a typed query
    async fn query<Q: Query>(&self, query: &Q) -> Result<Q::Result, QueryError>;

    /// Execute raw Datalog program
    async fn query_raw(&self, program: &DatalogProgram) -> Result<DatalogBindings, QueryError>;

    /// Subscribe to query results (live updates)
    fn subscribe<Q: Query>(&self, query: &Q) -> QuerySubscription<Q::Result>;

    /// Check authorization capabilities
    async fn check_capabilities(&self, caps: &[QueryCapability]) -> Result<(), QueryError>;

    /// Invalidate queries affected by fact changes
    async fn invalidate(&self, predicate: &FactPredicate);

    /// Execute with specific isolation level
    async fn query_with_isolation<Q: Query>(
        &self, query: &Q, isolation: QueryIsolation,
    ) -> Result<Q::Result, QueryError>;

    /// Execute and return statistics
    async fn query_with_stats<Q: Query>(
        &self, query: &Q,
    ) -> Result<(Q::Result, QueryStats), QueryError>;
}
```

### 5.2.1 Query Isolation Levels

`QueryIsolation` specifies consistency requirements:

- **ReadUncommitted**: Sees all facts including uncommitted CRDT state (fastest)
- **ReadCommitted**: Waits for specified consensus instances before querying
- **Snapshot**: Time-travel query against historical prestate
- **ReadLatest**: Waits for all pending consensus in scope

### 5.2.2 Query Statistics

`QueryStats` provides execution metrics for debugging and optimization:

```rust
pub struct QueryStats {
    pub execution_time: Duration,
    pub facts_scanned: usize,
    pub facts_matched: usize,
    pub cache_hit: bool,
    pub isolation_used: QueryIsolation,
}
```

See [Database Architecture](113_database.md) for complete query system documentation.

### 5.3 BoundSignal<Q>

A `BoundSignal` pairs a signal with its source query:

```rust
pub struct BoundSignal<Q: Query> {
    signal: Signal<Q::Result>,
    query: Q,
}

impl<Q: Query> BoundSignal<Q> {
    /// Register with a reactive handler
    pub async fn register<R: ReactiveEffects>(&self, handler: &R) -> Result<(), ReactiveError> {
        handler.register_query(&self.signal, self.query.clone()).await
    }

    /// Get fact dependencies for invalidation
    pub fn dependencies(&self) -> Vec<FactPredicate> {
        self.query.dependencies()
    }
}
```

### 5.4 UnifiedHandler

The `UnifiedHandler` composes Query + Reactive effects into a single cohesive handler:

```rust
pub struct UnifiedHandler {
    query: QueryHandler,
    reactive: Arc<ReactiveHandler>,
    capability_context: Option<Vec<u8>>,
}

impl UnifiedHandler {
    /// Commit a fact and invalidate affected queries
    pub async fn commit_fact(&self, predicate: &str, args: Vec<String>) {
        self.query.add_fact(predicate, args).await;
        let fact_pred = FactPredicate::new(predicate);
        self.query.invalidate(&fact_pred).await;
    }

    /// Execute authorized query
    pub async fn query<Q: Query>(&self, query: &Q) -> Result<Q::Result, QueryError> {
        if self.capability_context.is_some() {
            self.query.check_capabilities(&query.required_capabilities()).await?;
        }
        self.query.query(query).await
    }
}
```

### 5.5 Query-Signal Integration

The architecture enables automatic signal updates when facts change:

```
Intent → Fact Commit → FactPredicate → Query Invalidation → Signal Emit → UI Update
```

Application signals are bound to queries at initialization:

```rust
// In signal_defs.rs
pub static CHAT_SIGNAL: LazyLock<Signal<ChatState>> =
    LazyLock::new(|| Signal::new("app:chat"));

// Bind signal to query
pub async fn register_app_signals_with_queries<R: ReactiveEffects>(
    handler: &R,
) -> Result<(), ReactiveError> {
    handler.register_query(&*CHAT_SIGNAL, ChatQuery::default()).await?;
    handler.register_query(&*INVITATIONS_SIGNAL, InvitationsQuery::default()).await?;
    // ...
    Ok(())
}
```

When facts are committed, they flow through the reactive scheduler:

```rust
// In RuntimeSystem (aura-agent)
// Facts are published to the scheduler via attach_fact_sink()
effect_system.attach_fact_sink(pipeline.fact_sender());

// The scheduler processes facts and updates signal views
// Each view emits full state snapshots to its signal
```

The `ReactiveScheduler` processes facts in batches (5ms window) and drives all signal updates. This eliminates the dual-write bug class where different signal sources could desync.

This enables TUI screens to subscribe and automatically receive updates:

```rust
// In terminal screen
let mut stream = app_core.subscribe(&*INVITATIONS_SIGNAL);
while let Ok(state) = stream.recv().await {
    // Automatically update UI when facts change
    render_invitations(&state);
}
```

## 6. Lifecycle Management

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

## 7. Layers and Crates

The effect system spans several crates. Each crate defines a specific role in the architecture. These crates maintain strict dependency boundaries.

`aura-core` defines effect traits, identifiers, and core data structures. It contains no implementations.

`aura-effects` contains stateless and single-party effect handlers. It provides default implementations for cryptography, storage, networking, and randomness.

`aura-protocol` contains orchestrated and multi-party behavior. It bridges session types to effect calls. It implements the [guard chain](109_authorization.md), journal coupling, and consensus integrations.

`aura-agent` assembles handlers into runnable systems. It configures effect pipelines for production environments.

`aura-simulator` provides deterministic execution. It implements simulated time, simulated networking, and controlled failure injection.

## 8. Testing and Simulation

The effect system supports deterministic testing. Mock handlers implement predictable behavior. A simulated runtime provides control over time and network behavior. The simulator exposes primitives to inject delays or failures.

Tests use deterministic time control. Tests use in-memory storage. Tests use mock network. These components allow protocol execution without side effects.

```rust
let system = TestRuntime::new()
    .with_mock_crypto()
    .with_deterministic_time()
    .build();
```

This snippet creates a test runtime. The runtime uses mock handlers for all effects. It provides deterministic time and network control.

## 9. Performance Considerations

Aura includes several performance optimizations. Parallel initialization reduces startup time. Caching handlers reduce repeated computation. Buffer pools reduce memory allocation.

The effect system avoids OS threads for WASM compatibility. It uses async tasks and cooperative scheduling. Lazy initialization creates handlers on first use.

```rust
let builder = EffectSystemBuilder::new()
    .with_handler(Arc::new(RealCryptoHandler))
    .with_parallel_init();
```

This snippet shows parallel initialization of handlers. Parallel initialization increases startup throughput.

## 10. Guard Chain and Leakage Integration

The effect runtime enforces the guard-chain sequencing defined in [Authorization](109_authorization.md) and the leakage contract from [Privacy and Information Flow](003_information_flow_contract.md) using pure guard evaluation plus asynchronous interpretation. Each projected choreography message expands to:

1. **Snapshot preparation (async)** – gather capability frontier, budget headroom, leakage metadata, and randomness into a `GuardSnapshot` via `AuthorizationEffects`, `FlowBudgetEffects`, and cache state.
2. **Pure guard evaluation (sync)** – `CapGuard → FlowGuard → JournalCoupler` runs over the snapshot and request, producing a `GuardOutcome` that describes the authorization decision plus the `Vec<EffectCommand>` commands that need to execute next.
3. **Command interpretation (async)** – an `EffectInterpreter` executes each `EffectCommand` using `FlowBudgetEffects`, `LeakageEffects`, `JournalEffects`, and `TransportEffects`, preserving charge-before-send.

Handlers that implement `LeakageEffects` must surface both production-grade implementations (wired into the agent runtime) and deterministic versions for the simulator so privacy tests can assert leakage bounds. Because the executor orchestrates snapshots, pure evaluation, and interpretation explicitly, no transport observable can occur unless the preceding guards succeed, preserving the semantics laid out in the theoretical model.

### 10.1 GuardSnapshot

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

### 10.2 EffectCommands

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

### 10.3 EffectInterpreter

The `EffectInterpreter` trait encapsulates the async execution of commands. Production runtimes hook it to `aura-effects` handlers, while the simulator or tests hook deterministic interpreters that record events instead of hitting the network.

```rust
#[async_trait]
pub trait EffectInterpreter {
    async fn exec(&self, cmd: EffectCommand) -> Result<EffectResult>;
}
```

`ProductionEffectInterpreter` performs real I/O (storage, transport, journal) and keeps connection to the handler registry. `SimulationEffectInterpreter` records deterministic `SimulationEvent`s, consumes simulated time, and replays guard commands during tests. Borrowed or mock interpreters simplify protocol-level unit testing.

### 10.4 Why This Matters

Pure guard evaluation over `GuardSnapshot` avoids blocking sync/async bridges, prevents WASM deadlocks, and ensures simulation/production share identical logic. Effects become algebraic data, making them observable, testable, and replayable across deterministic runs. This design lets the guard chain enforce authorization, flow budgets, leakage budgets, and journal coupling without leaking implementation details into protocol handlers.

## 11. Handler Service Pattern

The runtime exposes domain handlers as services through `AuraAgent`. Each handler becomes a service with a public API. Services share `AuraEffectSystem`, `AuthorityContext`, and `HandlerContext`.

```rust
impl AuraAgent {
    pub fn sessions(&self) -> &SessionService { ... }
    pub fn auth(&self) -> &AuthService { ... }
    pub fn invitations(&self) -> &InvitationService { ... }
    pub fn recovery(&self) -> &RecoveryService { ... }
}
```

This code shows the service accessor pattern. Each service provides domain-specific operations while delegating to the shared effect system for execution.

### 11.1 Service Registry

The `ServiceRegistry` initializes all services during agent startup. It holds references to each service and manages their lifecycle.

```rust
pub struct ServiceRegistry {
    sessions: Arc<SessionService>,
    auth: Arc<AuthService>,
    invitations: Arc<InvitationService>,
    recovery: Arc<RecoveryService>,
}
```

Services register with the `LifecycleManager` for initialization and shutdown coordination. The lifecycle manager executes initialization in dependency order and shutdown in reverse order.

### 11.2 Guard Chain Integration

All service operations use the guard chain pattern. Requests flow through capability, flow budget, and journal coupling guards before reaching the handler.

```
Request → CapGuard → FlowGuard → JournalCoupler → Handler → Response
                                        │
                                        ▼
                               Fact Journaling
```

This diagram shows the request flow through the guard chain. The guard chain enforces authorization, budgets, and journaling for every operation. See [System Architecture](001_system_architecture.md) for guard chain details.

## 12. Session Management and Choreography Execution

The effect system provides the framework for managing the lifecycle of distributed protocols. Choreographies define the logic of a protocol. A session represents a single, stateful execution of that choreography. The runtime uses the effect system to create, manage, and execute these sessions.

### 12.1 The Session Management Interface

The abstract interface for all session-related operations is the `SessionManagementEffects` trait defined in `aura-core`. This trait provides the API for creating sessions, joining them, sending and receiving messages, and querying their status.

```rust
pub trait SessionManagementEffects: Send + Sync {
    async fn create_choreographic_session(
        &self,
        session_type: SessionType,
        participants: Vec<ParticipantInfo>,
    ) -> Result<SessionId>;

    async fn send_choreographic_message(
        &self,
        session_id: SessionId,
        message: Vec<u8>,
    ) -> Result<()>;
}
```

This trait abstracts session management into an effect. The application logic remains decoupled from the underlying implementation such as in-memory or persistent session state.

### 12.2 Session Handlers and State

Concrete implementations of `SessionManagementEffects`, such as the `MemorySessionHandler` in `aura-protocol`, act as the engine for the session system. This handler maintains the state of all active sessions.

Each session has a `SessionId` for unique identification. It has a `SessionStatus` indicating the current phase (Initializing, Active, Completed). It has a `SessionEpoch` version number for coordinating state changes and invalidating old credentials. It has a list of participants involved in the choreography.

The creation and lifecycle of sessions are themselves managed as a choreographic protocol. The `SessionLifecycleChoreography` in `aura-protocol` ensures consistency across all participants.

### 12.3 Execution Flow

The relationship between the runtime, effects, sessions, and choreographies follows a defined sequence.

1. An event triggers the need to execute a distributed protocol such as FROST signing.
2. The `aura-agent` runtime calls `create_choreographic_session` via the effect system. The handler creates a new session instance with a unique `SessionId` and an initial `SessionEpoch`.
3. The session becomes the stateful context for executing the choreography. The agent uses the `SessionId` to route messages and drive the protocol state machine.
4. The handler updates the `SessionStatus` as the choreography progresses. If needed, the `SessionEpoch` can be incremented to securely evolve the session state.
5. Once the choreography finishes, the handler transitions the session to a terminal state (Completed or Failed) and resources are cleaned up.

The session system is a generic, stateful executor. A choreography is the specific, verifiable script that the executor runs.

## 13. Fact Registry Integration

The `FactRegistry` provides domain-specific fact type registration and reduction for the reactive scheduling system. It is integrated into the effect system via the `AuraEffectSystem` rather than being constructed separately.

### 13.1 Architecture

The `FactRegistry` lives in `aura-journal` and allows domain crates to register their fact types along with custom reducers. The registry is built during effect system initialization. It is made accessible through the effect system.

```rust
pub struct AuraEffectSystem {
    fact_registry: Arc<FactRegistry>,
}

impl AuraEffectSystem {
    pub fn fact_registry(&self) -> &FactRegistry {
        &self.fact_registry
    }
}
```

This code shows how `AuraEffectSystem` holds the registry. The `fact_registry()` method provides access to registered reducers.

### 13.2 Fact Registration

Domain crates register their fact types during effect system assembly. Each domain provides a type ID and a reducer function.

```rust
registry.register(
    "chat",
    ChatFact::type_id(),
    |facts| ChatFact::reduce(facts),
);
```

This code shows how `aura-chat` registers its fact type. Registered domains include Chat for message threading, Invitation for device invitations, Contact for relationship management, and Moderation for block/mute facts.

### 13.3 Reactive Scheduling

The `ReactiveScheduler` in `aura-agent` uses the `FactRegistry` to process domain facts. When facts arrive, the scheduler looks up the registered reducer for the domain. It applies the reducer to compute derived state. It then notifies reactive subscribers of state changes.

Production code obtains the registry via `effect_system.fact_registry()`. Tests may use `build_fact_registry()` for isolation.

### 13.4 Handler-Level Access

The `JournalHandler` holds an optional `FactRegistry` reference. This enables fact reduction during journal operations.

```rust
impl JournalHandler {
    pub fn with_fact_registry(mut self, registry: FactRegistry) -> Self {
        self.fact_registry = Some(registry);
        self
    }

    pub fn fact_registry(&self) -> Option<&FactRegistry> {
        self.fact_registry.as_ref()
    }
}
```

This code shows the handler-level integration. Journal operations can trigger domain-specific reductions when facts are committed.

### 13.5 Design Rationale

The registry is integrated at the effect system level, not the trait level. This avoids changes to the `JournalEffects` trait. Different runtime configurations can use different registries. Tests can construct isolated registries without the full effect system. Registry assembly stays in Layer 6 (runtime), not Layer 1 (core).

Protocol-level facts (Guardian, Recovery, Consensus, AMP) use the built-in reduction pipeline in `aura-journal/src/reduction.rs`. They do not require registry registration.

## 14. AppCore: Unified Frontend Interface

The `AppCore` in `aura-app` provides a unified interface for all frontend platforms. It wraps the `AuraAgent` and provides a clean API that hides the complexity of the effect system from UI code.

### 14.1 Architecture

AppCore sits between frontends (TUI, CLI, iOS, Android, Web) and the agent runtime:

```
┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
│     TUI     │  │     CLI     │  │     iOS     │  │     Web     │
└──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘
       │                │                │                │
       └────────────────┴────────────────┴────────────────┘
                               │
                               ↓
                   ┌───────────────────────┐
                   │       AppCore         │  ← aura-app (ONLY frontend interface)
                   │                       │
                   │  • ViewState signals  │
                   │  • Intent dispatch    │
                   │  • Service operations │
                   └───────────┬───────────┘
                               │
                               ↓ (internal, hidden from frontends)
                   ┌───────────────────────┐
                   │      AuraAgent        │  ← aura-agent (runtime)
                   │                       │
                   │  • Effect system      │
                   │  • Service handlers   │
                   └───────────────────────┘
```

Frontends import only from `aura-app`, never from `aura-agent` directly. This maintains proper layer boundaries.

### 14.2 Construction Modes

AppCore supports two construction modes for different use cases:

```rust
// Demo/Offline mode - local state only, no network
let app = AppCore::new(config)?;

// Production mode - with agent for full functionality
let agent = AgentBuilder::new()
    .with_config(agent_config)
    .with_authority(authority_id)
    .build_production()
    .await?;
let app = AppCore::with_agent(config, agent)?;
```

Demo mode enables offline development and testing. Production mode provides full effect system capabilities.

### 14.3 Push-Based Reactive Flow

All state changes flow through the reactive pipeline:

```
Local Intent ───┐
                │
Service Result ─┼──► Fact ──► Journal ──► Reduce ──► ViewState
                │                                      │
Remote Sync ────┘                                      ↓
                                               Signal<T> ──► UI
                                               (push, no poll)
```

Services emit facts, they never directly mutate ViewState. UI subscribes to signals using `signal.for_each()`. This preserves push semantics and avoids polling.

### 14.4 Accessing the Agent

When AppCore has an agent, it provides access to the full effect system:

```rust
// Check if agent is available
if app.has_agent() {
    // Get agent reference
    let agent = app.agent().unwrap();

    // Access effect system directly (no lock needed)
    let effects = agent.runtime().effects();

    // Use effects
    let time = effects.physical_time().await?;
}
```

The effect system uses `Arc<AuraEffectSystem>` for shared access. The effect system is immutable after construction; individual handlers manage their own internal state as needed.

### 14.5 Re-exports

`aura-app` re-exports types from `aura-agent` so frontends don't need direct dependencies:

```rust
// Agent types
pub use aura_agent::{AgentBuilder, AgentConfig, AuraAgent, AuraEffectSystem, EffectContext};

// Service types
pub use aura_agent::{SyncManagerConfig, SyncServiceManager, ...};

// Reactive types
pub use aura_agent::reactive::{Dynamic, FactSource, ReactiveScheduler, ...};
```

## 15. Service Pattern for Domain Crates

Domain crates (Layer 5) define stateless handlers that take effect references per-call.
The agent layer (Layer 6) wraps these with services that manage RwLock access.

### 15.1 Handler Layer (Domain Crates)

Handlers in `aura-chat`, `aura-invitation`, etc. are stateless and take `&E` per method:

```rust
// aura-chat/src/service.rs
pub struct ChatHandler;

impl ChatHandler {
    pub fn new() -> Self { Self }

    pub async fn create_group<E>(
        &self,
        effects: &E,  // <-- Per-call reference
        name: &str,
        creator_id: AuthorityId,
        initial_members: Vec<AuthorityId>,
    ) -> Result<ChatGroup>
    where
        E: StorageEffects + RandomEffects + PhysicalTimeEffects
    {
        let uuid = effects.random_uuid().await;
        // ...
    }
}
```

### 15.2 Service Layer (Agent)

Services in `aura-agent` wrap handlers with effect system access:

```rust
// aura-agent/src/handlers/chat_service.rs
pub struct ChatService {
    handler: ChatHandler,
    effects: Arc<AuraEffectSystem>,
}

impl ChatService {
    pub fn new(effects: Arc<AuraEffectSystem>) -> Self {
        Self {
            handler: ChatHandler::new(),
            effects,
        }
    }

    pub async fn create_group(
        &self,
        name: &str,
        creator_id: AuthorityId,
        initial_members: Vec<AuthorityId>,
    ) -> AgentResult<ChatGroup> {
        self.handler
            .create_group(&*self.effects, name, creator_id, initial_members)
            .await
            .map_err(Into::into)
    }
}
```

### 15.3 Agent API

The agent exposes services through clean accessor methods:

```rust
// aura-agent/src/core/api.rs
impl AuraAgent {
    pub fn chat(&self) -> ChatService {
        ChatService::new(self.runtime.effects())
    }

    pub async fn invitations(&self) -> AgentResult<InvitationService> {
        // Lazy initialization with caching
        InvitationService::new(self.runtime.effects(), self.context.clone())
    }
}
```

### 15.4 Benefits

This pattern keeps domain crates:

- **Pure**: No tokio dependency
- **Testable**: Pass mock effects directly in unit tests
- **Consistent**: Same pattern across all domain crates

The agent layer provides:

- **Shared access**: Effect system shared via `Arc<AuraEffectSystem>`
- **Error normalization**: Convert domain errors to `AgentError`
- **Factory methods**: Services created on-demand with no lazy-init overhead

### 15.5 When to Use

| Scenario | Location |
|----------|----------|
| Domain service logic | Domain crate `*Handler` (e.g., `aura-chat::ChatHandler`) |
| Agent service wrapper | `aura-agent/src/handlers/*_service.rs` |
| Agent API accessor | `aura-agent/src/core/api.rs` |

## 16. Summary

The effect system provides abstract interfaces and concrete handlers. The runtime assembles these handlers into working systems as services accessible through `AuraAgent`. Domain crates define stateless handlers that take effect references per-call, while the agent layer wraps these with services that provide shared access via `Arc<AuraEffectSystem>`. `AppCore` wraps the agent to provide a unified, platform-agnostic interface for all frontends. The `ReactiveScheduler` processes journal facts and drives UI signal updates. Context propagation ensures consistent execution. Lifecycle management coordinates initialization and shutdown. Crate boundaries enforce separation. Testing and simulation provide deterministic behavior.
