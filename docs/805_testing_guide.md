# Testing Guide

This guide covers Aura's testing infrastructure built on the stateless effect system architecture. Testing validates protocol correctness through property-based testing, integration testing, and provides async testing support with the `#[aura_test]` macro.

## Core Testing Philosophy

Aura's testing approach is built on four key principles:

1. **Async-Native Testing** - The `#[aura_test]` macro provides automatic tracing setup and timeout handling
2. **Effect System Compliance** - Tests MUST use [effect traits](106_effect_system_and_runtime.md), never direct impure function access
3. **Protocol Fidelity** - Tests run actual protocol logic through real effect implementations
4. **Deterministic Execution** - Controlled effects enable reproducible test environments

**Critical**: Tests must follow the same [effect system](106_effect_system_and_runtime.md) guidelines as production code. Direct usage of `SystemTime::now()`, `thread_rng()`, `File::open()`, `Uuid::new_v4()`, or other impure functions is forbidden. All impure operations must flow through effect traits to ensure deterministic simulation and WASM compatibility.

This approach eliminates boilerplate while providing testing capabilities through automatic tracing, timeout protection, and reusable test fixtures.

## The #[aura_test] Macro

The `#[aura_test]` macro is a lightweight wrapper around `#[tokio::test]` that provides:
- Automatic tracing initialization for test output
- Default 30-second timeout protection
- Proper async test setup

### Basic Usage

```rust
use aura_macros::aura_test;
use aura_testkit::*;

#[aura_test]
async fn test_basic_protocol() -> aura_core::AuraResult<()> {
    // Tracing automatically initialized
    // 30s timeout automatically applied

    let fixture = create_test_fixture().await?;

    // Test logic here
    assert!(true);

    Ok(())
}
```

### What #[aura_test] Provides

```rust
// This macro transforms:
#[aura_test]
async fn my_test() -> aura_core::AuraResult<()> {
    // test body
}

// Into approximately:
#[tokio::test]
async fn my_test() -> aura_core::AuraResult<()> {
    let _guard = aura_testkit::init_test_tracing();

    tokio::time::timeout(
        std::time::Duration::from_secs(30),
        async move {
            // test body
        }
    ).await.expect("Test timed out after 30 seconds")
}
```

**Note**: Unlike some testing frameworks, `#[aura_test]` does NOT provide:
- Automatic effect system initialization (you create this explicitly)
- Test context injection (you create fixtures explicitly)
- Custom timeout configuration (always 30s)
- Time control functions (not currently implemented)

## Test Fixtures

The `TestFixture` type provides a reusable test environment with consistent configuration.

### Creating Test Fixtures

```rust
use aura_macros::aura_test;
use aura_testkit::*;

#[aura_test]
async fn test_with_fixture() -> aura_core::AuraResult<()> {
    // Create default fixture
    let fixture = create_test_fixture().await?;

    // Get device IDs for testing
    let device_id = fixture.device_id();
    let another_device = fixture.create_device_id();

    // Access the test context
    let context = fixture.context();

    Ok(())
}
```

### Custom Fixture Configuration

```rust
use aura_testkit::infrastructure::harness::{TestFixture, TestConfig};

#[aura_test]
async fn test_with_custom_config() -> aura_core::AuraResult<()> {
    let config = TestConfig {
        name: "custom_test".to_string(),
        deterministic_time: true,
        capture_effects: false,
        timeout: Some(std::time::Duration::from_secs(60)),
    };

    let fixture = TestFixture::with_config(config).await?;

    // Use fixture
    Ok(())
}
```

### Deterministic Identifier Generation

Tests must use deterministic methods for creating identifiers like `AuthorityId`, `ContextId`, and `DeviceId`. Never use methods that consume system entropy.
Production code should generate identifiers via `RandomEffects` (e.g., `aura-effects::identifiers::new_authority_id`) rather than direct entropy access.

```rust
use aura_core::identifiers::{AuthorityId, ContextId};
use uuid::Uuid;

// ✅ CORRECT: Deterministic identifiers for tests
let auth_id = AuthorityId::new_from_entropy([1u8; 32]);  // Deterministic bytes
let ctx_id = ContextId::from_uuid(Uuid::nil());          // Placeholder
let ctx_id = ContextId::from_uuid(Uuid::from_bytes([2u8; 16]));  // Unique but deterministic

// ❌ FORBIDDEN: Non-deterministic identifiers
// let auth_id = AuthorityId::from_uuid(Uuid::new_v4());  // Uses system entropy
// let ctx_id = ContextId::from_uuid(Uuid::new_v4());     // Uses system entropy
```

