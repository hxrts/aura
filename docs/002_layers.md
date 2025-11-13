# Aura's 8-Layer Clean Architecture

Aura's codebase is organized into 8 clean architectural layers, progressing from abstract interfaces to concrete applications. Each layer has clear responsibilities and builds on the layers below without reaching back down (no circular dependencies).

## Architecture Overview

```
┌─────────────────────────────────────────────┐
│ Layer 7: User Interface                     │ ← What users run (binaries with main)
│         (aura-cli, etc.)                    │
├─────────────────────────────────────────────┤
│ Layer 6: Runtime Composition                │ ← Assemble handlers + protocols into systems
│         (aura-agent, aura-simulator)        │ ← Libraries, not binaries
├─────────────────────────────────────────────┤
│ Layer 5: Feature/Protocol Implementation    │ ← Complete end-to-end protocols
│         (aura-frost, aura-invitation, etc.) │ ← No UI, reusable building blocks
├─────────────────────────────────────────────┤
│ Layer 4: Orchestration                      │ ← Multi-party coordination
│         (aura-protocol)                     │ ← Coordination primitives + patterns
├─────────────────────────────────────────────┤
│ Layer 3: Implementation                     │ ← Context-free effect handlers
│         (aura-effects)                      │ ← Stateless, single operations
├─────────────────────────────────────────────┤
│ Layer 2: Specification                      │ ← Type definitions + domain logic
│         (Domain crates + aura-mpst)         │ ← Semantics without implementation
├─────────────────────────────────────────────┤
│ Layer 1: Foundation/Interface               │ ← Trait declarations
│         (aura-core)                         │ ← Single source of truth
└─────────────────────────────────────────────┘
```

## Layer 1: Foundation — `aura-core`

**Purpose**: Single source of truth for all domain concepts and interfaces

**Contains**:
- Effect traits: `CryptoEffects`, `NetworkEffects`, `StorageEffects`, `TimeEffects`, `JournalEffects`, `ConsoleEffects`, `RandomEffects`
- Domain types: `DeviceId`, `AccountId`, `SessionId`, `Capability`, `FlowBudget`
- Semantic traits: `JoinSemilattice`, `MeetSemilattice`, `CvState`, `MvState`
- Error types and core protocols

**Key principle**: Interface only - no implementations, no business logic

**Dependencies**: Only `serde`, `uuid`, `thiserror`, `chrono` - no other Aura crates

---

## Layer 2: Specification — Domain Crates + `aura-mpst`

**Purpose**: Define "what things mean" in specific problem domains and how parties communicate

### Domain Crates

Define domain-specific types, semantics, and pure logic without effect handlers:

| Crate | Domain | Responsibility |
|-------|--------|-----------------|
| `aura-crypto` | Cryptography | FROST protocol types, DKD, HPKE, threshold signature logic |
| `aura-journal` | CRDT State | Eventually-consistent ledger, semilattice merge, ratchet tree |
| `aura-wot` | Trust/Authorization | Capability refinement, meet-semilattice, trust relationships |
| `aura-verify` | Identity | Complete identity system: cryptographic verification + device lifecycle |
| `aura-store` | Storage Impl | Storage backend implementations |
| `aura-transport` | Transport | P2P communication abstractions |

**Key characteristics**:
- Implement `aura-core` traits for domain-specific types
- No effect handlers - pure domain logic
- Define semantics and data structures

### Choreography Specification: `aura-mpst` & `aura-macros`

#### Runtime Library: `aura-mpst`

Provides semantic abstractions for Aura-specific choreographic features:

- **Session type extensions**: `CapabilityGuard`, `JournalCoupling`, `LeakageBudget`, `ContextIsolation` traits
- **Protocol types**: Abstract interfaces that define what choreographic operations must enforce
- **Type-level guarantees**: Session type algebra for deadlock freedom and type safety
- **No code generation**: Only traits, types, and runtime implementations
- **Used by**: 7 crates (aura-protocol, aura-frost, aura-authenticate, aura-recovery, aura-invitation, aura-storage, aura-rendezvous)

#### Compile-Time Tool: `aura-macros`

Provides code generation for choreography DSL syntax with Aura-specific extensions:

- **Choreography parser**: `choreography!` and `choreography!` macros with annotation support
- **Annotation extraction**: Parses `guard_capability`, `flow_cost`, `journal_facts` from choreography syntax
- **Code generation**: Converts DSL syntax into Rust trait implementations + extension registry calls
- **Output**: Session types, guard profiles, journal coupling code, handler setup, aura-mpst integration
- **Architecture**: Hybrid approach - strips Aura annotations then delegates to `rumpsteak-aura` for session type safety
- **Security features**: Complete annotation parsing with proper syn-based validation and error handling
- **Used by**: 2 crates (aura-recovery, aura-invitation) - feature crates that want to define choreographies via DSL

