# Aura System Architecture

This document describes how to implement systems using Aura's theoretical foundations. It covers the effect system architecture, CRDT implementation patterns, choreographic protocol design, and crate organization principles.

## Overview

Aura's system architecture translates mathematical foundations into practical implementation patterns through (formal definitions live in `docs/001_theoretical_foundations.md`):

1. **Unified Effect System** - Composable effect handlers with middleware support
2. **CRDT Implementation Architecture** - 4-layer system for conflict-free replication
3. **Choreographic Protocol Design** - Session-typed distributed coordination
4. **Crate Organization** - Clean dependency structure and separation of concerns

---

## Terminology & Layering

**Canonical Reference**: For all architectural terms and concepts, see [`docs/099_glossary.md`](099_glossary.md).

### Effect System Runtime Objects

The effect system uses these canonical names throughout the codebase:

- **AuraEffectSystem** - Main runtime façade for all effect operations
  - Implementation: [`crates/aura-protocol/src/effects/system.rs`](../crates/aura-protocol/src/effects/system.rs)
  - Usage: Primary entry point for applications
  
- **CompositeHandler** - Internal composition component within `AuraEffectSystem`
  - Implementation: [`crates/aura-protocol/src/handlers/composite.rs`](../crates/aura-protocol/src/handlers/composite.rs)  
  - Usage: Internal delegation pattern, not used directly

- **AuraHandler** - Unified trait interface for type-erased handlers
  - Implementation: [`crates/aura-protocol/src/handlers/erased.rs`](../crates/aura-protocol/src/handlers/erased.rs)
  - Usage: Base trait for dynamic dispatch

### Effect Trait Categories

**Core Effect Traits** - Foundational interfaces in [`crates/aura-core/src/effects/`](../crates/aura-core/src/effects/):
- `TimeEffects`, `CryptoEffects`, `StorageEffects`, `NetworkEffects`, `JournalEffects`, `ConsoleEffects`, `RandomEffects`

**Extended Effect Traits** - Higher-level interfaces in [`crates/aura-protocol/src/effects/`](../crates/aura-protocol/src/effects/):
- `SystemEffects`, `LedgerEffects`, `ChoreographicEffects`, `TreeEffects`, `AgentEffects`

### Data Layer Separation

- **Journal** ([`crates/aura-journal/`](../crates/aura-journal/)) - High-level CRDT state management
- **Ledger** ([`crates/aura-protocol/src/effects/ledger.rs`](../crates/aura-protocol/src/effects/ledger.rs)) - Low-level effect interface

See [`docs/105_journal.md`](105_journal.md) for canonical explanation of Journal vs Ledger architecture.

### Protocol Stack Layers

- **Choreographies** - Global protocol specifications (currently documentation, not executable)
- **Session Types** - Local projections of choreographies (infrastructure exists, projection pending)  
- **Protocols** - Current manual async implementations

See Protocol Stack section below for detailed explanation.

### Auth/Authz Flow

- **Authentication** ([`crates/aura-verify/`](../crates/aura-verify/) + [`crates/aura-authenticate/`](../crates/aura-authenticate/)) - Identity verification (WHO)
- **Authorization** ([`crates/aura-wot/`](../crates/aura-wot/)) - Capability evaluation (WHAT)
- **Integration** ([`crates/aura-protocol/src/authorization_bridge.rs`](../crates/aura-protocol/src/authorization_bridge.rs)) - Clean composition

See [`docs/101_auth_authz.md`](101_auth_authz.md) for complete authentication vs authorization architecture.

### Projection Roadmap

Aura treats choreographies as the source of truth, but only some protocols are hand-written today. The projection plan for 1.0 is:

