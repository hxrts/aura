# Unified Aura Effect System Architecture

This document explains Aura's unified effect system architecture that provides a single, elegant system for all effect operations across choreographic protocols, agent operations, and simulation testing.

## Overview

Aura uses a **unified effect system architecture** centered around the `AuraEffectSystem`. This system replaces the previous fragmented approach with a single, composable effect system that works consistently across all layers.

**Key Innovation**: Single `AuraEffectSystem` with optional middleware composition.

**Architecture Principles**:
- **Unified**: One effect system for all operations (choreography, agent, simulation)
- **Middleware-Optional**: Base system works directly; middleware adds enhancements when needed
- **Context-Driven**: Unified `AuraContext` flows through all operations
- **Mode-Aware**: Execution mode (Testing, Production, Simulation) drives behavior

## Algebraic Effect Theory & Terminology

To maintain conceptual clarity, Aura strictly adheres to algebraic effect terminology:

### Effects (Abstract Capabilities)
**Effects** are abstract capabilities or operations defined as trait interfaces:
```rust
#[async_trait]
pub trait CryptoEffects {
    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32];  // The "effect"
    async fn random_bytes(&self, len: usize) -> Vec<u8>;   // Another "effect"
}
```

Effects define **what** operations are available, not **how** they're implemented.

### Effect Handlers (Concrete Implementations)
**Effect Handlers** are concrete implementations that interpret abstract effects:
```rust
pub struct RealCryptoHandler { /* ... */ }
pub struct MockCryptoHandler { /* ... */ }
pub struct TestCryptoHandler { /* ... */ }

impl CryptoEffects for RealCryptoHandler {
    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        blake3::hash(data).into()  // Real implementation
    }
}

impl CryptoEffects for TestCryptoHandler {
    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        [0u8; 32]  // Deterministic test implementation
    }
}
```

Effect handlers define **how** abstract effects are realized in different contexts.

### Middleware (Cross-Cutting Decorators)
**Middleware** wraps effect handlers with cross-cutting concerns:
```rust
pub struct RetryMiddleware<H> { inner: H, max_attempts: u32 }
pub struct TracingMiddleware<H> { inner: H, service_name: String }

impl<H: CryptoEffects> CryptoEffects for RetryMiddleware<H> {
    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        for _ in 0..self.max_attempts {
            if let Ok(result) = self.inner.blake3_hash(data).await {
                return result;  // Retry logic wraps the inner handler
            }
        }
        panic!("All retry attempts failed")
    }
}
```

Middleware **decorates** handlers with additional behavior without changing the core effect semantics.

### Key Distinction
- **Effects**: Abstract interfaces (traits) - define capabilities
- **Effect Handlers**: Concrete implementations - provide behavior  
- **Middleware**: Decorators - enhance handlers with cross-cutting concerns

**Anti-Pattern**: Calling handlers "effect implementations" conflates the abstraction (effect) with its realization (handler).

### Core Effect Types

The unified system supports 18+ effect types organized by category:

| Category | Effect Type | Purpose |
|----------|-------------|---------|
| **Core** | Network | Peer communication, message routing, broadcast |
| | Crypto | BLAKE3 hashing, Ed25519 signatures, deterministic random |
| | Storage | Key-value persistence, namespacing, cross-platform compatibility |
| | Time | Epoch management, timeouts, current time retrieval |
| | Console | Structured logging, debugging, event emission |
| | Ledger | Account state management, CRDT operations |
| | Random | Cryptographic random byte generation |
| | Choreographic | Protocol-level distributed coordination |
| **Agent** | DeviceStorage | Secure device-specific storage with biometric protection |
| | Authentication | Device unlock, biometric authentication, session management |
| | Configuration | Device configuration management and persistence |
| | SessionManagement | Session lifecycle, token management, state tracking |
| **Simulation** | FaultInjection | Byzantine faults, network partitions, crash failures |
| | TimeControl | Time acceleration, pause/resume, checkpoint/restore |
| | StateInspection | State capture, diff analysis, change tracking |
| | PropertyChecking | Safety/liveness properties, temporal logic verification |
| | ChaosCoordination | Coordinated chaos experiments, scenario orchestration |

## Unified Architecture

### Implementation Note: Static Dispatch vs Effect Registry