**Why deterministic IDs matter**:
- **Reproducible tests**: Same inputs produce same outputs every run
- **Debuggability**: Failures can be reproduced exactly
- **CI reliability**: No flaky tests from random ID collisions

When tests need multiple distinct identifiers, use incrementing byte patterns:
```rust
let auth1 = AuthorityId::new_from_entropy([1u8; 32]);
let auth2 = AuthorityId::new_from_entropy([2u8; 32]);
let auth3 = AuthorityId::new_from_entropy([3u8; 32]);
```

### Effect System Compliance in Tests

**Tests must use [effect traits](106_effect_system_and_runtime.md) for all impure operations**:

```rust
use aura_agent::runtime::AuraEffectSystem;
use aura_agent::AgentConfig;
use aura_core::effects::{TimeEffects, RandomEffects, StorageEffects};

#[aura_test]
async fn test_with_effects() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;

    // Create effect system - uses real handlers with in-memory storage
    let effects = AuraEffectSystem::testing(&AgentConfig::default());
    let ctx = fixture.context();

    // ✅ CORRECT: Use effect traits
    let timestamp = effects.current_time().await;
    let nonce = effects.random_bytes(32).await?;
    let data = effects.read_chunk(&chunk_id).await?;

    // ❌ FORBIDDEN in tests (just like production code):
    // let now = SystemTime::now();
    // let random = thread_rng().gen::<u64>();
    // let file = File::open("test_data.txt")?;

    Ok(())
}
```

**Why effect compliance matters in tests**:
- **Deterministic execution**: Tests produce consistent results
- **WASM compatibility**: Test code can run in browsers
- **Simulation fidelity**: Same constraints as production code

## Integration Testing

Integration testing validates complete system behavior across multiple protocol layers.

### End-to-End Protocol Testing

```rust
use aura_macros::aura_test;
use aura_testkit::*;
use aura_agent::runtime::AuraEffectSystem;
use aura_agent::AgentConfig;

#[aura_test]
async fn test_threshold_signing_workflow() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;

    // Create multiple devices for threshold protocol
    let device_ids: Vec<_> = (0..5).map(|_| fixture.create_device_id()).collect();

    // Create effect systems for each participant
    let effect_systems: Vec<_> = (0..5)
        .map(|_| AuraEffectSystem::testing(&AgentConfig::default()))
        .collect();

    // Phase 1: Initialize threshold ceremony
    let message = b"integration test message";
    let threshold = 3;

    // Execute protocol phases
    // (actual protocol implementation depends on your choreography)

    Ok(())
}
```

### Testing with Real Handlers

Aura uses real effect handlers in tests, not mocks:

```rust
use aura_effects::crypto::RealCryptoHandler;
use aura_effects::storage::MemoryStorageHandler;
use aura_core::effects::{CryptoEffects, StorageEffects};

#[aura_test]
async fn test_with_real_handlers() -> aura_core::AuraResult<()> {
    // Create real handlers
    let crypto = RealCryptoHandler::new();
    let storage = MemoryStorageHandler::new();

    // Use handlers directly
    let key_pair = crypto.generate_signing_key().await?;

    let data = b"test data";
    storage.store(b"key", data).await?;
    let retrieved = storage.load(b"key").await?;

    assert_eq!(retrieved.as_deref(), Some(&data[..]));

    Ok(())
}
```

## Property-Based Testing

Property-based testing validates protocol invariants across diverse input spaces using proptest.

### Basic Property Testing

```rust
use proptest::prelude::*;
use aura_macros::aura_test;

// Define property strategies
fn arbitrary_message() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 1..=1024)
}

proptest! {
    #[test]
    fn protocol_maintains_invariant(message in arbitrary_message()) {
        // Property test - runs synchronously
        assert!(message.len() > 0);
        assert!(message.len() <= 1024);
    }
}
```

### Async Property Testing

