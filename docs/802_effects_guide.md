# Effects and Handlers Guide

This guide covers how to work with Aura's algebraic effect system. Use it when you need to extend the system at its boundaries: adding handlers, implementing platform support, or creating new effect traits.

For the full effect system specification, see [Effect System](103_effect_system.md).

## 1. Code Location

A critical distinction guides where code belongs in the architecture.

Single-party operations go in `aura-effects`. These are stateless, context-free handlers that take input and produce output without maintaining state or coordinating with other handlers.

Examples:
- `sign(key, msg) -> Signature` - one device, one cryptographic operation
- `store_chunk(id, data) -> Ok(())` - one device, one write
- `RealCryptoHandler` - self-contained cryptographic operations

Multi-party coordination goes in `aura-protocol`. These orchestrate multiple handlers together with stateful, context-specific operations.

Examples:
- `execute_anti_entropy(...)` - orchestrates sync across multiple parties
- `CrdtCoordinator` - manages state of multiple CRDT handlers
- `GuardChain` - coordinates authorization checks across sequential operations

If removing one effect handler requires changing the logic of how other handlers are called (not just removing calls), it belongs in Layer 4 as orchestration.

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

### Boundary Questions

- **Stateless or stateful?** Stateless goes in `aura-effects`. Stateful goes in `aura-protocol`.
- **One party or multiple?** Single-party goes in `aura-effects`. Multi-party goes in `aura-protocol`.
- **Context-free or context-specific?** Context-free goes in `aura-effects`. Context-specific goes in `aura-protocol`.

## 2. Effect Handler Pattern

Effect handlers are stateless. Each handler implements one or more effect traits from `aura-core`. It receives input, performs a single operation, and returns output. No state is maintained between calls.

Production handlers (like `RealCryptoHandler`) use real libraries. Mock handlers (like `MockCryptoHandler` in `aura-testkit`) use deterministic implementations for testing.

See [Cryptographic Architecture](100_crypto.md) for cryptographic handler requirements.

### Implementing a Handler

**Step 1: Define the trait in `aura-core`**

```rust
#[async_trait]
pub trait MyEffects: Send + Sync {
    async fn my_operation(&self, input: Input) -> Result<Output, EffectError>;
}
```

**Step 2: Implement the production handler in `aura-effects`**

```rust
pub struct RealMyHandler;

#[async_trait]
impl MyEffects for RealMyHandler {
    async fn my_operation(&self, input: Input) -> Result<Output, EffectError> {
        // Implementation using real libraries
    }
}
```

**Step 3: Implement the mock handler in `aura-testkit`**

```rust
pub struct MockMyHandler {
    seed: u64,
}

#[async_trait]
impl MyEffects for MockMyHandler {
    async fn my_operation(&self, input: Input) -> Result<Output, EffectError> {
        // Deterministic implementation for testing
    }
}
```

### Adding a Cryptographic Primitive

1. Define the type in `aura-core` crypto module
2. Implement `aura-core` traits for the type's semantics
3. Add a single-operation handler in `aura-effects` that implements the primitive
4. Use the handler in feature crates or protocols through the effect system

## 3. Platform Implementation

Use the `AgentBuilder` API to assemble the runtime with appropriate effect handlers for each platform.

### Builder Strategies

| Strategy | Use Case | Compile-Time Safety |
|----------|----------|---------------------|
| Platform preset | Standard platforms (CLI, iOS, Android, Web) | Configuration validation |
| Custom preset | Full control over all effects | Typestate enforcement |
| Effect overrides | Preset with specific customizations | Mixed |

### Platform Presets

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

### Custom Preset with Typestate

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

### Required Effects

| Effect | Purpose | Trait |
|--------|---------|-------|
| Crypto | Signing, verification, encryption | `CryptoEffects` |
| Storage | Persistent data storage | `StorageEffects` |
| Time | Wall-clock timestamps | `PhysicalTimeEffects` |
| Random | Cryptographically secure randomness | `RandomEffects` |
| Console | Logging and output | `ConsoleEffects` |

### Optional Effects

| Effect | Default Behavior |
|--------|-----------------|
| `TransportEffects` | TCP transport |
| `LogicalClockEffects` | Derived from storage |
| `OrderClockEffects` | Derived from random |
| `ReactiveEffects` | Default reactive handler |
| `JournalEffects` | Derived from storage + crypto |
| `BiometricEffects` | Fallback no-op handler |

### Platform Implementation Checklist

- [ ] Identify platform-specific APIs for crypto, storage, time, random, console
- [ ] Implement the five core effect traits
- [ ] Create a preset builder (optional)
- [ ] Add feature flags for platform-specific dependencies
- [ ] Write integration tests using mock handlers
- [ ] Document platform-specific security considerations
- [ ] Consider transport requirements (WebSocket, BLE, etc.)

## 4. Testing Handlers

Test handlers using mock implementations from `aura-testkit`.

```rust
use aura_testkit::*;

#[aura_test]
async fn test_my_handler() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;

    // Use fixture.effects() to get mock effect system
    let result = my_operation(&fixture.effects()).await?;

    assert!(result.is_valid());
    Ok(())
}
```

**Key principles**:
- Never use real system calls in tests (no `SystemTime::now()`, `thread_rng()`, etc.)
- Use deterministic seeds for reproducibility
- Test both success and error paths

See [Testing Guide](804_testing_guide.md) for comprehensive testing patterns.

## 5. Effect System Architecture

For deeper understanding of the effect system architecture, see:

- [Effect System](103_effect_system.md) - Full specification
- [Cryptographic Architecture](100_crypto.md) - Crypto handler requirements
- [System Architecture](001_system_architecture.md) - Layer boundaries

### Key Concepts

The effect system uses three layers:

1. **Foundation effects** (`aura-core`): Crypto, storage, time, random, console, transport
2. **Infrastructure effects** (`aura-effects`): Production handlers implementing foundation traits
3. **Composite effects**: Built by composing foundation effects (e.g., `TreeEffects` = storage + crypto)

All impure operations (time, randomness, filesystem, network) must flow through effect traits. Direct calls break simulation determinism and WASM compatibility.

Run `just check-arch` to validate effect trait placement and layer boundaries.
