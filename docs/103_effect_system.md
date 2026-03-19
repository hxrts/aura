# Effect System

## Overview

Aura uses algebraic effects to abstract system capabilities. Effect traits define abstract interfaces for cryptography, storage, networking, time, and randomness. Handlers implement these traits with concrete behavior. Context propagation ensures consistent execution across async boundaries.

This document covers effect trait design, handler patterns, and the context model. See [Runtime](104_runtime.md) for lifecycle management, service composition, and guard chain execution.
See [Ownership Model](122_ownership_model.md) for the repo-wide
`Pure`/`MoveOwned`/`ActorOwned`/`Observed` taxonomy.

The `aura-agent` runtime uses structured concurrency with explicit session ownership.
Session-bound effects execute only under the current owner via canonical ingress.
For complete details on async ownership, session ownership, typed runtime errors,
and instrumentation policy, see `crates/aura-agent/ARCHITECTURE.md`.

The runtime contract is intentionally split:

- actor services supervise long-lived runtime structure
- move semantics govern session and endpoint ownership

Effect execution that touches session state belongs to the second category, not the first.

## Ownership At Effect Boundaries

Effect traits sit at an ownership boundary and should preserve the repo-wide
ownership model rather than hide it.

- Effect trait definitions in `aura-core` are primarily `Pure`.
- Long-lived mutable async ownership belongs to `ActorOwned` runtime services,
  not to effect trait definitions or ad hoc handler-local state.
- Exclusive authority transfer belongs to `MoveOwned` handles, owner tokens, or
  transfer records rather than shared mutable rewrites.
- Effect handlers may implement capabilities, but parity-critical mutation and
  publication should remain capability-gated in the exposed API shape.

Practical implications:

- define trait methods so callers can preserve typed ownership and typed failure
- do not use effect handlers as a loophole for bypassing authority checks
- do not let observation-facing layers gain semantic mutation power through
  convenience helpers
- ensure long-running effect-driven flows report typed terminal outcomes rather
  than implicit success or silent hangs
- prefer the canonical `aura-core` ownership vocabulary at effect-facing
  boundaries:
  - `OperationContext` for move-owned workflow ownership
  - exact progress/terminal publication wrappers plus consumed
    `TerminalPublisher` for lifecycle
  - `OwnedTaskSpawner`, `OwnedShutdownToken`, and `BoundedActorIngress` for
    actor-owned task and ingress boundaries

## Effect Traits

Aura defines effect traits as abstract interfaces for system capabilities. Core traits expose essential functionality. Extended traits expose optional operations and coordinated behaviors. Each trait is independent and does not assume global state.

Core traits include `CryptoCoreEffects`, `NetworkCoreEffects`, `StorageCoreEffects`, time domain traits, `RandomCoreEffects`, and `JournalEffects`. Extended traits include `CryptoExtendedEffects`, `NetworkExtendedEffects`, `StorageExtendedEffects`, `RandomExtendedEffects`, and system-level traits such as `SystemEffects` and `ChoreographicEffects`.

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

This example shows a core effect trait for cryptographic operations. Traits contain async methods for compatibility with async runtimes. Extension traits add optional capabilities without forcing all handlers to implement them. The `hash()` function is intentionally pure in `aura-core::hash` rather than an effect because it is deterministic and side-effect-free.

## Time Traits

The legacy monolithic `TimeEffects` trait is replaced by domain-specific traits. `PhysicalTimeEffects` returns wall-clock time with uncertainty and provides sleep operations. `LogicalClockEffects` advances and reads causal vector clocks and Lamport scalars. `OrderClockEffects` produces opaque total order tokens without temporal meaning.

Callers select the domain appropriate to their semantics. Guards and transport use physical time. CRDT operations use logical clocks. Privacy-preserving ordering uses order tokens.