For async property tests, use tokio runtime explicitly:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn async_protocol_property(data in arbitrary_message()) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let fixture = create_test_fixture().await.unwrap();

            // Test property with async code
            let result = some_async_operation(&fixture, data).await;

            assert!(result.is_ok());
        });
    }
}
```

## Testing Best Practices

### Structure Tests with Fixtures

```rust
use aura_macros::aura_test;
use aura_testkit::*;
use aura_agent::runtime::AuraEffectSystem;
use aura_agent::AgentConfig;

#[aura_test]
async fn test_structured_protocol() -> aura_core::AuraResult<()> {
    // Setup
    let fixture = create_test_fixture().await?;
    let effects = AuraEffectSystem::testing(&AgentConfig::default());

    // Execute
    let result = execute_protocol(&effects).await?;

    // Verify
    assert!(result.is_valid());
    assert_eq!(result.participant_count(), 3);

    Ok(())
}
```

### Use Test Builders for Complex Setup

```rust
use aura_testkit::builders::*;

#[aura_test]
async fn test_with_builder() -> aura_core::AuraResult<()> {
    // Use builder pattern for complex test setup
    let account = test_account_with_seed(42).await;
    let key_pair = test_key_pair(1337);

    // Build test state
    let fixture = create_test_fixture().await?;

    Ok(())
}
```

### Testing Error Conditions

```rust
#[aura_test]
async fn test_error_handling() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;

    // Test expected failures
    let result = invalid_operation(&fixture).await;

    assert!(result.is_err());

    match result {
        Err(aura_core::AuraError::InvalidInput { .. }) => {
            // Expected error type
        }
        _ => panic!("Expected InvalidInput error"),
    }

    Ok(())
}
```

## Module Organization

Organize tests by functionality:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use aura_macros::aura_test;
    use aura_testkit::*;

    mod unit {
        use super::*;

        #[aura_test]
        async fn test_single_function() -> aura_core::AuraResult<()> {
            // Unit test
            Ok(())
        }
    }

    mod integration {
        use super::*;

        #[aura_test]
        async fn test_full_workflow() -> aura_core::AuraResult<()> {
            // Integration test
            Ok(())
        }
    }

    mod properties {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn invariant_holds(input in any::<u64>()) {
                // Property test
                assert!(input == input);
            }
        }
    }
}
```

## Available Test Utilities

The `aura-testkit` crate provides several utilities:

### Foundation Utilities

```rust
use aura_testkit::foundation::*;

// Create mock test context
let context = create_mock_test_context()?;

// Get device ID from context
let device_id = context.device_id();
```

### Builder Functions

```rust
use aura_testkit::builders::*;

// Create test accounts with deterministic seeds
let account = test_account_with_seed(42).await;

// Create test key pairs
let (signing_key, verifying_key) = test_key_pair(1337);
```

### Verification Utilities

```rust
use aura_testkit::verification::*;

// Assertion helpers for common patterns
// (specific utilities depend on your test needs)
```

## Testing Sync/Async Code (GuardSnapshot Pattern)

Aura's guard chain uses a three-phase pattern that separates sync evaluation from async execution. This is important for testing pure guard logic independently from effect execution.

### The GuardSnapshot Pattern

Guard evaluation is **pure and synchronous** over a prepared snapshot. The async interpreter then executes the resulting commands:

```rust
use aura_macros::aura_test;
use aura_testkit::*;

#[aura_test]
async fn test_guard_chain_evaluation() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;
    let effects = fixture.effects();
    let ctx = fixture.context();

    // Phase 1: Async - Prepare the snapshot
    let snapshot = prepare_guard_snapshot(&ctx, &effects).await?;

    // Phase 2: Sync - Pure guard evaluation (no I/O, easily testable)
    let commands = guard_chain.evaluate(&snapshot)?;

    // Phase 3: Async - Interpret commands
    for cmd in commands {
        execute_effect_command(&effects, cmd).await?;
    }

    Ok(())
}
```

### Testing Pure Guard Logic

Because guard evaluation is synchronous and pure, you can unit test it without async runtime:

```rust
#[test]
fn test_cap_guard_denies_unauthorized() {
    // Create snapshot with no capabilities
    let snapshot = GuardSnapshot {
        capabilities: vec![],
        flow_budget: FlowBudget { limit: 100, spent: 0, epoch: 0 },
        ..Default::default()
    };

    // Evaluate guard synchronously - no async needed
    let result = CapGuard::evaluate(&snapshot, &SendRequest::default());

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), GuardError::Unauthorized));
}

#[test]
fn test_flow_guard_blocks_over_budget() {
    let snapshot = GuardSnapshot {
        flow_budget: FlowBudget { limit: 100, spent: 95, epoch: 0 },
        ..Default::default()
    };

    // Request that would exceed budget
    let request = SendRequest { cost: 10, ..Default::default() };
    let result = FlowGuard::evaluate(&snapshot, &request);

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), GuardError::BudgetExceeded));
}
```

### When to Use Each Phase

- **Snapshot preparation**: Async - gathers current state from effects
- **Guard evaluation**: Sync - pure business logic, easily testable without mocks
- **Command interpretation**: Async - actual side effects (charging, journaling, sending)

This separation ensures that authorization logic remains testable without complex async test harnesses.

**File reference:** `docs/001_system_architecture.md` (Sections 2.1, 3.5)

## Limitations and Future Work

### Current Limitations

1. **No Time Control**: The guide previously documented `freeze_time()` and `advance_time_by()` functions, but these are not currently implemented. Use actual async delays for time-dependent tests.

2. **No Automatic Context Injection**: Unlike some frameworks, `#[aura_test]` doesn't inject a `ctx` parameter. You must create fixtures explicitly.

3. **No Performance Monitoring**: Built-in performance monitoring (`PerformanceMonitor`, `AllocationTracker`) is not currently available. Use external profiling tools.

4. **No Network Simulation in Testkit**: For network simulation, use the `aura-simulator` crate (see [Simulation Guide](806_simulation_guide.md)).

### Recommended Patterns

For features not in testkit, use these patterns:

**Time-dependent tests** - Use effect traits for time operations:
```rust
use aura_core::effects::TimeEffects;

#[aura_test]
async fn test_with_delay() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;
    let effects = AuraEffectSystem::testing(&AgentConfig::default());
    
    // ✅ CORRECT: Use TimeEffects
    let start = effects.current_time().await;
    
    // For delays in tests, use tokio::time::sleep (acceptable in tests)
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    
    let end = effects.current_time().await;
    let elapsed = end.duration_since(start)?;
    assert!(elapsed >= std::time::Duration::from_millis(100));

    Ok(())
}
```

**Note**: `tokio::time::sleep` is acceptable in test code for coordination, but time measurement must use `TimeEffects` for consistency with production patterns.

**Performance testing** - Use criterion for benchmarks:
```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_operation(c: &mut Criterion) {
    c.bench_function("operation", |b| {
        b.iter(|| {
            black_box(expensive_operation())
        });
    });
}

criterion_group!(benches, benchmark_operation);
criterion_main!(benches);
```

## TUI/CLI Deterministic Testing

The TUI and CLI are tested using a deterministic state machine approach that enables fast, reliable testing without PTY automation.

### Architecture

The TUI is modeled as a pure state machine:
```
TuiState × TerminalEvent → (TuiState, Vec<TuiCommand>)
```

This enables:
- **Deterministic tests**: Same inputs always produce same outputs
- **Fast execution**: ~1ms per test (vs seconds for PTY tests)
- **Quint verification**: Formal model checking of TUI invariants
- **Generative testing**: Automated state space exploration

### Test Types

**1. State Machine Unit Tests** (`tests/unit_state_machine.rs`):
```rust
mod support;
use support::TestTui;
use aura_terminal::tui::screens::Screen;

#[test]
fn test_screen_navigation() {
    let mut tui = TestTui::new();

    tui.assert_screen(Screen::Block);
    tui.send_char('2');  // Navigate to Neighborhood
    tui.assert_screen(Screen::Neighborhood);
}

#[test]
fn test_insert_mode() {
    let mut tui = TestTui::new();

    tui.send_char('i');  // Enter insert mode
    tui.assert_insert_mode();

    tui.send_escape();   // Exit insert mode
    tui.assert_normal_mode();
}
```