1. **Targeted Projection**: Start with the `AddDevice` choreography. Implement the full path `choreography!` → rumpsteak projection → generated session code → effect execution. This will validate the compiler and runtime bridge.
2. **Runtime Bridge**: Use `aura-mpst` as the projection engine and `aura-protocol/src/choreography/runtime` as the interpreter. The bridge will emit FlowBudget charges and leakage guards automatically for each transition.
3. **Graduation Criteria**: A protocol is considered “projected” when the generated code replaces the manual async implementation and passes the existing `just smoke-test` suite. Until then, the manual protocol remains the reference implementation.
4. **Staged Rollout**: After `AddDevice`, project `GuardianRecovery` and `RendezvousOffer`. Each rollout must include a migration plan and verification artifacts captured in `docs/003_distributed_applications.md`.

This roadmap keeps the specification executable without blocking 1.0 functionality.

---

## 1. Unified Effect System Architecture

### 1.1 Core Principles

Aura uses a **unified effect system architecture** centered around the `AuraEffectSystem`. This system provides:

**Architecture Principles:**
- **Unified**: One effect system for all operations (choreography, agent, simulation)
- **Middleware-Optional**: Base system works directly; middleware adds enhancements when needed
- **Context-Driven**: Unified `AuraContext` flows through all operations
- **Mode-Aware**: Execution mode (Testing, Production, Simulation) drives behavior

### 1.2 Algebraic Effect Theory & Terminology

Aura uses algebraic effect terminology with strict separation between abstract interfaces and concrete implementations.

**Effects** define abstract capabilities as trait interfaces:
```rust
#[async_trait]
pub trait CryptoEffects {
    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32];
}
```
Effect traits specify operations without implementation details. Multiple handlers can implement the same trait with different behaviors.

**Effect Handlers** provide concrete implementations:
```rust
pub struct RealCryptoHandler;
impl CryptoEffects for RealCryptoHandler {
    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        blake3::hash(data).into()
    }
}
```
Handlers contain the actual business logic. Different handlers enable testing, production, and simulation modes.

**Middleware** wraps handlers with cross-cutting concerns:
```rust
pub struct RetryMiddleware<H> { inner: H, max_attempts: u32 }
```
Middleware implements effect traits by delegating to inner handlers with additional behavior like retry logic or metrics.

### 1.3 Core Effect Types

The effect system organizes capabilities into categories:

**Core Effects** provide fundamental operations including network communication, cryptographic primitives, persistent storage, time management, and console output.

**Agent Effects** handle device-specific concerns like secure storage, biometric authentication, and session management.

**Simulation Effects** enable controlled testing with fault injection, time manipulation, and property verification.

**Privacy Effects** enforce leakage budgets and flow control to maintain confidentiality guarantees across protocol executions.

### 1.4 Session Type Algebra Integration

The unified effect system integrates with Aura's session type algebra for choreographic programming:

```
Session Type Algebra (Global Protocol)
    ↓ projection
Local Session Types (Per-Role Protocols)
    ↓ execution via Effect Interpreter Interface
Effect Algebra (CryptoEffects, NetworkEffects, etc.)
    ↓ interpretation by
Handler Implementations
```

**Static Path** generates direct effect calls from choreographies:
```rust
choreography! {
    protocol P2PDkd {
        roles: Alice, Bob;
        Alice -> Bob: Hello;
    }
}
```
The macro generates compile-time session types that map directly to effect system operations.

**Dynamic Path** interprets session types at runtime:
```rust
let Roles(mut alice, mut bob) = setup();
rumpsteak_aura::try_session(&mut alice, |session| async move {
    execute_alice_role(session, &effect_system).await
}).await?
```
Runtime interpretation provides flexibility for complex protocols that cannot be statically compiled.

### 1.5 SecureChannel Abstraction

Protocols that complete a rendezvous or recovery handshake return a `SecureChannel`. This abstraction:

- Wraps a QUIC connection plus metadata `(context, peer_device, epoch, channel_id)`
- Is managed by the transport layer so higher-level protocols obtain channels via `TransportEffects`
- Enforces a single active channel per `(context, peer_device)` and tears it down when FlowBudget reservations or epochs change