#### Relationship: Compiler + Library

Think of it like a language implementation:

```
Choreography DSL Syntax
        ↓
    aura-macros (compiler)
        ↓
Generated Rust Code
        ↓
    aura-mpst (stdlib)
        ↓
Runtime Types & Traits
```

**aura-macros** (compile-time):
- Parses choreography syntax from `.rs` files with Aura-specific annotations
- Extracts and validates `guard_capability`, `flow_cost`, `journal_facts` annotations
- Generates type-safe Rust code with guard profiles, coupling code, extension registrations
- Uses hybrid parsing: syn-based structural analysis + string-based annotation extraction
- Strips annotations and delegates clean DSL to `rumpsteak-aura` for session type soundness
- Deduplicates message types and generates role/message type definitions

**aura-mpst** (runtime):
- Provides `CapabilityGuard`, `JournalCoupling` traits that generated code implements
- Provides `LeakageBudget`, `ContextIsolation` types for protocol verification with security policies
- Provides `AuraRuntime`, `ExecutionContext` for orchestration
- **Security-first**: `LeakageTracker` defaults to deny undefined budgets (secure by default)
- **Backward compatibility**: `legacy_permissive()` mode available for existing code
- Works with hand-written protocols too (not just macro-generated ones)

**Example**: A choreography DSL with guard annotations:

```rust
// Input: DSL in aura-macros
choreography! {
    #[namespace = "secure_messaging"]
    protocol SecureMessage {
        roles: Alice, Bob;
        
        Alice[guard_capability = "send", flow_cost = 50] -> Bob: Message;
        Bob[journal_facts = "message_received"] -> Alice: Ack;
    }
}

// Output: Generated Rust code using aura-mpst types
pub mod secure_messaging {
    // Session types (rumpsteak-aura compatible)
    pub mod session_types {
        pub struct SecureMessage;
        pub mod roles { pub struct Alice; pub struct Bob; }
        pub mod messages { pub struct Message; pub struct Ack; }
    }
    
    // Extension registry for aura-mpst runtime integration
    pub mod extensions {
        pub fn register_extensions(registry: &mut aura_mpst::ExtensionRegistry) {
            registry.register_guard("send", "Alice");
            registry.register_flow_cost(50, "Alice");
            registry.register_journal_fact("message_received", "Bob");
        }
    }
}
```

#### Security-First Design Philosophy

Both `aura-macros` and `aura-mpst` implement **security by default** with backward compatibility:

**Privacy Budget Enforcement**:
```rust
// Secure by default - denies undefined budgets
let tracker = LeakageTracker::new(); // UndefinedBudgetPolicy::Deny

// Legacy compatibility mode
let tracker = LeakageTracker::legacy_permissive(); // UndefinedBudgetPolicy::Allow

// Configurable policy
let tracker = LeakageTracker::with_undefined_policy(
    UndefinedBudgetPolicy::DefaultBudget(1000)
);
```

**Annotation Parsing**:
- Robust syn-based validation prevents malformed choreographies from compiling
- Proper error messages guide developers toward secure patterns
- All TODOs and production placeholders completed for deployment readiness

Note: `aura-sync` intentionally removed the `aura-macros` dependency but still uses `aura-mpst` types through `aura-protocol` - demonstrating that the semantic library is independent of the DSL compiler.

---

## Layer 3: Implementation — `aura-effects`

**Purpose**: Standard library of context-free effect handlers that work in any execution context


Provides **stateless, single-operation implementations** of `aura-core` effect traits:

### Mock Handlers (Testing)
- `MockCryptoHandler` - Deterministic signatures for reproducible tests
- `MockNetworkHandler` - Simulated peer-to-peer communication
- `InMemoryStorageHandler` - Ephemeral storage for testing
- `MockTimeHandler` - Controllable time for deterministic scheduling

### Real Handlers (Production)
- `RealCryptoHandler` - Actual cryptographic operations
- `TcpNetworkHandler` - Real TCP/network communication
- `FilesystemStorageHandler` - Persistent disk storage
- `RealTimeHandler` - System clock for scheduling

**Key characteristics**:
- **Stateless**: Single operation → result (e.g., `sign(key, msg) → sig`)
- **Single-party**: Works for one device/handler in isolation
- **Context-free**: Doesn't assume choreographic execution or multi-party coordination
- Each handler implements one effect trait independently

**Dependencies**: `aura-core` + external libraries (tokio, serde, blake3, etc.)

### What's NOT in `aura-effects`:
- Multi-handler composition
- Choreography bridges
- Multi-party sync logic
- Coordination state