Cross-domain comparisons are explicit via `TimeStamp::compare(policy)`. Total ordering across domains must use `OrderTime` or consensus sequencing. Direct `SystemTime::now()` or chrono usage is forbidden outside effect implementations.

### Timeout And Backoff Guidance

Wall clock time is a local choice in Aura. It is appropriate for:

- local owner deadlines
- retry and backoff policy
- expiration and cooldowns
- coordination with external systems

Wall clock time is not, by itself, distributed semantic truth.

Rules:

- use `PhysicalTimeEffects` for local timeout budgeting and retry policy
- use logical, order, or provenanced time when the concern is semantic
  ordering, causality, or attestation
- do not treat a local timeout as proof of protocol completion, causal order,
  or consensus finality
- nested workflows should consume remaining timeout budget rather than stacking
  unrelated per-stage wall-clock literals
- parity-critical timeout handling should surface typed timeout failure instead
  of silent hangs or ad hoc `tokio::time::timeout` wrappers at call sites

The shared timeout/backoff vocabulary lives in `aura-core::time::timeout` and
should be preferred over duplicated timeout arithmetic or raw sleep loops.
Parity-critical workflow APIs should take `OperationTimeoutBudget` or narrower
budget/policy types rather than raw `Duration`.

## Threshold Signing

Aura provides a unified `ThresholdSigningEffects` trait in `aura-core/src/effects/threshold.rs` for all threshold signing scenarios. The trait supports multi-device personal signing, guardian recovery approvals, and group operation approvals.

The trait uses a unified `SigningContext` that pairs a `SignableOperation` with an `ApprovalContext`. This design allows the same FROST signing machinery to handle all scenarios with proper audit context. The `ThresholdSigningService` in `aura-agent` provides the production implementation.

Key components include `ThresholdSigningEffects` for async signing operations, lifecycle traits for provisional and consensus modes, and `AppCore.sign_tree_op()` for high-level signing. See [Cryptography](100_crypto.md) for detailed threshold signature architecture.

## When to Create Effect Traits

Create new effect traits when abstracting OS or external system integration. Use them when defining domain-specific operations that multiple implementations might provide. They isolate side effects for testing and simulation. They enable deterministic simulation of complex behaviors.

Follow YAGNI principles. Defer abstraction when only one implementation exists. Avoid abstractions that add complexity without clear benefit. Do not abstract without concrete need.

Application-specific effect traits should remain in their application layer. Do not move `CliEffects` or `ConfigEffects` from `aura-terminal` to `aura-core` when only one implementation exists. The `aura-core` crate provides infrastructure effects. Application layers compose these into domain-specific abstractions.

## Database Effects

Database operations use existing effect traits rather than a dedicated `DatabaseEffects` layer. `JournalEffects` in `aura-core` provides fact insertion for monotone operations. Non-monotone operations use `aura-consensus` protocols driven by session types and the guard chain.

Reactive queries are handled via `QueryEffects` and `ReactiveEffects`. The coordination pattern follows two orthogonal dimensions described in [Database Architecture](107_database.md). Authority scope determines single versus cross-authority operations. Agreement level determines monotone versus consensus operations.

## Handler Design

Effect handlers implement effect traits. Stateless handlers execute operations without internal state. Stateful handlers coordinate multiple effects or maintain internal caches. Typed handlers implement concrete effect traits. Type-erased handlers allow dynamic dispatch through the effect executor.

Handlers do not store global state. All required inputs flow through method parameters. This avoids hidden dependencies and enables deterministic testing.

## Unified Encrypted Storage

Aura uses `StorageEffects` as the single persistence interface in application code. The production runtime wires `StorageEffects` through a unified encryption-at-rest wrapper. `FilesystemStorageHandler` provides raw bytes persistence. `RealSecureStorageHandler` uses Keychain or TPM for master-key persistence.

`EncryptedStorage` implements `StorageEffects` by encrypting and decrypting transparently. It generates or loads the master key on first use. Runtime assembly remains synchronous.

