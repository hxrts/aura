# System Internals Guide

This guide covers deep system patterns for contributors working on Aura core. Use it when you need to understand guard chain internals, service layer patterns, core types, and reactive scheduling.

## 1. Guard Chain Internals

The guard chain coordinates authorization, flow budgets, and journal effects in strict sequence. See [Authorization](106_authorization.md) for the full specification.

### Three-Phase Pattern

Guards are pure: evaluation runs synchronously over a prepared `GuardSnapshot` and yields `EffectCommand` items that an async interpreter executes.

```rust
// Phase 1: Authorization via Biscuit + policy (async, cached)
let token = effects.verify_biscuit(&request.token).await?;
let capabilities = evaluate_candidate_frontier(
    &token,
    evaluation_candidates_for_chat_guard(),
    &policy,
)?;

// Phase 2: Prepare snapshot and evaluate guards (sync)
let snapshot = GuardSnapshot {
    capabilities,
    flow_budget: current_budget,
    leakage_budget: current_leakage,
    ..Default::default()
};

let commands = guard_chain.evaluate(&snapshot, &request)?;

// Phase 3: Execute commands (async)
for command in commands {
    match command {
        EffectCommand::ChargeBudget { cost } => {
            budget_handler.charge(cost).await?;
        }
        EffectCommand::RecordLeakage { budget } => {
            leakage_handler.record(budget).await?;
        }
        EffectCommand::CommitJournal { facts } => {
            journal_handler.commit(facts).await?;
        }
        EffectCommand::SendTransport { message } => {
            transport_handler.send(message).await?;
        }
    }
}
```

No transport observable occurs until the interpreter executes commands in order.

### Guard Chain Sequence

The guards execute in this order:

1. **CapabilityGuard**: Validates the evaluated Biscuit/policy frontier
2. **FlowBudgetGuard**: Checks and charges flow budget
3. **LeakageTracker**: Records privacy leakage
4. **JournalCoupler**: Commits facts to journal
5. **TransportEffects**: Sends messages

### Security Patterns

**Privacy Budget Enforcement**:

```rust
// Secure default: denies undefined budgets
let tracker = LeakageTracker::new();

// Backward compatibility: allows undefined budgets
let tracker = LeakageTracker::legacy_permissive();

// Configurable default
let tracker = LeakageTracker::with_undefined_policy(DefaultBudget(1000));
```

## 2. Service Layer Patterns

Domain crates define stateless handlers that take effect references per-call. The agent layer wraps these with services that manage RwLock access.

### Domain Handler (Layer 2-5)

```rust
// In domain crate (e.g., aura-chat/src/service.rs)
pub struct ChatFactService;

impl ChatFactService {
    pub fn new() -> Self { Self }

    pub async fn send_message<E>(
        &self,
        effects: &E,
        channel_id: ChannelId,
        content: String,
    ) -> Result<MessageId>
    where
        E: StorageEffects + RandomEffects + PhysicalTimeEffects
    {
        let message_id = effects.random_uuid().await;
        let timestamp = effects.physical_time().await?;
        // ... domain logic using effects
        Ok(message_id)
    }
}
```

### Agent Service Wrapper (Layer 6)

```rust
// In aura-agent/src/handlers/chat_service.rs
pub struct ChatService {
    handler: ChatFactService,
    effects: Arc<RwLock<AuraEffectSystem>>,
}

impl ChatService {
    pub fn new(effects: Arc<RwLock<AuraEffectSystem>>) -> Self {
        Self { handler: ChatFactService::new(), effects }
    }

    pub async fn send_message(
        &self,
        channel_id: ChannelId,
        content: String,
    ) -> AgentResult<MessageId> {
        let effects = self.effects.read().await;
        self.handler.send_message(&*effects, channel_id, content)
            .await
            .map_err(Into::into)
    }
}
```

### Agent API Exposure

```rust
// In aura-agent/src/core/api.rs
impl AuraAgent {
    pub fn chat_service(&self) -> ChatService {
        ChatService::new(self.runtime.effects())
    }
}
```

**Benefits**:
- Domain crate stays pure (no tokio/RwLock)
- Testable with mock effects
- Consistent pattern across crates

### Core + Orchestrator Rule

The Core + Orchestrator Rule is defined in [System Architecture](001_system_architecture.md). Layer 4 crates split logic into pure core modules and effectful orchestrator modules.

## 3. Type Reference

### ProtocolType

Canonical definition in `aura-core`. All crates re-export this definition.

```rust
pub enum ProtocolType {
    Dkd,        // Deterministic Key Derivation
    Counter,    // Counter reservation protocol
    Resharing,  // Key resharing for threshold updates
    Locking,    // Resource locking protocol
    Recovery,   // Account recovery protocol
    Compaction, // Ledger compaction protocol
}
```

### SessionStatus

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

The time domain system is specified in [Effect System](103_effect_system.md). See that document for domain definitions, effect trait mappings, and usage constraints.

### Capability System Layering

The capability system uses multiple layers:

