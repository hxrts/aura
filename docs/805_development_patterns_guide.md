# Development Patterns and Workflows

This document covers practical patterns and workflows for building choreographic protocols and high-level applications with Aura.

## 1. Code Location

### Effects vs Coordination

A critical distinction guides where code belongs in the architecture.

**Single-party operations** in `aura-effects` are stateless and context-free. Each operation takes input and produces output without maintaining state or coordinating with other handlers.

Examples:
- `sign(key, msg) → Signature` - One device, one cryptographic operation
- `store_chunk(id, data) → Ok(())` - One device, one write
- `RealCryptoHandler` - Self-contained cryptographic operations

**Multi-party coordination** in `aura-protocol` orchestrates multiple handlers together. Operations are stateful and context-specific.

Examples:
- `execute_anti_entropy(...)` - Orchestrates sync across multiple parties
- `CrdtCoordinator` - Manages state of multiple CRDT handlers
- `GuardChain` - Coordinates authorization checks across sequential operations

The distinction determines layer placement. Single-party operations go in `aura-effects` (Layer 3). Multi-party coordination goes in `aura-protocol` (Layer 4).

**Rule of thumb**: If removing one effect handler requires changing the logic of how other handlers are called (not just removing calls), it's orchestration and belongs in Layer 4.

### Decision Matrix

| Pattern | Characteristics | Location |
|---------|-----------------|----------|
| Single effect trait method | Stateless, single operation | `aura-effects` |
| Multiple effects/handlers | Stateful, multi-handler | `aura-protocol` |
| Multi-party coordination | Distributed state, orchestration | `aura-protocol` |
| Domain types and semantics | Pure logic, no handlers | Domain crate |
| Complete reusable protocol | End-to-end, no UI | Feature crate |
| Handler/protocol assembly | Runtime composition | `aura-agent` |
| User-facing application | Has main() entry point | `aura-terminal` |

**Boundary questions for edge cases:**

- **Stateless or stateful?** Stateless goes in `aura-effects`. Stateful goes in `aura-protocol`.
- **One party or multiple?** Single-party goes in `aura-effects`. Multi-party goes in `aura-protocol`.
- **Context-free or context-specific?** Context-free goes in `aura-effects`. Context-specific goes in `aura-protocol`.

## 2. Development Workflows

### Adding a Cryptographic Primitive

1. Define the type in `aura-core` crypto module
2. Implement `aura-core` traits for the type's semantics
3. Add a single-operation handler in `aura-effects` that implements the primitive
4. Use the handler in feature crates or protocols through the effect system

### Adding a Distributed Protocol

This pipeline applies to all Layer 4/5 choreographies and all Category C ceremonies.

**Phase 1: Classification and Facts**

Classify the operation as Category A, B, or C using the decision tree in [Operation Categories](107_operation_categories.md). Define fact types with schema versioning. Implement view reducers and define the status model.

Use the `#[ceremony_facts]` macro from `aura-macros` for ceremony fact enums:

```rust
use aura_macros::ceremony_facts;

#[ceremony_facts]
pub enum InvitationFact {
    CeremonyInitiated {
        ceremony_id: CeremonyId,
        agreement_mode: Option<AgreementMode>,
        trace_id: Option<String>,
        timestamp_ms: u64,
    },
    CeremonyCommitted {
        ceremony_id: CeremonyId,
        relationship_id: String,
        agreement_mode: Option<AgreementMode>,
        trace_id: Option<String>,
        timestamp_ms: u64,
    },
    CeremonyAborted {
        ceremony_id: CeremonyId,
        reason: String,
        trace_id: Option<String>,
        timestamp_ms: u64,
    },
}
```

The macro provides canonical `ceremony_id()` and `ceremony_timestamp_ms()` accessors.

**Phase 2: Choreography Specification**

Write the choreography in a `.choreo` file and load it through `aura-macros::choreography!`. Use annotation syntax for security: `Role[guard_capability = "...", flow_cost = N] -> Target: Message`. Select the narrowest `TimeStamp` domain for each time field.

See [MPST and Choreography](108_mpst_and_choreography.md) for the DSL and projection rules.

**Phase 3: Runtime Wiring**

Create the protocol implementation in `aura-protocol` or a feature crate. Implement role runners and wire execution through `AuraProtocolAdapter` or `AuraChoreoEngine`. Register the protocol with the runtime. Integrate with the guard chain.

Category C operations must follow the ceremony contract in [Operation Categories](107_operation_categories.md).

**Phase 4: Status and Testing**

Implement `CeremonyStatus` (for Category C) or protocol-specific status views. Add shared bus integration tests, simulation tests covering partitions and delays, and choreography parity/replay checks.

**Definition of Done:**