Lifecycle and invariants:
- Single active channel per `(ContextId, peer_device)`.
- Channel teardown on: `epoch(ctx)` rotation, capability shrink that invalidates `need(message) ≤ Caps(ctx)`, or context invalidation.
- Reconnect behavior: re-run rendezvous; budget reservations and receipts do not carry across epochs.
- Receipt scope: per-hop receipts are bound to `(ctx, src, dst, epoch)` and are never reused across channels or epochs.

See `docs/104_rendezvous.md` for full lifecycle details.

### 1.6 Guard Chain and Predicate

All transport side effects must pass the following guard chain, in order:

1. CapGuard — authorization: `need(message) ≤ Caps(ctx)`
2. FlowGuard — budgeting: `headroom(ctx, cost)` (charge-before-send)
3. JournalCoupler — atomic commit of attested facts on success

Observable behavior:
- If CapGuard fails: deny locally, no packet emitted.
- If FlowGuard fails: block locally, no packet emitted (no observable without charge).
- If JournalCoupler fails: do not emit; the commit and send are coupled.

Definitions:
- `headroom(ctx, cost)` succeeds iff charging `(ctx, peer)` by `cost` in the current `epoch(ctx)` keeps `spent ≤ limit` and yields a signed receipt bound to the epoch.

See `docs/001_theoretical_foundations.md` §5.3 for the formal contract and `docs/004_info_flow_model.md` for receipt/epoch details.

### 1.6 Hybrid Typed/Type-Erased Architecture

Aura uses a hybrid architecture that provides both typed effect traits and type-erased handlers:

**Two Parallel APIs:**
1. **Typed Effect Traits** - For performance-critical code and hot paths
2. **Type-Erased `dyn AuraHandler`** - For dynamic composition and middleware

| Pattern | API | Overhead | Use Case |
|---------|-----|----------|----------|
| **Direct typed traits** | `handler.random_bytes(32)` | **0ns** - Zero overhead | Hot loops, performance-critical |
| **Type-erased → typed** | `boxed.random_bytes(32)` | **~200ns** - Serialization | Dynamic composition |
| **Type-erased direct** | `execute_effect(...)` | **~200ns** - Serialization | Runtime effect selection |

#### AuraHandler Trait & Typed Bridge

The type-erased side is formalized in `crates/aura-protocol/src/handlers/erased.rs` as the `AuraHandler` trait. Concrete handlers (for testing, production, simulation) implement this trait, and `crates/aura-protocol/src/handlers/typed_bridge.rs` provides blanket implementations of every effect trait for `Arc<RwLock<Box<dyn AuraHandler>>>`. That bridge is what lets you call `CryptoEffects`/`NetworkEffects` on a type-erased handler without rewriting effect-specific glue. If you need to inspect or extend the dispatch surface, start with those two files.

Decision rule:
- Use typed traits directly on hot paths (zero overhead).
- Use `dyn AuraHandler` when you need middleware stacking, dynamic composition, or late binding.

Umbrella surface:
- Prefer an `AuraEffects` umbrella trait (re-exporting the core effect traits) in new code to keep call sites uniform. The typed bridge provides a blanket impl for `Arc<RwLock<Box<dyn AuraHandler>>>`.

### 1.7 Middleware Architecture

Middleware provides optional cross-cutting enhancements without affecting core protocols:
```rust
let with_retry = RetryMiddleware::new(base_handler, 3);
```
This wrapper provides retry functionality for transient failures. Common middleware includes retry logic, metrics collection, distributed tracing, and circuit breakers.

### 1.8 Context Management

Context flows through handlers as internal state:
```rust
let handler = AuraEffectSystem::for_production(device_id)?;
let bytes = handler.random_bytes(32).await;
```

`AuraContext` enforces privacy isolation by preventing cross-context communication. Each handler instance owns context state including relationship IDs, DKD namespaces, and leakage counters. Messages are blocked unless sender and receiver contexts match.

### 1.9 Execution Modes