- **Canonical types** in `aura-core`: validated `CapabilityName`
- **Owning families** in feature/domain crates: typed first-party capability
  declarations
- **Authorization layer** (`aura-authorization`): explicit issuance profiles and
  Biscuit/policy evaluation
- **Guard snapshots** (`aura-guards` plus runtime handlers): evaluated
  frontiers only
- **Storage layer** (`aura-store`): capability-based access control

Clear conversion paths enable inter-layer communication.

## 4. Reactive Scheduling

The `ReactiveScheduler` in `aura-agent/src/reactive/` processes journal facts and emits application signals.

### Signal System Overview

```rust
// Application signals
pub const CHAT_SIGNAL: &str = "chat";
pub const CONTACTS_SIGNAL: &str = "contacts";
pub const CHANNELS_SIGNAL: &str = "channels";
pub const RECOVERY_SIGNAL: &str = "recovery";
```

The scheduler:
1. Subscribes to journal fact streams
2. Reduces facts to view state
3. Emits signals when state changes
4. TUI/CLI components subscribe to signals

### TUI Reactive State

The TUI uses `futures-signals` for fine-grained reactive state management.

The reactive architecture pattern below represents the target design for TUI state management. Implementation status varies by view.

#### Signal Types

```rust
use futures_signals::signal::Mutable;
use futures_signals::signal_vec::MutableVec;

// Single reactive value
let count = Mutable::new(0);
count.set(5);
let value = count.get_cloned();

// Reactive collection
let items = MutableVec::new();
items.lock_mut().push_cloned("item1");
```

#### View Pattern

```rust
pub struct ChatView {
    channels: MutableVec<Channel>,
    messages: MutableVec<Message>,
    selected_channel: Mutable<Option<String>>,
}

impl ChatView {
    // Synchronous delta application
    pub fn apply_delta(&self, delta: ChatDelta) {
        match delta {
            ChatDelta::ChannelAdded { channel } => {
                self.channels.lock_mut().push_cloned(channel);
                // Signals automatically notify subscribers
            }
            ChatDelta::MessageReceived { channel_id, message } => {
                if self.selected_channel.get_cloned() == Some(channel_id) {
                    self.messages.lock_mut().push_cloned(message);
                }
            }
        }
    }
}
```

#### Best Practices

- Delta application should be synchronous (not async)
- Use `.get_cloned()` for reading, `.set()` for mutations
- Never hold lock guards across await points
- Use derived signals for computed values

## 5. Policy Compliance

Application code must follow policies defined in [Project Structure](999_project_structure.md).

### Impure Function Usage

All time, randomness, filesystem, and network operations must flow through effect traits.

**Forbidden**:
```rust
// Direct system calls break simulation and WASM
let now = SystemTime::now();
let random = thread_rng().gen();
let file = File::open("path")?;
```

**Required**:
```rust
// Use effect traits
let now = effects.physical_time().await?;
let random = effects.random_bytes(32).await?;
let data = effects.read_storage("key").await?;
```

### Serialization

- Wire protocols and facts: DAG-CBOR via `aura_core::util::serialization`
- User-facing configs: JSON allowed
- Debug output: JSON allowed

### Architectural Validation

Run `just check-arch` before submitting changes. The checker validates:
- Layer boundaries
- Effect trait placement
- Impure function routing
- Guard chain integrity

## 6. Architecture Compliance Checklist

- [ ] Layer dependencies flow downward only
- [ ] Effect traits defined in `aura-core` only
- [ ] Infrastructure effects implemented in `aura-effects`
- [ ] Application effects in domain crates
- [ ] No direct impure function usage outside effect implementations
- [ ] All async functions propagate `EffectContext`
- [ ] Production handlers are stateless, test handlers in `aura-testkit`
- [ ] Guard chain sequence respected

## Workflow Error Types

Workflow operations in `aura-app` use `WorkflowError` (`aura-app::workflows::error`) for typed error propagation. The enum provides structured variants for common failure modes:

- `RuntimeUnavailable` — runtime bridge not initialized
- `RuntimeCall { operation, source }` — a named runtime bridge call failed
- `ConnectivityRequired` — peer connectivity prerequisite not met
- `Journal { operation, source }` — journal load/merge/persist failure
- `FactEncoding { source }` — fact serialization failure
- `Ceremony { operation, source }` — ceremony lifecycle failure
- `DeliveryFailed { peer, attempts, detail }` — transport delivery exhausted retries
- `Precondition` — static invariant violation

`From<WorkflowError> for AuraError` enables workflows to keep `Result<T, AuraError>` signatures while constructing typed errors internally.

## 7. Instrumentation Contract

The instrumentation contract is specified in [Runtime](104_runtime.md). All long-lived services must emit the required event families defined there.

## Related Documentation

- [Effect System](103_effect_system.md) - Effect specification
- [Runtime](104_runtime.md) - Runtime specification
- [Authorization](106_authorization.md) - Guard chain specification
- [System Architecture](001_system_architecture.md) - Layer boundaries
- [CLI and TUI](117_user_interface.md) - Terminal specification
