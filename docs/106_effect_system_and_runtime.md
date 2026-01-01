# Effect System and Runtime

This document describes the effect system and runtime architecture in Aura. It defines effect traits, handler design, context propagation, lifecycle management, and integration across crates. It also describes testing modes and performance considerations.

## 1. Effect Traits and Categories

Aura defines effect traits as abstract interfaces for system capabilities. Core traits expose essential functionality. Extended traits expose optional operations and coordinated or system-wide behaviors. Each trait is independent and does not assume global state.

Core traits include `CryptoCoreEffects`, `NetworkCoreEffects`, `StorageCoreEffects`, time domain traits (`PhysicalTimeEffects`, `LogicalClockEffects`, `OrderClockEffects`), `RandomCoreEffects`, and `JournalEffects`. Extended traits include `CryptoExtendedEffects`, `NetworkExtendedEffects`, `StorageExtendedEffects`, `RandomExtendedEffects`, plus system-level traits such as `SystemEffects`, `EffectApiEffects`, `ChoreographicEffects`, and `AgentEffects`. `TraceEffects` provides structured instrumentation as an infrastructure effect.

```rust
#[async_trait]
pub trait CryptoCoreEffects: RandomCoreEffects + Send + Sync {
    async fn ed25519_sign(
        &self,
        message: &[u8],
        private_key: &[u8],
    ) -> Result<Vec<u8>, CryptoError>;

    async fn ed25519_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError>;

    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: u32,
    ) -> Result<Vec<u8>, CryptoError>;
}
```

This example shows a core effect trait. Implementations provide cryptographic operations. Traits contain async methods for compatibility with async runtimes. Extension traits add optional capabilities without forcing all handlers to implement them. Note that `hash()` is intentionally a pure function in `aura-core::hash` rather than an effect, because it is deterministic and side-effect-free.

### 1.1 Unified Time Traits

The legacy monolithic `TimeEffects` trait is replaced by domain-specific traits:

- `PhysicalTimeEffects` – returns `PhysicalTime { ts_ms, uncertainty }` and `sleep_ms` for wall-clock operations.
- `LogicalClockEffects` – advances and reads causal vector clocks and Lamport scalars.
- `OrderClockEffects` – produces opaque, privacy-preserving total order tokens without temporal meaning.

Callers select the domain appropriate to their semantics. Guards and transport use physical time. CRDT operations use logical clocks. Privacy-preserving ordering uses order tokens. Cross-domain comparisons are explicit via `TimeStamp::compare(policy)`, but total ordering/indexing across domains must use `OrderTime` or consensus/session sequencing (never raw timestamps).

Direct `SystemTime::now()` or chrono usage is forbidden outside effect implementations. The testkit and simulator provide deterministic handlers for all three traits.

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
- Lifecycle traits in `aura-core/src/threshold/lifecycle.rs` – Provisional/Coordinator/Consensus/Rotation modes (fast paths + finalization)
- `AppCore.sign_tree_op()` – High-level signing API returning `AttestedOp`

See [Cryptography](116_crypto.md) for the detailed threshold signature architecture.

#### Application-Specific Effect Traits

Application-specific effect traits (like `CliEffects`, `ConfigEffects`, `OutputEffects` in `aura-terminal`) should remain in their application layer (Layer 7). Do not move them to `aura-core` (Layer 1) when the traits compose core effects into application-specific operations. The same applies when only one implementation exists per application.

This follows proper layer separation. The `aura-core` crate provides infrastructure effects such as `ConsoleEffects`, `StorageCoreEffects`, and `PhysicalTimeEffects` (plus `TraceEffects`). Application layers compose these into domain-specific abstractions.

### 1.4 DatabaseEffects Organization

Database operations use existing effect traits and orchestration crates rather than a dedicated DatabaseEffects layer.

`JournalEffects` in `aura-core` provides fact insertion for monotone operations (0 RTT) and relational facts for cross-authority contexts. Non-monotone operations use `aura-consensus` protocols (1–3 RTT), driven by session types and the guard chain. Reactive queries are handled via `QueryEffects` and `ReactiveEffects`.

The coordination pattern still follows the two orthogonal dimensions described in the database docs: authority scope (single vs cross-authority) and agreement level (monotone/CRDT vs consensus). The implementation lives in the journal, consensus, and sync layers rather than a unified `transact()` entry point.

See [Database](113_database.md) and the reactive design document for details.

## 2. Handler Design

Effect handlers implement effect traits. Stateless handlers execute operations without internal state. Stateful handlers coordinate multiple effects or maintain internal caches.

Typed handlers implement concrete effect traits. Type-erased handlers allow dynamic dispatch through the effect executor. Both designs share the same execution interface.