**Note on Implementation**: While the logical architecture includes an "Effect Registry" concept for routing effect operations, the actual implementation uses Rust's **static trait dispatch** for zero-overhead performance. Effects are routed at compile-time via trait implementations rather than through runtime registry lookups.

This architectural trade-off provides:
- ✅ **Zero runtime overhead** - Full monomorphization and inlining
- ✅ **Type safety at compile time** - Catch errors before runtime
- ✅ **Excellent performance** - No vtable lookups or dynamic dispatch cost
- ⚠️ **Less runtime flexibility** - Cannot dynamically load effect handlers

**In Practice**:
```rust
// Conceptually: "Effect Registry" routing
effect_system.execute_effect(EffectType::Crypto, "hash", params)?;

// Actually: Static trait dispatch (zero overhead)
impl CryptoEffects for AuraEffectSystem {
    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        // Direct function call, fully inlined
    }
}
```

This means the "registry" is a logical concept in the architecture diagrams, but the implementation achieves the same routing behavior through Rust's trait system with zero runtime cost.

## File Structure and Architectural Layering

### Crate-Level Middleware Organization

Different crates organize middleware according to their architectural purpose:

**aura-protocol** (Protocol Infrastructure):
```
src/
├── middleware/          # Cross-cutting concerns
│   ├── observability/   # Tracing, metrics, monitoring
│   ├── resilience/      # Retry, timeout, circuit breaker
│   ├── security/        # Capabilities, authorization
│   └── caching/         # LRU cache, memoization
└── effects/
    └── system.rs        # Core AuraEffectSystem
```

**aura-simulator** (Simulation Testing):
```
src/
└── effects/
    ├── middleware/      # Simulation effect handlers
    │   ├── fault_injection.rs    # Byzantine faults, partitions
    │   ├── time_control.rs       # Time acceleration, pause/resume
    │   ├── state_inspection.rs   # State monitoring
    │   ├── property_checking.rs  # Safety/liveness validation
    │   └── chaos_coordination.rs # Chaos engineering
    └── system.rs        # SimulationEffectSystem
```

### Architectural Rationale

This structure reflects two distinct types of middleware:

1. **Cross-Cutting Middleware** (`src/middleware/`):
   - Wraps ANY effect system with concerns like retry, metrics, tracing
   - Uses decorator pattern to enhance existing handlers
   - Independent of specific effect types

2. **Effect-Specific Middleware** (`src/effects/middleware/`):
   - Implements specialized effect types (FaultInjection, TimeControl, etc.)
   - Part of the effect system itself, not external decoration
   - Domain-specific to the crate's purpose (simulation, agent operations, etc.)

### When to Use Each Pattern

**Use `src/middleware/` for:**
- Observability (tracing, metrics, logging)
- Resilience (retry, timeout, circuit breaker)
- Security (authorization, capability checking)
- Performance (caching, rate limiting)

**Use `src/effects/middleware/` for:**
- Crate-specific effect handlers
- Domain-specific handlers (simulation, agent, etc.)
- Effect handlers that are part of the system's core functionality

### AuraHandler Trait

The unified architecture centers around the `AuraHandler` trait provided by `AuraHandlerFactory`:

```rust
use aura_protocol::AuraEffectSystem;

// For testing
let handler = AuraEffectSystem::for_testing(device_id);

// For production
let handler = AuraEffectSystem::for_production(device_id)?;

// For simulation
let handler = AuraEffectSystem::for_simulation(device_id, seed);

// Use typed effect traits directly (zero overhead):
let bytes = handler.random_bytes(32).await;
let hash = handler.blake3_hash(&data).await;
```

### Middleware Architecture

Middleware provides **optional cross-cutting enhancements** to the core effect system. The base effect system works perfectly without any middleware - middleware adds capabilities like retry logic, metrics, tracing, and rate limiting when needed.

#### Core Pattern (No Middleware Required)

The effect system works directly without middleware:

```rust
// Direct usage - no middleware needed
let handler = AuraEffectSystem::for_production(device_id)?;
let bytes = handler.random_bytes(32).await;
let hash = handler.blake3_hash(&data).await;
```

#### Enhanced Pattern (With Middleware)

Add middleware for cross-cutting concerns:

```rust
// Base handler
let base = AuraEffectSystem::for_production(device_id)?;

// Optionally wrap with middleware
let with_retry = RetryMiddleware::new(base, 3);
let with_metrics = MetricsMiddleware::new(with_retry);
let with_tracing = TracingMiddleware::new(with_metrics, "service");

// Use the enhanced handler
let bytes = with_tracing.random_bytes(32).await;
```

#### When to Use Middleware

**Use middleware when you need:**
- Retry logic for unreliable operations
- Metrics collection and monitoring
- Distributed tracing across services
- Rate limiting and throttling
- Circuit breakers for fault tolerance
- Request logging and auditing

**Don't use middleware when:**
- Writing performance-critical hot paths
- Building simple unit tests
- Prototyping or early development
- Overhead isn't justified by benefits

#### Middleware Composition

Middleware can be stacked in any order:

```rust
AuraEffectSystem (core)
    ↓ optionally wrapped by
RetryMiddleware
    ↓ optionally wrapped by
MetricsMiddleware
    ↓ optionally wrapped by
TracingMiddleware
    ↓ executes with
Unified AuraContext
```

**Architecture Flow**:
```
AuraEffectSystem (core) → Optional Middleware Stack → Unified AuraContext
```


## Usage Patterns

### Unified Choreographic Protocols

Use the new unified integration for all choreographic protocols:

```rust
use aura_choreography::integration::{create_testing_adapter, create_choreography_endpoint};
use aura_protocol::ChoreographicRole;

// 1. Create unified choreography adapter
let mut adapter = create_testing_adapter(device_id);

// 2. Create endpoint for this device's role
let role = ChoreographicRole::new(device_id, 0);
let endpoint = create_choreography_endpoint(device_id, role, adapter.context().clone());

// 3. Execute choreography with unified system
choreography.execute(&mut adapter, &mut endpoint).await?;
```

### Session Type Protocols

Use the unified session adapter for session type execution:

```rust
use aura_choreography::integration::create_testing_session_adapter;
use aura_choreography::session_types::LocalSessionType;

// 1. Create unified session adapter
let mut adapter = create_testing_session_adapter(device_id);

// 2. Define and execute session type
let session = LocalSessionType::send(
    target_role,
    LocalSessionType::recv(sender_role, LocalSessionType::end())
);

session.execute(&mut adapter).await?;
```

### Direct Effect Usage

```rust
// Create and use effect system directly
let system = AuraEffectSystem::for_production(device_id)?;
let hash = system.blake3_hash(b"data").await;
```

## Execution Modes

All effect systems support three execution modes:

```rust
let test_system = AuraEffectSystem::for_testing(device_id);     // Mock, deterministic handlers
let prod_system = AuraEffectSystem::for_production(device_id)?; // Real effect handlers
let sim_system = AuraEffectSystem::for_simulation(device_id, 42); // Controlled, seeded handlers
```


## Context Management

Context flows through handlers as internal state rather than being passed per-call:

```rust
// Context is owned by the handler
let handler = AuraEffectSystem::for_production(device_id)?;

// Internal context is accessed via handler methods
let bytes = handler.random_bytes(32).await;  // Context used internally
let hash = handler.blake3_hash(&data).await;  // Same context flows through
```

### Why Not Per-Call Context?

Handlers store context rather than requiring it as a parameter to every effect call:

**Technical Reasons**:
- **Async trait limitations**: Passing `&mut AuraContext` creates complex lifetime issues with async traits
- **Simplified API surface**: Effect traits become simpler without context parameters
- **Consistent state**: Context naturally flows through all operations within a handler
- **Middleware compatibility**: Middleware can access and modify handler-owned context

**Practical Benefits**:
```rust
// WITHOUT handler-owned context (complex):
async fn operation(handler: &H, ctx: &mut AuraContext) -> Result<T> {
    let data = handler.fetch(param, ctx).await?;  // Context everywhere
    let hash = handler.hash(&data, ctx).await?;   // Repetitive
    handler.store(hash, ctx).await?;              // Error-prone
    Ok(result)
}

// WITH handler-owned context (clean):
async fn operation(handler: &H) -> Result<T> {
    let data = handler.fetch(param).await?;  // Context flows implicitly
    let hash = handler.hash(&data).await?;   // Cleaner
    handler.store(hash).await?;              // Simpler
    Ok(result)
}
```