**2. Property-Based Tests** (proptest):
```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_escape_exits_insert_mode(screen in 0..7u8) {
        let mut tui = TestTui::new();
        tui.send_event(char((b'1' + screen) as char));
        tui.send_event(char('i'));
        tui.send_event(escape());
        tui.assert_normal_mode();
    }

    #[test]
    fn prop_transitions_are_deterministic(events in prop::collection::vec(any_event(), 0..50)) {
        let mut tui1 = TestTui::new();
        let mut tui2 = TestTui::new();

        for event in &events {
            tui1.send_event(event.clone());
            tui2.send_event(event.clone());
        }

        assert_eq!(tui1.state(), tui2.state());
    }
}
```

**3. ITF Trace Replay** (`tests/verification_itf_replay.rs`):
```rust
use aura_terminal::testing::itf_replay::ITFTraceReplayer;

#[test]
fn test_replay_quint_trace() {
    let replayer = ITFTraceReplayer::new();
    let result = replayer
        .replay_trace_file("verification/quint/tui_trace.itf.json")
        .expect("Failed to replay trace");

    assert!(result.all_states_match);
}
```

**4. Generative Testing** (Quint-generated traces):
```rust
#[test]
#[ignore] // Run with: cargo test --ignored
fn test_generative_replay() {
    // Generate 100 samples × 50 steps from Quint model
    // quint run --max-samples=100 --max-steps=50 --out-itf=trace.json

    let replayer = ITFTraceReplayer::new();
    let result = replayer.replay_trace_file("trace.json").unwrap();
    assert!(result.all_states_match);
}
```

### Test Organization

Tests in `aura-terminal` are organized by category with consistent naming prefixes:

| Prefix | Category | Description |
|--------|----------|-------------|
| `unit_*` | Unit | Pure state machine / deterministic tests |
| `integration_*` | Integration | Tests with AppCore/IoContext |
| `e2e_*` | E2E | PTY-based end-to-end tests (deprecated) |
| `demo_*` | Demo | Demo flow tests |
| `verification_*` | Verification | ITF replay, generative, Quint-backed tests |

### Support Module (`tests/support/`)

The `tests/support/` module provides reusable test infrastructure:

```rust
mod support;
use support::{TestTui, SimpleTestEnv, wait_for_chat};
```

**Available modules:**

- **`support::state_machine`** - `TestTui` wrapper for pure state machine testing
- **`support::env`** - Test environment setup (`SimpleTestEnv`, `FullTestEnv`)
- **`support::signals`** - Signal waiting helpers (`wait_for_chat`, `wait_for_contacts`, etc.)
- **`support::demo`** - Demo-specific helpers (invite code generation, agent IDs)

**TestTui Usage:**
```rust
mod support;
use support::TestTui;
use aura_terminal::tui::screens::Screen;

#[test]
fn test_navigation() {
    let mut tui = TestTui::new();

    tui.assert_screen(Screen::Block);
    tui.send_char('3');
    tui.assert_screen(Screen::Chat);

    // Access state for assertions
    assert_eq!(tui.state().chat.focus, ChatFocus::Channels);

    // Mutate state for test setup
    tui.state_mut().chat.channel_count = 10;
}
```

**Signal Waiting:**
```rust
mod support;
use support::{wait_for_chat, wait_for_contacts, DEFAULT_TIMEOUT};

#[tokio::test]
async fn test_async_flow() {
    let env = SimpleTestEnv::new("test").await;

    // Wait for chat state to satisfy a predicate
    let chat = wait_for_chat(&env.app_core, |s| !s.messages.is_empty()).await;
    assert!(!chat.messages.is_empty());
}
```

### Running TUI Tests

```bash
# Fast deterministic tests (recommended)
cargo test --package aura-terminal --test unit_state_machine

# ITF trace replay tests
cargo test --package aura-terminal --features testing --test verification_itf_replay

# Generative tests (slower, more thorough)
cargo test --package aura-terminal --features testing --test verification_itf_replay -- --ignored

# Legacy PTY tests (deprecated, may be flaky)
cargo test --package aura-terminal --test e2e_legacy_pty

# Run all unit tests
cargo test --package aura-terminal --test 'unit_*'

# Run all integration tests
cargo test --package aura-terminal --test 'integration_*'
```

### Quint Model Verification

The TUI state machine has a formal Quint specification at `verification/quint/tui_state_machine.qnt` that:
- Defines screens, modals, and state transitions
- Specifies invariants (e.g., insert mode only on valid screens)
- Enables model checking via Apalache