---

## Layer 4: Orchestration — `aura-protocol`

**Purpose**: Coordinate effects across multiple parties or handlers in distributed systems

Provides **stateful coordination infrastructure** for multi-party execution:

### Core Coordination Primitives

**Handler Orchestration**:
- `AuraHandlerAdapter` - Bridges choreography DSL to effect handlers
- `CompositeHandler` - Composes multiple effect handlers (stateful)
- `CrdtCoordinator` - Coordinates 4+ CRDT handler types for distributed sync
- `GuardChain` - Authorization pipeline: `CapGuard → FlowGuard → JournalCoupler`

**Middleware & Cross-Cutting Concerns**:
- Circuit breakers for fault isolation
- Retry logic with exponential backoff
- Protocol-level authorization and validation

### Reusable Coordination Patterns

Common distributed protocols used across applications:

| Protocol | Purpose |
|----------|---------|
| `anti_entropy` | CRDT synchronization choreography |
| `consensus` | Byzantine fault-tolerant agreement |
| `snapshot` | Coordinated garbage collection |
| `threshold_ceremony` | Privacy-preserving threshold signing |

**Example: Single Operation vs Coordination**

```rust
// Layer 3: Single operation (aura-effects)
impl CryptoEffects for RealCryptoHandler {
    async fn sign(&self, key: &SecretKey, msg: &[u8]) -> Signature {
        // One device, one operation, no coordination
    }
}

// Layer 4: Coordinated multi-party operation (aura-protocol)
pub async fn execute_anti_entropy(
    coordinator: CrdtCoordinator,      // Coordinates 4 CRDT handlers
    adapter: AuraHandlerAdapter,       // Coordinates choreography + effects
    guards: GuardChain,                // Coordinates authorization
) -> Result<SyncResult> {
    // Orchestrates distributed sync across parties
}
```

**Key characteristics**:
- **Stateful**: Maintains coordination state across operations
- **Multi-party**: Assumes distributed execution or multi-handler orchestration
- **Context-specific**: Requires choreographic or synchronization context

**Dependencies**: `aura-core` + `aura-effects` + `aura-mpst`

---

## Layer 5: Feature/Protocol Implementation

**Purpose**: Complete end-to-end protocol implementations - more than coordination primitives but less than full applications

**Crates**:

| Crate | Protocol | Purpose |
|-------|----------|---------|
| `aura-authenticate` | Authentication | Device and guardian authentication choreographies (G_auth, session establishment) |
| `aura-frost` | FROST | Threshold signature choreography, reference implementation |
| `aura-invitation` | Invitation | Peer onboarding, content-addressed invitations |
| `aura-recovery` | Recovery | Guardian recovery ceremonies, dispute escalation |
| `aura-rendezvous` | Peer Discovery | Secret-Branded Broadcasting (SBB) and peer discovery protocols |
| `aura-storage` | Storage | Capability-based storage with G_search and G_gc choreographies |
| `aura-sync` | Synchronization | Journal synchronization and anti-entropy protocols |

**Characteristics**:
- Implement complete business logic and protocol flows
- Use choreography macros from `aura-mpst`
- Compose handlers from `aura-effects`
- Use coordination primitives from `aura-protocol`
- No UI or main entry points - designed to be composed into larger systems
- Reusable building blocks for applications

---

## Layer 6: Runtime Composition

**Purpose**: Assemble effect handlers and protocols into working agent runtimes

These are **libraries**, not binaries. They provide runtime infrastructure that user interface layers drive:

### `aura-agent` — Production Runtime
- Composes all effect handlers and protocols
- Manages agent lifecycle and state
- Provides APIs for driving the agent
- Used by CLI, web UI, and other interfaces

### `aura-simulator` — Testing Runtime
- Deterministic runtime for reproducible testing
- Controlled scheduling and injectable effects
- Property-based testing support
- Used by test harnesses and formal verification

**Analogy**: Think of `tokio` (runtime library) vs `main.rs` (application):
- `aura-agent` = runtime that composes and executes protocols
- `aura-cli` = application that instantiates and drives an agent

**Dependencies**: All domain crates + `aura-effects` + `aura-protocol`

---

## Layer 7: User Interface — What Users Actually Run

**Purpose**: Provide user-facing applications with main entry points

**Crates**:

| Crate | Interface | Purpose |
|-------|-----------|---------|
| `aura-cli` | Terminal | Command-line tools for account management, testing |
| `app-console` | Web UI | Developer console (planned) |
| `app-wasm` | Browser | WebAssembly bindings (planned) |

**Key characteristic**: These have `main()` entry points. Users run these directly.

**Relationship**: Drive the `aura-agent` runtime from the UI layer, translating user actions into protocol operations.