Handlers do not store global state. All required inputs flow through method parameters. This avoids hidden dependencies.

### 2.1 Unified Encrypted Storage (StorageEffects)

Aura uses `StorageEffects` as the *single* persistence interface in application code. The production runtime wires `StorageEffects` through a unified encryption-at-rest wrapper:

- `FilesystemStorageHandler` (raw bytes persistence)
- `RealSecureStorageHandler` (`SecureStorageEffects` for master-key persistence; Keychain/TPM/Keystore with a filesystem fallback during bring-up)
- `EncryptedStorage` (implements `StorageEffects` by encrypting/decrypting transparently)

`EncryptedStorage` generates or loads the master key on first use, so runtime assembly remains synchronous.

In `aura-agent`, the storage behavior is controlled by `StorageConfig`:
- `encryption_enabled` (default `true`; testing/bring-up only)
- `opaque_names` (default `false`; note that prefix-based listing is not meaningful without an index)

Example wiring (simplified):

```rust
use aura_effects::{
    EncryptedStorage, EncryptedStorageConfig, FilesystemStorageHandler, RealCryptoHandler,
    RealSecureStorageHandler,
};
use std::sync::Arc;

let secure = Arc::new(RealSecureStorageHandler::with_base_path(base_path.clone()));
let storage = EncryptedStorage::new(
    FilesystemStorageHandler::from_path(base_path.clone()),
    Arc::new(RealCryptoHandler::new()),
    secure,
    EncryptedStorageConfig::default(),
);
```

`RealCryptoHandler` lives in `aura-effects` and implements `CryptoCoreEffects`. Hashing stays a pure function (`aura_core::hash`) rather than an effect.

## 3. Context Model

The effect system propagates an `EffectContext` through async tasks. The context carries authority identity, context scope, session identification, execution mode, and metadata. The context is explicit. No ambient state exists.

```rust
/// From aura-core/src/context.rs
pub struct EffectContext {
    authority_id: AuthorityId,
    context_id: ContextId,
    session_id: SessionId,
    execution_mode: ExecutionMode,
    metadata: HashMap<String, String>,
}
```

This structure defines the operation-scoped effect context. The context flows through all effect calls. It identifies which authority, context, and session the operation belongs to. The `execution_mode` controls handler selection (Production vs Test). Metadata supports diagnostics and telemetry.

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

The typical usage pattern follows Fact → Scheduler → Signal → UI:

```rust
// 1. Register signals on startup (in AppCore::init_signals)
app.register(&*CHAT_SIGNAL, ChatState::default()).await?;

// 2. Commit a typed fact (production path)
// The fact is published to ReactiveScheduler which updates the signal
let fact = Fact::new(FactContent::Relational(chat_fact));
runtime.commit_fact(fact).await?;

// 3. UI reads current state from signal
let chat = app_core.read(&*CHAT_SIGNAL).await?;

// 4. UI subscribes for updates
let mut stream = app_core.subscribe(&*CHAT_SIGNAL);
while let Ok(state) = stream.recv().await {
    render_chat_view(&state);
}
```

Domain signals (CHAT_SIGNAL, CONTACTS_SIGNAL, RECOVERY_SIGNAL, etc.) are driven by the `ReactiveScheduler` in `aura-agent/src/reactive/`. Journal facts committed to the runtime are published to the scheduler, which batches them and updates registered signal views. The signal views (`ChatSignalView`, `ContactsSignalView`, `InvitationsSignalView`) process facts and emit full state snapshots to their respective signals.

For demo/test scenarios that don't have a full runtime, code can emit directly to signals via `ReactiveEffects::emit()`.

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
// Facts are published to the scheduler by wiring its sender into the effect system.
effect_system.attach_fact_sink(pipeline.fact_sender());

// The scheduler processes fact batches and updates signal views.
```

The `ReactiveScheduler` receives facts from multiple `FactSource` channels (journal commits, network receipts, timers). It batches them (5ms window) and drives all signal updates. This eliminates the dual-write bug class where different signal sources could desync.

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

Aura defines a lightweight lifecycle manager for initialization and shutdown. The current implementation primarily coordinates session cleanup timeouts and shutdown behavior.

```rust
pub struct LifecycleManager {
    session_cleanup_timeout: u64,
}
```

If richer component-aware lifecycle orchestration becomes necessary (init ordering, explicit phase transitions), it should be introduced via a dedicated design pass rather than assuming a more complex manager by default.

### 6.1 Runtime Maintenance Tasks

Long-lived runtimes must periodically prune caches and stale in-memory state. Aura handles this in **Layer 6** (runtime composition) via background maintenance tasks scheduled by the `RuntimeTaskRegistry`. Domain crates expose cleanup APIs, but **do not self-schedule**. The agent runtime wires these up during startup.

Example (conceptual):

```rust
// In aura-agent runtime builder
system.start_maintenance_tasks();