```bash
# Run Quint tests
quint test verification/quint/tui_state_machine.qnt

# Verify invariants with Apalache
quint verify --max-steps=5 --invariant=allInvariants verification/quint/tui_state_machine.qnt

# Generate (or check) the deterministic ITF trace used by replay tests
just tui-itf-trace
just tui-itf-trace-check
```

### CLI Test Harness

The CLI has a deterministic test harness in `src/testing/cli.rs`:

```rust
use aura_terminal::testing::cli::CliTestHarness;

#[tokio::test]
async fn test_cli_version() {
    let harness = CliTestHarness::new().await;
    harness.exec_version();
    harness.assert_stdout_contains("aura");
}
```

### CLI Thin Shell Pattern with CliOutput

CLI handlers use a "thin shell" pattern where business logic returns structured `CliOutput` instead of printing directly. This enables unit testing without stdout capture.

**Architecture**:
```
CLI Args → Handler (returns CliOutput) → render() → stdout/stderr
```

**Handler Pattern**:
```rust
use crate::handlers::{CliOutput, HandlerContext};
use anyhow::Result;

/// Handler returns structured output, not Result<()>
pub async fn handle_status(ctx: &HandlerContext<'_>) -> Result<CliOutput> {
    let mut output = CliOutput::new();

    // Build structured output
    output.section("Account Status");
    output.kv("Authority", ctx.effect_context().authority_id().to_string());
    output.kv("Device", ctx.device_id().to_string());

    // Error messages go to stderr
    if some_error_condition {
        output.eprintln("Warning: configuration issue detected");
    }

    Ok(output)
}
```

**CliOutput API**:
```rust
let mut output = CliOutput::new();

// Stdout methods
output.println("Normal message");           // Single line
output.section("Title");                    // "=== Title ==="
output.kv("Key", "Value");                  // "Key: Value"
output.blank();                             // Empty line
output.table(&["Col1", "Col2"], &rows);     // Formatted table

// Stderr method
output.eprintln("Error message");           // Goes to stderr

// Rendering (called by CliHandler wrapper)
output.render();                            // Prints to actual stdout/stderr
```

**Testing Handlers**:
```rust
use aura_terminal::handlers::{CliOutput, HandlerContext};

#[tokio::test]
async fn test_status_handler() {
    // Setup mock context
    let ctx = create_test_handler_context().await;

    // Call handler - returns structured output, no stdout pollution
    let output = status::handle_status(&ctx).await.unwrap();

    // Assert on structured output
    let stdout = output.stdout_lines();
    assert!(stdout.iter().any(|line| line.contains("Authority")));
    assert!(output.stderr_lines().is_empty());
}

#[test]
fn test_cli_output_formatting() {
    let mut output = CliOutput::new();
    output.section("Test");
    output.kv("Name", "Alice");

    let lines = output.stdout_lines();
    assert_eq!(lines[0], "=== Test ===");
    assert_eq!(lines[1], "Name: Alice");
}
```

**Benefits**:
- **Testable**: Assert on structured output without capturing stdout
- **Deterministic**: Same inputs produce same CliOutput
- **Separated concerns**: Logic produces data, render() handles I/O
- **Consistent formatting**: Shared methods ensure uniform output style

### Best Practices

1. **Prefer deterministic tests** over PTY tests for all TUI logic
2. **Use property tests** to verify invariants hold across inputs
3. **Run generative tests** periodically to find edge cases
4. **Keep Quint model in sync** with Rust implementation
5. **Add new transitions to both** Quint spec and Rust tests

## Summary

Aura's testing infrastructure provides:

- **`#[aura_test]` Macro** - Automatic tracing and timeout for async tests
- **TestFixture** - Reusable test environment with consistent setup
- **Real Effect Handlers** - Tests use actual implementations, not mocks
- **Property Testing** - Validate invariants with proptest
- **Integration Testing** - End-to-end protocol validation
- **TUI State Machine Testing** - Deterministic tests with Quint verification

The testing approach emphasizes simplicity and fidelity to production code. Tests use the same stateless effect handlers as production, ensuring high confidence in test results.

For simulation capabilities that enable fault injection and network modeling, see [Simulation Guide](806_simulation_guide.md). Learn about the effect system in [Effect System Guide](106_effect_system_and_runtime.md).