Effect systems support three execution modes:
```rust
let test_system = AuraEffectSystem::for_testing(device_id);
let prod_system = AuraEffectSystem::for_production(device_id)?;
let sim_system = AuraEffectSystem::for_simulation(device_id, 42);
```
Testing mode provides deterministic behavior. Production mode uses real implementations. Simulation mode enables controlled fault injection with seeded randomness.

### 1.10 Flow Budget Enforcement

Flow budgets prevent spam while maintaining privacy. Each context-peer pair has a `FlowBudget` with spent and limit counters stored in the journal. Transport effects check budgets before sending messages. Choreographies annotate operations with flow costs that are charged against available budgets.

Canonical type:
```
FlowBudget { limit: u64, spent: u64, epoch: Epoch }
```
Invariants: charge-before-send; no observable without charge; deterministic replenishment per epoch (see `docs/004_info_flow_model.md`). Cover traffic is explicitly deferred in 1.0; see `docs/004_info_flow_model.md` §Cover Traffic Strategy.

---

## 2. CRDT Implementation Architecture

### 2.1 4-Layer Architecture

Aura's CRDT system implements a **4-layer architecture** that separates:

1. **Semantic Foundation** - Core CRDT traits and message type definitions
2. **Effect Interpretation** - Composable handlers that enforce CRDT laws
3. **Choreographic Protocols** - Session-type communication patterns
4. **Application CRDTs** - Domain-specific implementations

### 2.2 File Organization

```
aura-core/src/semilattice/          # Foundation layer (workspace-wide)
├── semantic_traits.rs               # JoinSemilattice, MeetSemiLattice, CvState, MvState, etc.
├── message_types.rs                 # StateMsg<S>, MeetStateMsg<S>, OpWithCtx<Op,Ctx>, etc.
├── tests/                          # Property-based tests for algebraic laws
│   └── meet_properties.rs          # Meet semi-lattice law validation
└── mod.rs                          # Re-exports and trait implementations

aura-protocol/src/effects/semilattice/  # Effect interpreter layer
├── cv_handler.rs                   # CvHandler<S: CvState> - join-based state CRDTs
├── mv_handler.rs                   # MvHandler<S: MvState> - meet-based constraint CRDTs
├── delta_handler.rs                # DeltaHandler<S,D> - delta-based
├── cm_handler.rs                   # CmHandler<S,Op> - operation-based
├── delivery.rs                     # CausalBroadcast, AtLeastOnce effects
└── mod.rs                          # Handler composition + execute_* helpers

aura-protocol/src/choreography/      # Choreographic protocol layer
├── protocols/                      # Anti-entropy, snapshot, threshold, tree coordination, etc.
└── runtime/
    └── aura_handler_adapter.rs     # AuraHandlerAdapter + factory (testing/prod/sim scaffolding)

aura-journal/src/semilattice/       # Application semilattice layer
├── journal_map.rs                  # JournalMap as CvState implementation
├── account_state.rs                # Modern AccountState using semilattice composition
├── concrete_types.rs               # Domain-specific CRDT types (DeviceRegistry, etc.)
├── meet_types.rs                   # Domain-specific meet CRDTs (CapabilitySet, etc.)
├── op_log.rs                       # Operation logs + replay helpers
└── mod.rs                          # Journal-specific re-exports
```

Projection/runtime glue that connects the algebra to these modules lives in `crates/aura-mpst/src/runtime.rs` and `crates/aura-protocol/src/handlers/rumpsteak_handler.rs`.

### 2.3 Generic Handlers

Generic handlers enforce CRDT laws through typed interfaces:

**CvRDT Handler** manages state-based CRDTs:
```rust
pub struct CvHandler<S: CvState> { pub state: S }
impl<S: CvState> CvHandler<S> {
    pub fn on_recv(&mut self, msg: StateMsg<S>) { 
        self.state = self.state.join(&msg.0); 
    }
}
```
State-based handlers merge incoming states using join operations that preserve CRDT convergence properties.

**Delta Handler** processes incremental updates:
```rust
pub struct DeltaHandler<S: CvState, D: Delta> { 
    pub state: S, 
    pub inbox: Vec<D> 
}
```
Delta handlers batch updates for efficiency while maintaining causal consistency through ordering constraints.

