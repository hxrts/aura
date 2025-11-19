# Effect System and Runtime

This document describes the effect system and runtime architecture in Aura. It defines effect traits, handler design, context propagation, lifecycle management, and integration across crates. It also describes testing modes and performance considerations.

## 1. Effect Traits and Categories

Aura defines effect traits as abstract interfaces for system capabilities. Core traits expose essential functionality. Extended traits expose coordinated or system-wide behaviors. Each trait is independent and does not assume global state.

Core traits include `CryptoEffects`, `NetworkEffects`, `StorageEffects`, `TimeEffects`, `RandomEffects`, and `JournalEffects`. Extended traits include `SystemEffects`, `LedgerEffects`, `ChoreographicEffects`, and `AgentEffects`.

```rust
#[async_trait]
pub trait CryptoEffects {
    async fn hash(&self, data: &[u8]) -> [u8; 32];
    async fn hmac(&self, key: &[u8], data: &[u8]) -> [u8; 32];
}
```

This example shows a core effect trait. Implementations provide cryptographic operations. Traits contain async methods for compatibility with async runtimes.

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

`aura-protocol` contains orchestrated and multi-party behavior. It bridges session types to effect calls. It implements the guard chain, journal coupling, and consensus integrations.

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

The effect runtime enforces the guard-chain sequencing defined in `108_authorization_pipeline.md` and the leakage contract from `003_privacy_and_information_flow.md`. Each projected choreography message expands to the following effect calls:

1. **CapGuard / AuthorizationEffects** – evaluate Biscuit tokens plus sovereign policy to derive the capability frontier for the `(ContextId, peer)` pair.
2. **FlowGuard / FlowBudgetEffects** – atomically increment the replicated `spent` counter (stored as journal facts) and produce a receipt if the operation succeeds.
3. **LeakageEffects** – record observer-class leakage costs (`external`, `neighbor`, `group`) so privacy budgets remain monotone, as described in `001_theoretical_model.md`.
4. **JournalCoupler / JournalEffects** – merge protocol facts together with the FlowBudget charge to preserve the charge-before-send invariant.
5. **TransportEffects** – finally emit the encrypted packet over the secure channel.

Handlers that implement `LeakageEffects` must surface both production-grade implementations (wired into the agent runtime) and deterministic versions for the simulator so privacy tests can assert leakage bounds. Because the effect executor orchestrates these calls explicitly, no transport observable can occur unless the preceding guards succeed, preserving the semantics laid out in the theoretical model.

## 9. Summary

The effect system provides abstract interfaces and concrete handlers. The runtime assembles these handlers into working systems. Context propagation ensures consistent execution. Lifecycle management coordinates initialization and shutdown. Crate boundaries enforce separation. Testing and simulation provide deterministic behavior. Performance optimizations improve scalability and responsiveness.