// Internally, maintenance tasks call:
// - sync_service.maintenance_cleanup(...)
// - ceremony_tracker.cleanup_timed_out()
```

This keeps time-based policy in the runtime layer, preserves deterministic testing (simulator controls time), and avoids leaking runtime coupling into Layer 4/5 crates.

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
    ChargeBudget {
        context: ContextId,
        authority: AuthorityId,
        peer: AuthorityId,
        amount: u32
    },
    AppendJournal { entry: JournalEntry },
    RecordLeakage { bits: u32 },
    StoreMetadata { key: String, value: String },
    SendEnvelope {
        to: NetworkAddress,
        peer_id: Option<uuid::Uuid>,
        envelope: Vec<u8>
    },
    GenerateNonce { bytes: usize },
}
```

This vocabulary keeps the guard interface simple: commands describe *what* happened, not *how*. Interpreters can batch, cache, or reorder commands so long as the semantics remain intact.

### 10.3 EffectInterpreter

The `EffectInterpreter` trait encapsulates the async execution of commands. Production runtimes hook it to `aura-effects` handlers, while the simulator or tests hook deterministic interpreters that record events instead of hitting the network. Implementations expose an `interpreter_type()` tag for diagnostics.

```rust
#[async_trait]
pub trait EffectInterpreter: Send + Sync {
    async fn execute(&self, cmd: EffectCommand) -> Result<EffectResult>;
    fn interpreter_type(&self) -> &'static str;
}
```

`ProductionEffectInterpreter` performs real I/O (storage, transport, journal) and keeps connection to the handler registry. `SimulationEffectInterpreter` records deterministic `SimulationEvent`s, consumes simulated time, and replays guard commands during tests. Borrowed or mock interpreters simplify protocol-level unit testing.

### 10.4 Why This Matters

Pure guard evaluation over `GuardSnapshot` avoids blocking sync/async bridges, prevents WASM deadlocks, and ensures simulation/production share identical logic. Effects become algebraic data, making them observable, testable, and replayable across deterministic runs. This design lets the guard chain enforce authorization, flow budgets, leakage budgets, and journal coupling without leaking implementation details into protocol handlers.

## 11. Handler Service Pattern

The runtime exposes domain handlers as services through `AuraAgent`. Each handler becomes a service with a public API. Services share `AuraEffectSystem`, `AuthorityContext`, and `HandlerContext`.

```rust
impl AuraAgent {
    pub fn sessions(&self) -> &SessionServiceApi { ... }
    pub fn auth(&self) -> &AuthServiceApi { ... }
    pub fn invitations(&self) -> &InvitationServiceApi { ... }
    pub fn recovery(&self) -> &RecoveryServiceApi { ... }
}
```

This code shows the service accessor pattern. Each service provides domain-specific operations while delegating to the shared effect system for execution.

### 11.1 Service Registry

The `ServiceRegistry` initializes all services during agent startup. It holds references to each service and wires shared runtime dependencies.

```rust
pub struct ServiceRegistry {
    sessions: Arc<SessionServiceApi>,
    auth: Arc<AuthServiceApi>,
    invitations: Arc<InvitationServiceApi>,
    recovery: Arc<RecoveryServiceApi>,
}
```

If lifecycle coordination is required, the runtime system owns it; services themselves remain simple wrappers around pure handler logic plus effect interpretation.

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

This code shows how `aura-chat` registers its fact type. Registered domains include Chat for message threading, Invitation for device invitations, Contact for relationship management, and Moderation for home/mute facts.

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

The `AppCore` in `aura-app` provides a unified, portable interface for all frontend platforms. It is headless and runtime-agnostic; it can run without a runtime bridge (offline/demo), or be wired to a concrete runtime via the `RuntimeBridge` trait. `aura-agent` is one such runtime implementation today; future runtimes (e.g. WASM-compatible) can also implement the bridge.

### 14.1 Architecture

AppCore sits between frontends (TUI, CLI, iOS, Android, Web) and a runtime bridge:

```
┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
│     TUI     │  │     CLI     │  │     iOS     │  │     Web     │
└──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘
       │                │                │                │
       └────────────────┴────────────────┴────────────────┘
                               │
                               ↓
                   ┌───────────────────────┐
                   │       AppCore         │  ← aura-app (portable frontend interface)
                   │                       │
                   │  • ViewState signals  │
                   │  • Intent dispatch    │
                   │  • Service operations │
                   └───────────┬───────────┘
                               │
                               ↓ (internal, hidden from frontends)
                   ┌───────────────────────┐
                   │  RuntimeBridge impl  │  ← aura-agent or other runtime
                   │                       │
                   │  • Effect system      │
                   │  • Service handlers   │
                   └───────────────────────┘
```