**Operation Handler** applies operations with causal ordering:
```rust
pub struct CmHandler<S, Op, Id, Ctx> { pub state: S }
```
Operation-based handlers apply operations in causal order while preventing duplicate application through deduplication tracking.

### 2.4 Delivery Effects

Delivery effects ensure CRDT messages reach all participants with proper ordering:

```rust
// Delivery/order effects used alongside SessionSend/Recv
pub enum DeliveryEffect {
    CausalBroadcast { topic: TopicId },  // ensures happens-before delivery
    AtLeastOnce    { topic: TopicId },   // retries; dedup in handler
    GossipTick     { topic: TopicId },   // drive periodic exchange
    ExchangeDigest,                      // trigger repair subprotocol
}
```

Programs combine delivery effects with session operations for complete protocol execution:
```rust
let prog = Program::new()
    .choose("issue")
    .send(peer_id, op_message)
    .parallel(concurrent_sends)
    .end();
```
This creates atomic protocol sequences that maintain causal consistency across all participants.

---

## 3. Rumpsteak-Aura Choreographic System

### 3.1 System Overview

Rumpsteak-Aura is Aura's choreographic programming system. It enables writing distributed protocols as global specifications that automatically compile to local implementations for each participant.

The system translates global protocol descriptions into session-typed Rust code. This prevents communication errors like deadlocks while enabling optimization through asynchronous subtyping.

### 3.2 Architecture Components

**DSL Parser** converts choreographic syntax into Abstract Syntax Trees:
```rust
choreography! {
    PingPong {
        roles: Alice, Bob
        Alice -> Bob: Ping
        Bob -> Alice: Pong
    }
}
```
The parser validates role declarations and builds protocol trees from the textual specification.

**Projection Engine** transforms global protocols into local session types:
```rust
// Alice's projected view
LocalType::Send { to: Bob, message: Ping,
    continuation: LocalType::Receive { from: Bob, message: Pong, ... }
}
```
Each participant receives their specific protocol view without global coordination requirements.

**Code Generation** produces type-safe Rust implementations:
```rust
type Alice_Protocol = Send<Bob, Ping, Receive<Bob, Pong, End>>;
```
Generated session types enforce protocol compliance at compile time through the Rust type system.

**Effect Handler Bridge** connects session types to Aura's effect system:
```rust
pub trait ChoreoHandler {
    type Role;
    type Endpoint;
    async fn send<M>(&mut self, ep: &mut Self::Endpoint, to: Self::Role, msg: &M) -> Result<()>;
    async fn recv<M>(&mut self, ep: &mut Self::Endpoint, from: Self::Role) -> Result<M>;
}
```
Handlers implement protocol execution using different transport mechanisms while maintaining the same choreographic interface.

### 3.3 Integration with Aura Effects

Rumpsteak-Aura integrates with Aura's unified effect system through handler adapters. Choreographic operations map to effect system calls:

```rust
// Choreographic send operation
handler.send(&mut endpoint, role, &message).await?

// Maps to effect system
effects.network().send_message(peer_id, serialized_message).await?
```
The adapter layer handles serialization, context management, and capability checking.

Capability Guards ensure messages can only be sent when proper authorization exists. The guard condition `need(message) ≤ caps(context)` is verified before each send operation.

Journal Coupling automatically updates replicated state during protocol execution. State changes are atomic with message emission.

Leakage Budgets track privacy costs with annotations specifying external, neighbor, and group leakage limits.

### 3.4 Runtime Execution Modes

**In-Memory Handler** provides fast testing with deterministic message delivery through in-process channels.

**Production Handler** implements network communication using QUIC connections and WebSocket fallbacks for real deployment scenarios.

**Simulation Handler** enables controlled testing with configurable fault injection including delays, message drops, and Byzantine failures.

---

## 4. Protocol Stack Architecture

### 4.1 Three-Layer Protocol Stack