```rust
use aura_effects::{
    EncryptedStorage, EncryptedStorageConfig, FilesystemStorageHandler,
    RealCryptoHandler, RealSecureStorageHandler,
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

This example shows the encryption wrapper assembly. `RealCryptoHandler` lives in `aura-effects` and implements `CryptoCoreEffects`. Storage configuration controls encryption enablement and opaque naming. Application code uses `StorageEffects` without knowledge of encryption details.

## Context Model

The effect system propagates an `EffectContext` through async tasks. The context carries authority identity, context scope, session identification, execution mode, and metadata. No ambient state exists.

```rust
pub struct EffectContext {
    authority_id: AuthorityId,
    context_id: ContextId,
    session_id: SessionId,
    execution_mode: ExecutionMode,
    metadata: HashMap<String, String>,
}
```

This structure defines the operation-scoped effect context. The context flows through all effect calls and identifies which authority, context, and session the operation belongs to. The `execution_mode` controls handler selection for production versus test environments.

Context propagation uses scoped execution. A task local stores the current context. Nested tasks inherit the context. This ensures consistent behavior across async boundaries.

## ReactiveEffects Trait

The `ReactiveEffects` trait provides type-safe signal-based state management. Signals are phantom-typed identifiers that reference reactive state. The phantom type ensures compile-time type safety.

```rust
pub struct Signal<T> {
    id: SignalId,
    _phantom: PhantomData<T>,
}

#[async_trait]
pub trait ReactiveEffects: Send + Sync {
    async fn read<T>(&self, signal: &Signal<T>) -> Result<T, ReactiveError>
    where T: Clone + Send + Sync + 'static;

    async fn emit<T>(&self, signal: &Signal<T>, value: T) -> Result<(), ReactiveError>
    where T: Clone + Send + Sync + 'static;

    fn subscribe<T>(&self, signal: &Signal<T>) -> Result<SignalStream<T>, ReactiveError>
    where T: Clone + Send + Sync + 'static;

    async fn register<T>(&self, signal: &Signal<T>, initial: T) -> Result<(), ReactiveError>
    where T: Clone + Send + Sync + 'static;
}
```

The trait defines four core operations for reactive state. The `read` method returns the current value. The `emit` method updates the value. The `subscribe` method returns a stream of changes. The `register` method initializes a signal with a default value. See [Runtime](104_runtime.md) for reactive scheduling implementation. Subscribing an unregistered signal fails fast; Aura no longer permits "dead stream" subscription success for missing registrations.

## QueryEffects Trait

The `QueryEffects` trait provides typed Datalog queries with capability-based authorization. Queries implement the `Query` trait which defines typed access to journal facts.

```rust
pub trait Query: Send + Sync + Clone + 'static {
    type Result: Clone + Send + Sync + Default + 'static;

    fn to_datalog(&self) -> DatalogProgram;
    fn required_capabilities(&self) -> Vec<QueryCapability>;
    fn dependencies(&self) -> Vec<FactPredicate>;
    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError>;
    fn query_id(&self) -> String;
}