- [ ] Operation category declared (A/B/C)
- [ ] Facts defined with reducer and schema version
- [ ] Choreography specified with roles/messages documented
- [ ] Runtime wiring added (role runners + registration)
- [ ] Category C uses ceremony runner and emits standard facts
- [ ] Status output implemented
- [ ] Shared-bus integration test added
- [ ] Simulation test added
- [ ] Choreography parity/replay tests added (Category C)

See `crates/aura-consensus/src/protocol/` for canonical examples.

### Creating a Domain Service

Domain crates define stateless handlers that take effect references per-call. The agent layer wraps these with services that manage RwLock access.

**Step 1: Create the Domain Handler**

In the domain crate (e.g., `aura-chat/src/service.rs`):

```rust
pub struct MyHandler;

impl MyHandler {
    pub fn new() -> Self { Self }

    pub async fn my_operation<E>(
        &self,
        effects: &E,
        param: SomeType,
    ) -> Result<Output>
    where
        E: StorageEffects + RandomEffects + PhysicalTimeEffects
    {
        let uuid = effects.random_uuid().await;
        // ... domain logic
    }
}
```

**Step 2: Create the Agent Service Wrapper**

In `aura-agent/src/handlers/my_service.rs`:

```rust
pub struct MyService {
    handler: MyHandler,
    effects: Arc<RwLock<AuraEffectSystem>>,
}

impl MyService {
    pub fn new(effects: Arc<RwLock<AuraEffectSystem>>) -> Self {
        Self { handler: MyHandler::new(), effects }
    }

    pub async fn my_operation(&self, param: SomeType) -> AgentResult<Output> {
        let effects = self.effects.read().await;
        self.handler.my_operation(&*effects, param).await.map_err(Into::into)
    }
}
```

**Step 3: Expose via Agent API**

In `aura-agent/src/core/api.rs`:

```rust
impl AuraAgent {
    pub fn my_service(&self) -> MyService {
        MyService::new(self.runtime.effects())
    }
}
```

Benefits: Domain crate stays pure (no tokio/RwLock), testable with mock effects, consistent pattern across crates.

### Writing Tests

1. Create test fixtures in `aura-testkit`
2. Use mock handlers from `aura-testkit` for reproducibility
3. Configure appropriate leakage budget policies for test scenarios
4. Drive the agent from the test harness
5. Compose protocols using `aura-simulator` for deterministic execution

### Implementing for New Platforms

Use the `AgentBuilder` API to assemble the runtime with appropriate effect handlers.

**Builder Strategies:**

| Strategy | Use Case | Compile-Time Safety |
|----------|----------|---------------------|
| Platform preset | Standard platforms (CLI, iOS, Android, Web) | Configuration validation |
| Custom preset | Full control over all effects | Typestate enforcement |
| Effect overrides | Preset with specific customizations | Mixed |

**Platform Presets:**

```rust
// CLI
let agent = AgentBuilder::cli()
    .data_dir("~/.aura")
    .build()
    .await?;

// iOS (requires --features ios)
let agent = AgentBuilder::ios()
    .app_group("group.com.example.aura")
    .keychain_access_group("com.example.aura")
    .build()
    .await?;

// Android (requires --features android)
let agent = AgentBuilder::android()
    .application_id("com.example.aura")
    .use_strongbox(true)
    .build()
    .await?;

// Web/WASM (requires --features web)
let agent = AgentBuilder::web()
    .storage_prefix("aura_")
    .build()
    .await?;
```

**Custom Preset with Typestate:**

```rust
let agent = AgentBuilder::custom()
    .with_crypto(Arc::new(RealCryptoHandler::new()))
    .with_storage(Arc::new(FilesystemStorageHandler::new("~/.aura".into())))
    .with_time(Arc::new(PhysicalTimeHandler::new()))
    .with_random(Arc::new(RealRandomHandler::new()))
    .with_console(Arc::new(RealConsoleHandler::new()))
    .build()
    .await?;
```

All five required effects must be provided or the code won't compile.

**Required Effects:**

| Effect | Purpose | Trait |
|--------|---------|-------|
| Crypto | Signing, verification, encryption | `CryptoEffects` |
| Storage | Persistent data storage | `StorageEffects` |
| Time | Wall-clock timestamps | `PhysicalTimeEffects` |
| Random | Cryptographically secure randomness | `RandomEffects` |
| Console | Logging and output | `ConsoleEffects` |

**Optional Effects** (have defaults):

| Effect | Default Behavior |
|--------|-----------------|
| `TransportEffects` | TCP transport |
| `LogicalClockEffects` | Derived from storage |
| `OrderClockEffects` | Derived from random |
| `ReactiveEffects` | Default reactive handler |
| `JournalEffects` | Derived from storage + crypto |
| `BiometricEffects` | Fallback no-op handler |

**Platform Implementation Checklist:**

- [ ] Identify platform-specific APIs for crypto, storage, time, random, console
- [ ] Implement the five core effect traits
- [ ] Create a preset builder (optional)
- [ ] Add feature flags for platform-specific dependencies
- [ ] Write integration tests using mock handlers
- [ ] Document platform-specific security considerations
- [ ] Consider transport requirements (WebSocket, BLE, etc.)