### Alternative: Middleware for Explicit Context Control

If you need explicit context manipulation, use middleware:

```rust
pub struct ContextMiddleware<H> {
    inner: H,
    context: AuraContext,
}

impl<H> ContextMiddleware<H> {
    pub fn with_context(handler: H, context: AuraContext) -> Self {
        Self { inner: handler, context }
    }

    pub fn context_mut(&mut self) -> &mut AuraContext {
        &mut self.context
    }
}

// Use the middleware to control context:
let mut handler = ContextMiddleware::with_context(base_handler, custom_context);
handler.context_mut().set_device_id(new_id);
let result = handler.execute_operation().await?;
```

This pattern provides explicit context control when needed while keeping the common case simple.

## Hybrid Typed/Type-Erased Architecture

Aura uses a hybrid architecture that provides both typed effect traits and type-erased handlers:

### Two Parallel APIs

1. **Typed Effect Traits** - For performance-critical code and hot paths
2. **Type-Erased `dyn AuraHandler`** - For dynamic composition and middleware

Both APIs coexist and bridge seamlessly through blanket implementations.

### Performance Characteristics

| Pattern | API | Overhead | Use Case |
|---------|-----|----------|----------|
| **Direct typed traits** | `handler.random_bytes(32)` | **0ns** - Zero overhead | Hot loops, performance-critical |
| **Type-erased → typed** | `boxed.random_bytes(32)` | **~200ns** - Serialization | Dynamic composition |
| **Type-erased direct** | `execute_effect(...)` | **~200ns** - Serialization | Runtime effect selection |

### Usage Patterns

**Pattern 1: Hot Path (Zero Overhead)**
```rust
// Performance-critical choreography uses typed traits
async fn dkd_commitment<C: CryptoEffects, R: RandomEffects>(
    crypto: &C,
    random: &R,
) -> Commitment {
    let nonce = random.random_bytes(32).await;  // FAST: Direct call, fully inlined
    let hash = crypto.blake3_hash(&data).await;  // FAST: Zero overhead
    Commitment { hash, nonce }
}

// Call with concrete handler - zero overhead
let handler = CompositeHandler::for_testing(device_id);
let commitment = dkd_commitment(&handler, &handler).await;
```

**Pattern 2: Dynamic Composition (Flexible)**
```rust
use std::sync::Arc;
use tokio::sync::RwLock;

// Middleware stacking uses type-erased handlers wrapped in Arc<RwLock<>>
fn create_handler(config: &Config) -> Arc<RwLock<Box<dyn AuraHandler>>> {
    let base = CompositeHandler::for_testing(device_id);
    let with_retry = RetryMiddleware::new(base, 3);
    let with_tracing = TracingMiddleware::new(with_retry, "svc");
    Arc::new(RwLock::new(Box::new(with_tracing)))
}

// Can STILL use typed traits through blanket impl!
let handler = create_handler(&config);
let bytes = handler.random_bytes(32).await;  // Works! Uses blanket impl
```

**Pattern 3: Hybrid Approach**
```rust
// Function can accept either API
async fn flexible_protocol(handler: &mut dyn AuraHandler) {
    // Use typed traits (via blanket impl) - ergonomic
    let hash = handler.blake3_hash(&data).await;
    handler.broadcast(msg).await?;

    // OR use type-erased API for dynamic operations
    handler.execute_effect(
        effect_type,
        operation,
        params,
        ctx,
    ).await?;
}
```

### When to Use Which

**Use Typed Traits When:**
- Hot loops with millions of iterations
- Performance-critical choreographies (DKD, FROST)
- Known concrete types at compile time
- Want maximum compiler optimization
- Writing unit tests with typed mocks

**Use Type-Erased When:**
- Dynamic handler selection at runtime
- Building middleware stacks
- Heterogeneous collections of handlers
- Plugin systems or dynamic loading
- Simpler function signatures (avoid generic soup)

### The Bridge

Blanket implementations in `aura-protocol/src/utils/typed_bridge.rs` automatically provide typed trait implementations for `Arc<RwLock<Box<dyn AuraHandler>>>`:

```rust
#[async_trait]
impl CryptoEffects for Arc<RwLock<Box<dyn AuraHandler>>> {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut handler = self.write().await;
        HandlerUtils::execute_typed_effect(
            &mut **handler,
            EffectType::Crypto,
            "random_bytes",
            RandomBytesParams { len },
            &mut ctx,
        ).await.unwrap_or_default()
    }
}
```

**Note**: The `Arc<RwLock<>>` wrapper is required because `AuraHandler::execute_effect` needs `&mut self`. This provides interior mutability for thread-safe typed trait usage.

This means type-erased handlers automatically work as typed traits (best of both worlds).

## Creating Custom Middleware

### Using the AuraMiddleware Macro
```rust
#[derive(AuraMiddleware)]
#[middleware(effects = "[CryptoEffects, TimeEffects, RandomEffects]")]
pub struct MyMiddleware<H> {
    inner: H,
    config: MyConfig,
}

impl<H> MyMiddleware<H> {
    pub fn new(handler: H, config: MyConfig) -> Self {
        Self { inner: handler, config }
    }
}
```

The macro generates:
- All specified effect trait implementations
- Automatic delegation to `inner` handler
- Proper async/await handling
- Type-safe composition with other middleware
- Automatic `CoreEffects` implementation when core effects are present
- Automatic `ProtocolEffects` implementation when all effects are present

To include `ProtocolEffects`, specify all 8 effects: `[NetworkEffects, StorageEffects, CryptoEffects, TimeEffects, ConsoleEffects, LedgerEffects, ChoreographicEffects, RandomEffects]`.

## Acceptable Direct System Access

Production handlers legitimately need direct system access. Mark these with `#[allow(clippy::disallowed_methods)]`:

### Real Handler Implementation
```rust
#![allow(clippy::disallowed_methods)] // Real handler file

pub struct RealTimeHandler { /* ... */ }

#[async_trait]
impl TimeEffects for RealTimeHandler {
    async fn current_epoch(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}
```

The `#![allow(...)]` at file level indicates the entire handler is a legitimate system boundary.

## Testing Strategy

### Unit Tests
```rust
#[tokio::test]
async fn test_dkd() {
    let crypto = MockCryptoHandler::new();
    let random = TestRandomEffects::default();

    let result = deterministic_key_derivation(&crypto, &random).await?;
    assert_eq!(result.threshold, 2);
}
```

### Integration Tests
```rust
#[tokio::test]
async fn test_full_protocol() {
    let effects = MiddlewareStack::new(
        CompositeHandler::for_testing(device_id),
        device_id
    )
    .with_tracing("integration".to_string())
    .build();

    run_distributed_protocol(&effects).await?;
}
```

### Property Tests
Use deterministic effect handlers with property-based testing (proptest) to verify protocol invariants:

```rust
proptest! {
    #[test]
    fn prop_dkd_deterministic(seed: u64) {
        let random = TestRandomEffects::with_seed(seed);
        let result = tokio_test::block_on(async {
            run_dkd(&random).await
        })?;
        // Same seed always produces same output
    }
}
```

## Crate Boundary Rules

### Foundation Types Only (`aura-types`)
- **Allowed**: Core shared identifiers (`DeviceId`, `SessionId`, `AccountId`)
- **Allowed**: Fundamental time types (`Timestamp`, `Duration`, `LamportTimestamp`)
- **Allowed**: Basic error handling (`AuraError`, `ErrorContext`)
- **Allowed**: Serialization traits only (no implementations)
- **Forbidden**: Effect traits, domain-specific types, business logic

### Complete Effect System (`aura-protocol`)
- **Allowed**: Core effect trait definitions (`CryptoEffects`, `StorageEffects`, `NetworkEffects`, etc.)
- **Allowed**: Core effect handler implementations (delegates to system APIs)
- **Allowed**: Cross-cutting middleware implementations (retry, metrics, tracing)
- **Allowed**: Effect system infrastructure (`AuraEffectSystem`, context management)
- **Forbidden**: Domain-specific business logic, application-layer operations

### Runtime Composition (`aura-agent`)
- **Allowed**: Agent-specific effect trait definitions (`AgentEffects`, `DeviceStorageEffects`)
- **Allowed**: Agent-specific handler implementations (compose core effects into device workflows)
- **Allowed**: Agent-specific middleware (validation, agent metrics, device-specific tracing)
- **Allowed**: Runtime composition (`AuraAgent` composes handlers + middleware)
- **Forbidden**: Core effect trait definitions, system-level effect handlers