#[async_trait]
pub trait QueryEffects: Send + Sync {
    async fn query<Q: Query>(&self, query: &Q) -> Result<Q::Result, QueryError>;
    async fn query_raw(&self, program: &DatalogProgram) -> Result<DatalogBindings, QueryError>;
    fn subscribe<Q: Query>(&self, query: &Q) -> QuerySubscription<Q::Result>;
    async fn check_capabilities(&self, caps: &[QueryCapability]) -> Result<(), QueryError>;
    async fn invalidate(&self, predicate: &FactPredicate);
}
```

The `Query` trait converts queries to Datalog programs and defines capability requirements. The `QueryEffects` trait executes queries and manages subscriptions. Query isolation levels control consistency requirements. See [Database Architecture](107_database.md) for complete query system documentation.

## Determinism Rules

Effect boundaries determine native and WASM conformance parity. Protocol code must follow these rules to ensure deterministic execution.

The pure transition core requires identical outputs given the same input stream. No hidden state may affect observable behavior. All state must flow through explicit effect calls. Non-determinism is permitted only through explicit algebraic effects. Time comes from time traits. Randomness comes from `RandomEffects`. Storage comes from `StorageEffects`.

Conformance lanes compare logical steps rather than wall-clock timing. Tests must not depend on execution speed. Time-dependent behavior uses simulated time through effect handlers. Conformance artifacts use canonical encoding with deterministic field ordering.

## Session-Local VM Bridge Effects

Production choreography execution uses a narrow synchronous bridge trait at the Aura and Telltale boundary. `VmBridgeEffects` in `aura-core` exposes only immediate session-local queue and snapshot operations. It does not expose async transport, storage, or journal methods.

This split exists because Telltale host callbacks are synchronous. The callback path may enqueue outbound payloads, record blocked receive edges, consume branch choices, and snapshot scheduler signals. It must not perform network I/O or journal work directly.

Async host work resumes outside the VM step boundary in Layer 6 runtime services. `vm_host_bridge` observes `VmBridgeEffects` state, performs transport and guard-chain work, and injects completed results back into the VM. This preserves deterministic VM progression while keeping Aura's runtime async.

`aura-agent` runtime code preserves this boundary through canonical ingress and explicit session ownership. Network callbacks, timers, and background tasks route typed session-ingress messages to the current local owner. Each active session has exactly one owner at any time.

That owner may be hosted by an actor, but the effect-routing rule is still ownership-based:
session-bound effects execute because the caller is the current owner, not merely because it runs inside a service actor.

The runtime must also distinguish owner identity from owner capability:

- owner identity identifies the current fragment/session owner
- owner capability authorizes specific session-bound effects within fragment scope

Both checks matter for effect routing, especially across delegation boundaries.

Parity-critical ownership boundaries should declare that split explicitly
through `aura-macros` rather than comments or naming convention alone:
- `#[semantic_owner(..., category = "move_owned")]` for move-owned workflow owners
- `#[actor_owned(..., category = "actor_owned")]` for long-lived async domains
- `#[capability_boundary(category = "capability_gated", ...)]` for mint/publication helpers

See `crates/aura-agent/ARCHITECTURE.md` for the complete ownership model.

## Layer Placement

The effect system spans several crates with strict dependency boundaries. `aura-core` defines effect traits, identifiers, and core data structures. It contains no implementations.

`aura-effects` contains stateless and single-party effect handlers. It provides default implementations for cryptography, storage, networking, and randomness. `aura-protocol` contains orchestrated and multi-party behavior. It bridges session types to effect calls.

`aura-agent` assembles handlers into runnable systems. It configures effect pipelines for production environments. `aura-simulator` provides deterministic execution with simulated time, networking, and controlled failure injection.

## Performance

Aura includes several performance optimizations. Parallel initialization reduces startup time. Caching handlers reduce repeated computation. Buffer pools reduce memory allocation. The effect system avoids OS threads for WASM compatibility.

```rust
let builder = EffectSystemBuilder::new()
    .with_handler(Arc::new(RealCryptoHandler))
    .with_parallel_init();
```

This snippet shows parallel initialization of handlers. The builder pattern allows flexible handler composition. Lazy initialization creates handlers on first use. Async tasks and cooperative scheduling provide efficient execution.

## Testing Support

The effect system supports deterministic testing through mock handlers. A simulated runtime provides control over time and network behavior. The simulator exposes primitives to inject delays or failures.

```rust
let system = TestRuntime::new()
    .with_mock_crypto()
    .with_deterministic_time()
    .build();
```

This snippet creates a test runtime with mock handlers for all effects. It provides deterministic time and network control. Tests use in-memory storage and mock networking to execute protocols without side effects. See [Test Infrastructure Reference](118_testkit.md) for test patterns.