Aura's protocol layer implements a three-tier architecture that separates global specifications from local implementations:

```
Global View        ┌─────────────────────────────────────┐
Choreographies  ───┤ choreography! { protocol P2P_DKD    │  Specification
                   │   roles: Alice, Bob; ... }          │  (Documentation)
                   └─────────────────┬───────────────────┘
                                     │ projection (planned)
                                     ▼
Local View         ┌─────────────────────────────────────┐
Session Types   ───┤ LocalSessionType::Alice(...)        │  Infrastructure
                   │ Generated per-role protocols        │  (Exists)
                   └─────────────────┬───────────────────┘
                                     │ implementation
                                     ▼
Implementation     ┌─────────────────────────────────────┐
Protocols       ───┤ async fn execute_dkd_alice(...)     │  Working Code
                   │ Manual async protocol impls         │  (Current)
                   └─────────────────────────────────────┘
```

### 4.2 Current Implementation Status

**Choreographies** - Global protocol specifications using `choreography!` macro:
- **Location**: [`crates/aura-protocol/src/choreography/protocols/`](../crates/aura-protocol/src/choreography/protocols/)
- **Status**: Used as documentation and specification, not executable
- **Example**: [`crates/aura-protocol/src/choreography/protocols/frost.rs`](../crates/aura-protocol/src/choreography/protocols/frost.rs)
- **Current Role**: Protocol documentation with manual implementation fallback

**Session Types** - Local projections of choreographies:
- **Infrastructure**: [`crates/aura-mpst/`](../crates/aura-mpst/) provides MPST extensions
- **Runtime**: [`crates/aura-protocol/src/choreography/runtime/`](../crates/aura-protocol/src/choreography/runtime/)
- **Status**: Infrastructure exists, choreography projection not yet implemented
- **Extensions**: Capability guards, journal coupling, leakage budgets working

**Protocols** - Current manual async implementations:
- **Status**: Working implementations used until choreographic projection is complete
- **Pattern**: Manual async functions that execute the choreographic intent
- **Integration**: Protocols use effect system for all operations

### 4.3 Architecture Evolution Path

**Current (Phase 1)**: Manual protocols with choreographies as documentation
```rust
// Choreography documents the protocol
choreography! {
    protocol FrostThreshold {
        roles: Coordinator, Signer1, Signer2;
        Coordinator -> Signer1: FrostInit(message);
        // ...
    }
}

// But execution uses manual implementation
async fn execute_frost_signing(effects: &AuraEffectSystem) -> Result<FrostResult> {
    // Manual async protocol implementation
}
```

**Planned (Phase 2)**: Generated session types with choreographic compilation
```rust
// Choreography compiles to executable session types
let session_type = FrostThreshold::project_to_coordinator();
let result = execute_session(session_type, &effects).await?;
```

### 4.4 MPST Extensions Integration

The session type infrastructure includes Aura-specific extensions:

**Capability Guards** verify authorization before message transmission. The condition `need(message) ≤ caps(context)` ensures proper permissions exist.

**Journal Coupling** atomically updates CRDT state with message emission. This prevents state divergence during protocol execution.

**Leakage Budgets** track privacy costs per operation with fine-grained external, neighbor, and group leakage accounting.

### 4.5 Effect System Integration

Choreographic protocols execute through the unified effect system for operations like message transmission, journal updates, and signature verification. This provides testability through mock effects, deterministic simulation, and clean protocol composition.

---

## 5. Authentication vs Authorization Flow

### 5.1 Architecture Overview

Aura maintains strict separation between authentication (WHO) and authorization (WHAT) while providing clean integration patterns. This separation enables independent testing, flexible policy evolution, and clear security boundaries.

**Authentication Layer**:
- **aura-verify** ([`crates/aura-verify/`](../crates/aura-verify/)) - Pure cryptographic identity verification
- **aura-authenticate** ([`crates/aura-authenticate/`](../crates/aura-authenticate/)) - Choreographic authentication protocols