---

## Layer 8: Testing & Development Tools

**Purpose**: Provide cross-cutting test utilities and formal verification bridges

**Crates**:

| Crate | Purpose |
|-------|---------|
| `aura-testkit` | Shared test fixtures, scenario builders, property test helpers |
| `aura-quint-api` | Formal verification bridge to Quint model checker |

---

## Code Location Decision Matrix

Use these questions to classify code and determine the correct crate:

| Pattern | Answer | Location |
|---------|--------|----------|
| Implements single effect trait method | Stateless + single operation | `aura-effects` |
| Coordinates multiple effects/handlers | Stateful + multi-handler | `aura-protocol` |
| Multi-party coordination logic | Distributed state + orchestration | `aura-protocol` |
| Domain-specific types/semantics | Pure logic + no handlers | Domain crate or `aura-mpst` |
| Complete reusable protocol | End-to-end + no UI | Feature/Protocol crate |
| Assembles handlers + protocols | Runtime composition | `aura-agent` or `aura-simulator` |
| User-facing application | Has main() | `aura-cli` or `app-*` |

### Boundary Questions for Edge Cases

1. **Is it stateless or stateful?**
   - Stateless + single operation → `aura-effects`
   - Stateful + coordination → `aura-protocol`

2. **Does it work for one party or multiple?**
   - Single-party → `aura-effects`
   - Multi-party → `aura-protocol`

3. **Is it context-free or context-specific?**
   - Context-free (works anywhere) → `aura-effects`
   - Context-specific (requires orchestration) → `aura-protocol`

4. **Does it coordinate multiple handlers?**
   - No → `aura-effects`
   - Yes → `aura-protocol`

---

## Architecture Principles

### No Circular Dependencies
Each layer builds on lower layers without reaching back down. This enables:
- Testability: Mock any layer independently
- Reusability: Lower layers work in any context
- Clear responsibility: Each layer answers one question

### "What vs How" Mental Model

| Layer | Answers | Form | Example |
|-------|---------|------|---------|
| `aura-core` | "What operations exist?" | Interfaces | `trait CryptoEffects` |
| Domain crates | "What does this mean?" | Types + Logic | `JournalMap`, merge semantics |
| `aura-mpst` | "How do parties communicate?" | Protocols | Choreography macros |
| `aura-effects` | "How do I do ONE thing?" | Single operations | `sign()`, `store_chunk()` |
| `aura-protocol` | "How do I COORDINATE?" | Multi-party orchestration | `execute_anti_entropy()` |
| Feature crates | "What features exist?" | Complete protocols | FROST ceremony |
| Runtime layer | "How do I assemble?" | Handler composition | `AuraAgent` |
| UI layer | "What do users run?" | Binaries | CLI commands |

### Critical Distinction: Effects vs Coordination

**`aura-effects` (Single operations)**:
- `sign(key, msg) → Signature` - No coordination needed
- `store_chunk(id, data) → Ok(())` - One device, one write
- `RealCryptoHandler` - Self-contained cryptographic operation

**`aura-protocol` (Coordination)**:
- `execute_anti_entropy(...)` - Orchestrates sync across parties
- `CrdtCoordinator` - Manages state of multiple CRDT handlers
- `GuardChain` - Coordinates authorization checks across operations

---

## Typical Workflow

### Adding a New Cryptographic Primitive

1. Define type in appropriate domain crate (`aura-crypto`)
2. Implement `aura-core` traits for semantics
3. Add single-operation handler in `aura-effects`
4. Use in feature crates or protocols

### Adding a New Distributed Protocol

1. Write choreography in `aura-mpst` using session types or DSL with `aura-macros`
2. Use annotation syntax for security: `Role[guard_capability = "...", flow_cost = N] -> Target: Message`
3. Create protocol implementation in `aura-protocol` or feature crate
4. Implement coordination logic using handlers from `aura-effects`
5. Wire into `aura-agent` runtime with proper leakage budget policies
6. Expose through CLI or application

### Writing a New Test

1. Create test fixtures in `aura-testkit`
2. Use mock handlers from `aura-effects` for reproducibility
3. Configure appropriate leakage budget policies for test scenarios
4. Drive agent from test harness
5. Compose protocols using `aura-simulator` for determinism

## Implementation Status

**Production Ready (No TODOs)**:
- `aura-macros`: Complete annotation parsing, session type generation, rumpsteak-aura integration
- `aura-mpst`: Security-first leakage tracking with configurable policies, full choreography runtime

**Security Improvements**:
- Privacy budgets now deny undefined access by default (breaking change with legacy compatibility)
- Robust error handling and validation in all parsing and code generation paths
- All placeholder "in production" comments replaced with complete implementations