### Higher-Order Runtime (`aura-simulator`)
- **Allowed**: Simulation-specific effect trait definitions (`FaultInjectionEffects`, `TimeControlEffects`)
- **Allowed**: Simulation handler implementations (controlled, deterministic behaviors)
- **Allowed**: Higher-order runtime composition (creates networks of agent instances)
- **Allowed**: Simulated infrastructure (in-memory transports, controlled time, fault injection)
- **Purpose**: Creates agent instances with appropriate injected effects and simulated network

### Choreographic Coordination (`aura-choreography`)
- **Allowed**: Session type adapters that delegate to effect handlers
- **Allowed**: Choreographic protocol definitions
- **Forbidden**: Effect trait definitions, effect handler implementations, domain types

### Domain Business Logic (`aura-crypto`, `aura-messages`, `aura-journal`, `aura-authentication`, `aura-transport`, `aura-store`)
- **Allowed**: Domain-specific types (crypto keys, message formats, journal state, etc.)
- **Allowed**: Domain-specific business logic and algorithms
- **Allowed**: Effect consumption via dependency injection
- **Forbidden**: Effect trait definitions, effect handler implementations, middleware

### Testing & Tooling (`aura-cli`, `aura-test-utils`)
- **Allowed**: Domain-specific testing types and utilities
- **Allowed**: Effect consumption for testing scenarios
- **Forbidden**: Production effect handler implementations, middleware

## Dependency Direction Rules

### Complete Crate Hierarchy

```
aura-types (foundation identifiers, time, basic errors)
    ↑
aura-protocol (core effect system + infrastructure)
    ↑
aura-choreography (session type integration)
    ↑
┌─── aura-crypto (crypto types + algorithms)
├─── aura-messages (wire formats + protocols)  
├─── aura-journal (ledger types + CRDT operations)
├─── aura-authentication (auth types + credential management)
├─── aura-transport (network types + peer management)
├─── aura-store (storage types + content addressing)
├─── aura-agent (runtime composition + agent handlers)
├─── aura-cli (CLI types + command definitions)
└─── aura-test-utils (shared test types + utilities)
    ↑
aura-simulator (higher-order runtime + simulated networks)
```

### Architectural Layers

**Layer 0: Foundation Types** (`aura-types`)
- Core shared identifiers, time types, basic errors
- Serialization and configuration traits
- No effect-related code, no domain-specific types

**Layer 1: Core Effect System** (`aura-protocol`)
- Core effect trait definitions (`CryptoEffects`, `StorageEffects`, `NetworkEffects`)
- System-level effect handler implementations (delegates to OS/network APIs)
- Cross-cutting middleware implementations (retry, metrics, tracing)
- Effect system infrastructure (`AuraEffectSystem`, context management)

**Layer 2: Protocol Coordination** (`aura-choreography`)
- Session type adapters
- Choreographic protocol definitions  
- Consumes effects via dependency injection

**Layer 3: Domain Business Logic** (`aura-crypto`, `aura-messages`, `aura-journal`, etc.)
- Domain-specific types and algorithms
- Effect consumers via dependency injection
- No effect definitions or handlers

**Layer 4: Runtime Composition** (`aura-agent`)
- Agent-specific effect trait definitions (`DeviceStorageEffects`, `AuthenticationEffects`)
- Agent-specific handler implementations (compose core effects into device workflows)
- Runtime composition (`AuraAgent` composes handlers + middleware into executable runtime)
- Agent-specific middleware (validation, device metrics, biometric flows)

**Layer 5: Higher-Order Runtime** (`aura-simulator`)
- Simulation-specific effect traits (`FaultInjectionEffects`, `TimeControlEffects`)  
- Simulation handler implementations (controlled, deterministic behaviors)
- Creates agent instances with appropriate injected effects
- Simulated infrastructure (in-memory transports, controlled time, Byzantine faults)
- Orchestrates simulated networks of agents executing full choreographies

**Forbidden**: Circular imports, effect trait duplication, parallel effect handler systems

## Crate Roles in Effect System Architecture