Frontends import UI-facing types from `aura-app` and may additionally depend on a runtime crate (such as `aura-agent`) to obtain a concrete `RuntimeBridge`. This keeps `aura-app` portable while allowing multiple runtime backends.

### 14.2 Construction Modes

AppCore supports two construction modes for different use cases:

```rust
// Demo/Offline mode - local state only, no network
let app = AppCore::new(config)?;

// Production mode - with a runtime bridge for full functionality
let agent = AgentBuilder::new()
    .with_config(agent_config)
    .with_authority(authority_id)
    .build_production()
    .await?;
let app = AppCore::with_runtime(config, agent.as_runtime_bridge())?;
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

When AppCore has a runtime, it provides access to runtime-backed operations:

```rust
// Check if runtime is available
if app.has_runtime() {
    let runtime = app.runtime().unwrap();
    let status = runtime.get_sync_status().await;
    println!("Sync status: {:?}", status);
}
```

The runtime bridge exposes async capabilities (sync, signing, protocols) while keeping `aura-app` decoupled from any specific runtime implementation.

### 14.5 Re-exports

`aura-app` does **not** re-export runtime types. Frontends import app-facing types from `aura-app`, and runtime types (e.g., `AuraAgent`, `AgentBuilder`) directly from `aura-agent` or another runtime crate.

## 15. Service Pattern for Domain Crates

Domain crates (Layer 5) define stateless handlers that take effect references per-call.
The agent layer (Layer 6) wraps these with services that manage RwLock access.

### 15.1 Handler Layer (Domain Crates)

Handlers in `aura-chat`, `aura-invitation`, etc. are stateless and return `GuardOutcome` values (pure plans describing effect commands) rather than performing I/O directly:

```rust
// aura-chat/src/fact_service.rs
pub struct ChatFactService;

impl ChatFactService {
    pub fn new() -> Self { Self }

    pub fn prepare_create_channel(
        &self,
        snapshot: &GuardSnapshot,
        channel_id: ChannelId,
        name: String,
        topic: Option<String>,
        is_dm: bool,
    ) -> GuardOutcome {
        // Pure evaluation returning effect commands.
        GuardOutcome::authorized(vec![
            EffectCommand::AppendJournal { entry: /* ... */ },
        ])
    }
}
```

### 15.2 Service Layer (Agent)

Services in `aura-agent` wrap handlers, run guard evaluation, and interpret commands:

```rust
// aura-agent/src/handlers/chat_service.rs
pub struct ChatServiceApi {
    handler: ChatFactService,
    effects: Arc<AuraEffectSystem>,
}

impl ChatServiceApi {
    pub fn new(effects: Arc<AuraEffectSystem>) -> Self {
        Self {
            handler: ChatFactService::new(),
            effects,
        }
    }

    pub async fn create_group(
        &self,
        name: &str,
        creator_id: AuthorityId,
        initial_members: Vec<AuthorityId>,
    ) -> AgentResult<ChatGroup> {
        let snapshot = /* gather GuardSnapshot via effects */;
        let outcome = self.handler.prepare_create_channel(
            &snapshot,
            ChannelId::new(),
            name.to_string(),
            None,
            false,
        );
        /* interpret GuardOutcome via effect interpreter */
        Ok(/* created group */)
    }
}
```

### 15.3 Agent API

The agent exposes services through clean accessor methods:

```rust
// aura-agent/src/core/agent.rs
impl AuraAgent {
    pub fn chat(&self) -> ChatServiceApi {
        ChatServiceApi::new(self.runtime.effects())
    }

    pub async fn invitations(&self) -> AgentResult<InvitationServiceApi> {
        // Lazy initialization with caching
        InvitationServiceApi::new(self.runtime.effects(), self.context.clone())
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
| Domain service logic | Domain crate `*FactService` (e.g., `aura-chat::ChatFactService`) |
| Agent service wrapper | `aura-agent/src/handlers/*_service.rs` |
| Agent API accessor | `aura-agent/src/core/api.rs` |

## 16. Summary

The effect system provides abstract interfaces and concrete handlers. The runtime assembles these handlers into working systems as services accessible through `AuraAgent`. Domain crates define stateless handlers that take effect references per-call, while the agent layer wraps these with services that provide shared access via `Arc<AuraEffectSystem>`. `AppCore` wraps the agent to provide a unified, platform-agnostic interface for all frontends. The `ReactiveScheduler` processes journal facts and drives UI signal updates. Context propagation ensures consistent execution. Lifecycle management coordinates initialization and shutdown. Crate boundaries enforce separation. Testing and simulation provide deterministic behavior.