**Authorization Layer**:
- **aura-wot** ([`crates/aura-wot/`](../crates/aura-wot/)) - Capability-based access control using meet-semilattice operations

**Integration Layer**:
- **authorization_bridge** ([`crates/aura-protocol/src/authorization_bridge.rs`](../crates/aura-protocol/src/authorization_bridge.rs)) - Clean composition without coupling

### 5.2 Data Flow Architecture

```
Identity Proof → Authentication → Authorization → Permission Grant
     ↓               ↓               ↓               ↓
Device/Guardian → Verified Identity → Capability → Allow/Deny
Signature          (WHO verified)     Evaluation    Operation
```

**Linear Data Flow**:
1. **Input**: `IdentityProof` (device signature, guardian signature, or threshold signature)
2. **AuthorizationContext**: evaluated capabilities for the active `ContextId`
3. **Predicate at send sites**: `need(m) ≤ Caps(ctx) ∧ headroom(ctx, cost)`

`AuthorizationContext` flows through sessions/effects so that each send site can evaluate the same predicate uniformly. Cap failures or headroom failures are handled locally with no network observable.
2. **Authentication**: `aura-verify::verify_identity_proof()` → `VerifiedIdentity`
3. **Authorization**: `aura-wot::evaluate_authorization()` → `PermissionGrant`
4. **Integration**: `authorization_bridge::authenticate_and_authorize()` orchestrates both layers

### 4.3 Effect System Integration

The auth/authz layers integrate seamlessly with Aura's unified effect system:

**Agent-Level Effects** ([`crates/aura-agent/src/handlers/auth.rs`](../crates/aura-agent/src/handlers/auth.rs)):
```rust
#[async_trait]
pub trait AuthenticationEffects: Send + Sync {
    async fn authenticate_device(&self) -> Result<AuthenticationResult>;
    async fn is_authenticated(&self) -> Result<bool>;
    async fn get_session_ticket(&self) -> Result<Option<SessionTicket>>;
}
```

**Protocol-Level Effects** ([`crates/aura-protocol/src/effects/agent.rs`](../crates/aura-protocol/src/effects/agent.rs)):
```rust
#[async_trait]
pub trait AgentEffects: Send + Sync {
    async fn verify_capability(&self, capability: &[u8]) -> Result<bool>;
    async fn evaluate_tree_operation(&self, op: &TreeOp) -> Result<PermissionGrant>;
    async fn authorize_operation(&self, request: AuthorizedOperationRequest) -> Result<PermissionGrant>;
}
```

### 4.4 Formal Properties

**Meet-Semilattice Capability Operations**:
- **Associativity**: `a.meet(b.meet(c)) == a.meet(b).meet(c)`
- **Commutativity**: `a.meet(b) == b.meet(a)`
- **Idempotence**: `a.meet(a) == a`
- **Monotonicity**: Capabilities can only be refined (reduced), never expanded

**Zero Coupling Guarantee**:
- Authentication layers never import authorization code
- Authorization layers never import authentication code
- Bridge orchestrates both through well-defined interfaces
- Each layer is independently testable with mocks

### 4.5 Implementation Status

**✅ Working Components**:
- Pure cryptographic verification with all proof types
- Capability evaluation with verified semilattice properties
- Authorization bridge with zero coupling
- Effect system integration with unified traits

**⚠️ In Progress**:
- Choreographic authentication ceremonies (infrastructure exists)
- Advanced policy evaluation and delegation chains
- Session management and token lifecycle

**See**: [`docs/101_auth_authz.md`](101_auth_authz.md) for complete implementation details and usage patterns.

---

## 5. Choreographic Protocol Design

### 5.1 Free Algebra Property

Choreographies expand into the `Program<R, M>` free algebra with effects for message passing, choice, parallelism, and control flow:

```rust
pub enum Effect<R, M> {
    Send { to: R, msg: M },
    Recv { from: R, msg_type: &'static str },
    Choose { at: R, label: Label },
    Parallel { programs: Vec<Program<R, M>> },
    End,
}
```