## 3. Implementation Patterns

### Effect Handler Pattern

Effect handlers are stateless. Each handler implements one or more effect traits from `aura-core`. It receives input, performs a single operation, and returns output. No state is maintained between calls.

Production handlers (`RealCryptoHandler`) use real libraries. Mock handlers (`MockCryptoHandler`) use deterministic implementations for testing. See [Cryptographic Architecture](100_crypto.md) for details.

### Multi-Party Coordination Pattern

Coordination logic in `aura-protocol` manages multiple handlers working together.

Coordination functions are async and stateful. For example, `execute_anti_entropy(coordinator, adapter, guards)` accepts multiple handlers, maintains state across operations, coordinates between authorization/storage/transport, and returns results depending on combined state.

### Guard Chain Pattern

The guard chain coordinates authorization, flow budgets, and journal effects in strict sequence. Guards are pure: evaluation runs synchronously over a prepared `GuardSnapshot` and yields `EffectCommand` items that an async interpreter executes.

Three-phase pattern:
1. Authorization via Biscuit + policy (async, cached)
2. Prepare snapshot (async) and evaluate guards (sync)
3. Execute commands (async) - charge, record leakage, commit journal, send transport

No transport observable occurs until the interpreter executes commands in order. See [Authorization](104_authorization.md) for examples.

### Security Patterns

**Privacy Budget Enforcement:**

The leakage tracking system implements security by default. `LeakageTracker::new()` denies undefined budgets (secure default). `LeakageTracker::legacy_permissive()` allows undefined budgets for backward compatibility. `LeakageTracker::with_undefined_policy(DefaultBudget(1000))` provides a configurable default.

**Annotation Validation:**

The choreography compiler validates annotations at compile time. Invalid or missing annotations are rejected with helpful error messages.

## 4. Type Reference

### ProtocolType

Canonical definition in `aura-core`. All crates re-export this definition.

Variants:
- `Dkd` - Deterministic Key Derivation
- `Counter` - Counter reservation protocol
- `Resharing` - Key resharing for threshold updates
- `Locking` - Resource locking protocol
- `Recovery` - Account recovery protocol
- `Compaction` - Ledger compaction protocol

### SessionStatus

Canonical definition in `aura-core`. Represents session lifecycle.

Lifecycle order:
1. `Initializing` - Session initializing
2. `Active` - Session executing
3. `Waiting` - Waiting for participant responses
4. `Completed` - Completed successfully
5. `Failed` - Failed with error
6. `Expired` - Expired due to timeout
7. `TimedOut` - Timed out during execution
8. `Cancelled` - Was cancelled

### TimeStamp Domains

`TimeStamp` in `aura-core` is the only time type for new facts and public APIs.

| Domain | Effect Trait | Primary Use |
|--------|--------------|-------------|
| `PhysicalClock` | `PhysicalTimeEffects` | Wall time: cooldowns, receipt timestamps, liveness |
| `LogicalClock` | `LogicalClockEffects` | Causal ordering: CRDT merge, happens-before |
| `OrderClock` | `OrderClockEffects` | Deterministic ordering without timing leakage |
| `Range` | `PhysicalTimeEffects` + policy | Validity windows with bounded skew |
| `ProvenancedTime` | `TimeAttestationEffects` | Attested timestamps for consensus |

Use effect traits for all time reads. Do not call `SystemTime::now()` or chrono APIs.

Use the narrowest domain that satisfies the requirement. Compare mixed domains with `TimeStamp::compare(policy)`. Persist `TimeStamp` values directly in facts.

**Anti-patterns:**
- Mixing clock domains in one sort path without explicit policy
- Using `PhysicalClock` for privacy-sensitive ordering
- Using UUID or insertion order as time proxy
- Exposing `SystemTime` or chrono types in interfaces

### Capability System Layering

The capability system uses multiple layers intentionally:

- **Canonical types** in `aura-core` provide lightweight references
- **Authorization layer** (`aura-authorization`) adds policy enforcement
- **Storage layer** (`aura-store`) implements capability-based access control

Clear conversion paths enable inter-layer communication.

## 5. Policy Compliance

Application code must follow policies defined in [Project Structure](999_project_structure.md).

### Impure Function Usage

All time, randomness, filesystem, and network operations must flow through effect traits. Direct calls to `SystemTime::now()`, `thread_rng()`, or `std::fs` break simulation determinism and WASM compatibility.

### Serialization

Wire protocols and facts use DAG-CBOR encoding via `aura_core::util::serialization`. JSON is allowed for user-facing config files and debug output.

### Architectural Validation

Run `just check-arch` before submitting changes. The checker validates layer boundaries, effect trait placement, impure function routing, and guard chain integrity.