### `aura-protocol`: Core Effect System Infrastructure
**Role**: Provides the foundational effect system that all other crates build upon.
- **Effect Traits**: Defines core system capabilities (`CryptoEffects`, `StorageEffects`, `NetworkEffects`)
- **Core Handlers**: Implements system-level handlers (filesystem, network sockets, crypto libraries)
- **Infrastructure**: Provides `AuraEffectSystem`, context management, middleware framework
- **Cross-Cutting**: Implements universal middleware (retry, metrics, tracing, caching)

### `aura-agent`: Runtime Composition Layer
**Role**: Composes core effects into device-specific runtimes for identity management.
- **Agent Effects**: Defines device-level capabilities (`DeviceStorageEffects`, `AuthenticationEffects`)
- **Agent Handlers**: Implements device workflows by composing core effects
- **Runtime**: `AuraAgent` composes handlers + middleware into executable device runtime
- **Device Middleware**: Agent-specific concerns (validation, biometric flows, device metrics)

**Example**: Authentication handler composes `CryptoEffects` + `StorageEffects` + biometric APIs into device unlock workflow.

### `aura-simulator`: Higher-Order Runtime Orchestration
**Role**: Creates networks of agent instances with controlled environments for testing distributed protocols.
- **Simulation Effects**: Defines simulation capabilities (`FaultInjectionEffects`, `TimeControlEffects`)
- **Simulation Handlers**: Implements controlled, deterministic behaviors for testing
- **Network Orchestration**: Creates multiple agent instances with appropriate effect injection
- **Infrastructure**: Provides in-memory transports, controlled time, Byzantine fault injection

**Example**: Creates 3 agent instances with simulated network partitions to test threshold recovery protocols.

```rust
// aura-simulator creates and orchestrates agent runtimes
let simulator = NetworkSimulator::new()
    .with_agents(3)
    .with_byzantine_faults(1)
    .with_network_partition_after(Duration::from_secs(30));

// Each agent gets injected with simulation-controlled effects
for agent in simulator.agents() {
    agent.start_threshold_ceremony().await?;
}

// Simulator controls time, network, and fault injection
simulator.advance_time(Duration::from_minutes(5)).await;
simulator.heal_network_partition().await;
```

### Domain Crates: Effect Consumers
**Role**: Implement domain-specific business logic by consuming effects via dependency injection.
- **Pure Consumers**: Use effects without implementing them
- **Domain Types**: Crypto keys, message formats, journal entries, etc.
- **Business Logic**: Algorithms and workflows using injected effects

**Example**: `aura-crypto` implements FROST threshold signatures by consuming `CryptoEffects` and `RandomEffects`.

## Anti-Patterns to Avoid

### Duplicating Effect Traits
Never redefine effect traits in other crates:
```rust
// WRONG - creates incompatible interfaces
// In aura-journal/src/effects.rs
pub trait CryptoEffects { ... }  // Conflicts with aura-types version
```

### Effect Handler Duplication
Never implement the same effect trait in multiple crates:
```rust
// WRONG - multiple CryptoEffects handler implementations
// aura-protocol/src/handlers/crypto/real.rs: impl CryptoEffects
// aura-crypto/src/handlers.rs: impl CryptoEffects  // Domain crates should not implement effect handlers
```

### Effect Handlers in Business Logic
Business logic crates should consume effects via dependency injection, never implement effect handlers:
```rust
// WRONG - aura-journal implementing effect handlers
impl CryptoEffects for JournalCryptoHandler { ... }

// RIGHT - aura-journal consuming effects via dependency injection
async fn derive_key(effects: &impl CryptoEffects) { ... }
```

### Domain Types in Foundation
Foundation crates should not contain domain-specific types:
```rust
// WRONG - crypto types in aura-types
pub struct Ed25519SigningKey { ... }  // Should be in aura-crypto
pub struct ChunkId([u8; 32]);         // Should be in aura-store

// RIGHT - only shared foundation types in aura-types
pub struct DeviceId(Uuid);   // Used across all crates
pub struct Timestamp(u64);   // Universal time representation
```

## Related Documentation

- **[Work Item 012: Restructured Effect System Architecture](../work/012.md)** - Proposed reorganization for better architectural coherence
- **[Effects Enforcement](400_effects_inforcement.md)** - Lint rules ensuring proper effect usage patterns