The polymorphic interpreter walks the AST and dispatches operations to AuraEffectSystem via handlers.

### 5.2 Algebraic Operators

**Sequential Composition** chains operations through continuation fields in protocol definitions or explicit sequencing in program algebra.

**Parallel Composition** executes multiple protocols concurrently using the `Parallel` effect with independent program branches.

**Choice Operations** enable branching protocols where one role selects from multiple options and other roles adapt accordingly.

### 5.3 Usage Patterns

**Choreographic Protocols** execute through the unified effect system for operations like threshold ceremonies and session management.

**Session Type Protocols** provide runtime validation through the MPST system with automatic role projection and type checking.

### 5.4 Capability-Guarded Transitions

MPST extensions include capability guards that verify `need(message) ≤ caps(context)` before allowing message transmission. The runtime enforces authorization through meet-semilattice capability checks.

### 5.5 Journal-Coupled Transitions

Protocol messages can trigger replicated state changes through journal coupling. The handler computes state deltas, applies them locally, then transmits the message atomically.

### 5.6 Leakage Budgets

Privacy budgets track external, neighbor, and group leakage costs per protocol transition. Handler policies enforce aggregate leakage thresholds through traffic shaping and padding decisions.

---

## 6. Crate Organization and Dependencies

### 6.1 Crate Hierarchy

The workspace uses layered dependencies from foundation types through domain logic to runtime composition. Core types flow upward through the effect system to choreographic coordination and finally to business logic crates and runtime composition.

### 6.2 Architectural Layers

**Foundation Types** provide core identifiers, time types, and error handling without domain-specific logic.

**Core Effect System** defines effect traits, system-level handlers, middleware, and infrastructure for the unified effect system.

**Protocol Coordination** implements session type adapters and choreographic protocol definitions consuming effects via dependency injection.

**Domain Business Logic** contains domain-specific types and algorithms while consuming effects through dependency injection.

**Runtime Composition** provides agent-specific effect traits, handlers, and runtime composition combining core effects into executable workflows.

**Higher-Order Runtime** implements simulation-specific effects and controlled behaviors for testing with deterministic infrastructure.

### 6.3 Crate Boundary Rules

**Foundation Types** contain core identifiers, time types, and error handling while excluding effect traits and domain logic.

**Effect System** provides core effect traits, handlers, middleware, and infrastructure while excluding domain-specific operations.

**Runtime Composition** implements agent-specific effects and runtime composition while avoiding core effect definitions.

**Domain Logic** contains domain-specific types and algorithms consuming effects through dependency injection while avoiding effect definitions.

### 6.4 Anti-Patterns to Avoid

**Duplicating Effect Traits** creates incompatible interfaces across crates and breaks composition.

**Effect Handler Duplication** violates separation of concerns when domain crates implement system-level handlers.

**Effect Handlers in Business Logic** breaks architectural boundaries. Domain crates should consume effects through dependency injection.

### 6.5 Crate Roles in Effect System Architecture

**Core Infrastructure** defines system capabilities and implements universal middleware for foundational operations.

**Runtime Composition** provides device-level capabilities and implements workflows by composing core effects into executable runtimes.

**Simulation Orchestration** defines simulation capabilities and implements controlled behaviors for testing with deterministic infrastructure.

---

## 7. Implementation Guidelines

### 7.1 Creating Custom Middleware

Custom middleware wraps handlers to add cross-cutting functionality through macro-generated trait implementations that delegate to inner handlers.

### 7.2 Direct System Access

Production handlers implement real system operations and may bypass linting restrictions for legitimate system calls in controlled contexts.

### 7.3 Usage Patterns

CRDT integration combines foundation types with effect handlers to provide composable semilattices with type safety and automatic conflict resolution.

## See Also

- `001_theoretical_foundations.md` - Mathematical foundations and formal model
- `003_distributed_applications.md` - Concrete applications and examples
- `000_overview.md` - Overall project architecture and goals
