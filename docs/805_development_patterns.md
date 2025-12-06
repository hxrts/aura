# Development Patterns and Workflows

This document covers practical patterns and workflows for developing Aura systems.

## Effects vs Coordination

A critical distinction guides where code belongs in the architecture.

### Single-Party Operations

`aura-effects` implements single-party operations that are stateless and context-free. Each operation takes input and produces output without maintaining state or coordinating with other handlers.

Examples:
- `sign(key, msg) → Signature` - One device, one cryptographic operation
- `store_chunk(id, data) → Ok(())` - One device, one write
- `RealCryptoHandler` - Self-contained cryptographic operations
- `MockNetworkHandler` - Simulated peer communication for testing

Single-party operations are reusable in any context. They work in unit tests, integration tests, and production equally well.

### Multi-Party Coordination

`aura-protocol` implements multi-party coordination where multiple handlers orchestrate together. Operations are stateful and context-specific.

Examples:
- `execute_anti_entropy(...)` - Orchestrates sync across multiple parties
- `CrdtCoordinator` - Manages state of multiple CRDT handlers
- `GuardChain` - Coordinates authorization checks across sequential operations

Multi-party coordination requires a context. It assumes multiple handlers are involved and state is maintained across operations.

The distinction is critical for understanding where code belongs. Single-party operations go in `aura-effects`. Multi-party coordination goes in `aura-protocol`.

## Code Location Decision Matrix

Use these questions to classify code and determine the correct crate.

| Pattern | Answer | Location |
|---------|--------|----------|
| Implements single effect trait method | Stateless and single operation | `aura-effects` |
| Coordinates multiple effects or handlers | Stateful and multi-handler | `aura-protocol` |
| Multi-party coordination logic | Distributed state and orchestration | `aura-protocol` |
| Domain-specific types and semantics | Pure logic without handlers | Domain crate or `aura-mpst` |
| Complete reusable protocol | End-to-end without UI | Feature/protocol crate |
| Assembles handlers and protocols | Runtime composition | `aura-agent` or `aura-simulator` |
| User-facing application | Has main() entry point | `aura-terminal` or `app-*` |

### Boundary Questions for Edge Cases

**Is it stateless or stateful?**

Stateless and single operation go in `aura-effects`. Stateful and coordinating go in `aura-protocol`.

**Does it work for one party or multiple?**

Single-party code goes in `aura-effects`. Multi-party code goes in `aura-protocol`.

**Is it context-free or context-specific?**

Context-free code (works anywhere) goes in `aura-effects`. Context-specific code (requires orchestration) goes in `aura-protocol`.

**Does it coordinate multiple handlers?**

No coordination goes in `aura-effects`. Multiple handlers being orchestrated goes in `aura-protocol`.

## Typical Workflows

### Adding a New Cryptographic Primitive

1. Define the type in `aura-core` crypto module
2. Implement `aura-core` traits for the type's semantics
3. Add a single-operation handler in `aura-effects` that implements the primitive
4. Use the handler in feature crates or protocols through the effect system

### Adding a New Distributed Protocol

1. Write the choreography in `aura-mpst` using session types or DSL syntax with `aura-macros`
2. Use annotation syntax for security: `Role[guard_capability = "...", flow_cost = N] -> Target: Message`
3. Create the protocol implementation in `aura-protocol` or a feature crate
4. Implement the coordination logic using handlers from `aura-effects`
5. Wire the protocol into `aura-agent` runtime with appropriate leakage budget policies
6. Expose the protocol through CLI or application interfaces

### Writing a New Test

1. Create test fixtures in `aura-testkit`
2. Use mock handlers from `aura-effects` for reproducibility
3. Configure appropriate leakage budget policies for the test scenarios
4. Drive the agent from the test harness
5. Compose protocols using `aura-simulator` for deterministic execution

## Type Consolidation and Single Source of Truth

### ProtocolType

The canonical definition of `ProtocolType` lives in `aura-core`. All other crates re-export and use this canonical definition.

Variants:
- `Dkd` - Deterministic Key Derivation
- `Counter` - Counter reservation protocol
- `Resharing` - Key resharing for threshold updates
- `Locking` - Resource locking protocol
- `Recovery` - Account recovery protocol
- `Compaction` - Ledger compaction protocol

Usage is consistent across `aura-protocol` and `aura-simulator`.

### SessionStatus

The canonical definition of `SessionStatus` lives in `aura-core`. Variants represent the session lifecycle.

Lifecycle order:
1. `Initializing` - Session initializing before execution
2. `Active` - Session currently executing
3. `Waiting` - Session waiting for participant responses
4. `Completed` - Session completed successfully
5. `Failed` - Session failed with error
6. `Expired` - Session expired due to timeout
7. `TimedOut` - Session timed out during execution
8. `Cancelled` - Session was cancelled

All crates that track session state use the canonical definition from `aura-core`.

### Capability System Layering

The capability system intentionally uses multiple architectural layers. Each layer serves legitimate purposes.

- **Canonical types** in `aura-core` provide lightweight references
- **Authorization layer** (`aura-wot`) adds policy enforcement features
- **Storage layer** (`aura-store`) implements capability-based access control

Clear conversion paths enable inter-layer communication without confusion.

## Effect Handler Patterns

### Stateless Handler Pattern

Effect handlers follow a consistent pattern. Each handler implements one or more effect traits from `aura-core`.

A handler is stateless. It receives input, performs a single operation, and returns output. No state is maintained between calls.

Example:

```rust
pub struct RealCryptoHandler;

impl CryptoEffects for RealCryptoHandler {
    async fn sign(&self, key: &SecretKey, msg: &[u8]) -> Signature {
        // Single operation: sign the message
    }
    
    async fn verify(&self, key: &PublicKey, msg: &[u8], sig: &Signature) -> bool {
        // Single operation: verify the signature
    }
}
```

Mock handlers follow the same pattern but use deterministic or simulated implementations:

```rust
pub struct MockCryptoHandler;

impl CryptoEffects for MockCryptoHandler {
    async fn sign(&self, key: &SecretKey, msg: &[u8]) -> Signature {
        // Deterministic mock signature for testing
    }
    
    async fn verify(&self, key: &PublicKey, msg: &[u8], sig: &Signature) -> bool {
        // Always returns true in testing mode
    }
}
```

### Multi-Party Coordination Pattern

Coordination logic in `aura-protocol` manages multiple handlers working together.

Coordination functions are async and stateful. They orchestrate handlers to accomplish multi-party goals.

Example:

```rust
pub async fn execute_anti_entropy(
    coordinator: CrdtCoordinator,      // Coordinates multiple CRDT handlers
    adapter: AuraHandlerAdapter,       // Coordinates choreography and effects
    guards: GuardChain,                // Coordinates authorization
) -> Result<SyncResult> {
    // Orchestrates distributed sync across parties
}
```

Coordination functions typically accept multiple handlers or a composed system. They maintain state across multiple operations. They coordinate between different concerns like authorization, storage, and transport. They return results that depend on the combined state.

### Guard Chain Execution Pattern

The guard chain coordinates authorization, flow budgets, and journal effects in strict sequence. Guards themselves are pure: evaluation runs synchronously over a prepared `GuardSnapshot` and yields `EffectCommand` items that an async interpreter executes. This keeps guard logic deterministic and prevents observable side effects from failed authorization attempts.

```rust
async fn send_storage_put(
    bridge: &BiscuitAuthorizationBridge,
    guards: &GuardChain,
    interpreter: &dyn EffectInterpreter,
    ctx: ContextId,
    peer: AuthorityId,
    token: Biscuit,
    payload: PutRequest,
) -> Result<()> {
    // Phase 1: Authorization via Biscuit + policy (async, cached)
    let auth_result = bridge.authorize(&token, "storage_write", &payload.scope())?;
    if !auth_result.authorized {
        return Err(AuraError::permission_denied("Token authorization failed"));
    }

    // Phase 2: Prepare snapshot (async) and evaluate guards (sync)
    let snapshot = prepare_guard_snapshot(ctx, peer, &auth_result.cap_frontier).await?;
    let outcome = guards.evaluate(&snapshot, &payload.guard_request());
    if outcome.decision.is_denied() {
        return Err(AuraError::permission_denied("Guard evaluation denied"));
    }

    // Phase 3: Execute commands (async) - charge, record leakage, commit journal, send transport
    for cmd in outcome.effects {
        interpreter.exec(cmd).await?;
    }

    Ok(())
}
```

This pattern implements the guard chain guarantee: snapshot preparation happens before synchronous guard evaluation, and no transport observable occurs until the interpreter executes the resulting commands in order.

## Security-First Design Philosophy

### Privacy Budget Enforcement

The leakage tracking system implements security by default with backward compatibility.

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

The default policy is to deny access to undefined budgets. This prevents accidental privacy violations. Legacy mode is available for existing code that needs to operate without strict budget enforcement.

### Annotation Parsing

Robust syn-based validation prevents malformed choreographies from compiling. Proper error messages guide developers toward secure patterns. All placeholders have been replaced with complete implementations for deployment readiness.

The choreography compiler validates annotations at compile time. Invalid or missing annotations are rejected with helpful error messages that explain the requirement.

## Creating a New Domain Service

Domain crates define stateless handlers that take effect references per-call. The agent layer wraps these with services that manage RwLock access.

### Step 1: Create the Domain Handler

In the domain crate (e.g., `aura-chat/src/service.rs`):

```rust
/// Stateless handler - takes effect reference per-call
pub struct MyHandler;

impl MyHandler {
    pub fn new() -> Self { Self }

    pub async fn my_operation<E>(
        &self,
        effects: &E,  // <-- Per-call reference
        param: SomeType,
    ) -> Result<Output>
    where
        E: StorageEffects + RandomEffects + PhysicalTimeEffects
    {
        // Use effects for side effects
        let uuid = effects.random_uuid().await;
        // ... domain logic
    }
}
```

### Step 2: Create the Agent Service Wrapper

In `aura-agent/src/handlers/my_service.rs`:

```rust
pub struct MyService {
    handler: MyHandler,
    effects: Arc<RwLock<AuraEffectSystem>>,
}

impl MyService {
    pub fn new(effects: Arc<RwLock<AuraEffectSystem>>) -> Self {
        Self {
            handler: MyHandler::new(),
            effects,
        }
    }

    pub async fn my_operation(&self, param: SomeType) -> AgentResult<Output> {
        let effects = self.effects.read().await;  // <-- Acquire lock
        self.handler
            .my_operation(&*effects, param)
            .await
            .map_err(Into::into)
    }
}
```

### Step 3: Expose via Agent API

In `aura-agent/src/core/api.rs`:

```rust
impl AuraAgent {
    pub fn my_service(&self) -> MyService {
        MyService::new(self.runtime.effects())
    }
}
```

### Benefits

- **Domain crate stays pure**: No tokio/RwLock dependency
- **Testable**: Pass mock effects directly in unit tests
- **Consistent**: Same pattern across all domain crates
- **Safe**: RwLock managed automatically at agent layer

See `docs/106_effect_system_and_runtime.md` section 13 for more details.
