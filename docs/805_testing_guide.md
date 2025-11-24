# Testing Guide

This guide covers Aura's testing infrastructure built on the stateless effect system architecture. Testing validates protocol correctness through property-based testing, integration testing, and provides async testing support with the `#[aura_test]` macro.

## Core Testing Philosophy

Aura's testing approach is built on four key principles:

1. **Async-Native Testing** - The `#[aura_test]` macro provides automatic tracing setup and timeout handling
2. **Effect System Compliance** - Tests MUST use [effect traits](106_effect_system_and_runtime.md), never direct impure function access
3. **Protocol Fidelity** - Tests run actual protocol logic through real effect implementations
4. **Deterministic Execution** - Controlled effects enable reproducible test environments

**Critical**: Tests must follow the same [effect system](106_effect_system_and_runtime.md) guidelines as production code. Direct usage of `SystemTime::now()`, `thread_rng()`, `File::open()`, or other impure functions is forbidden. All impure operations must flow through effect traits to ensure deterministic simulation and WASM compatibility.

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

## Summary

Aura's testing infrastructure provides:

- **`#[aura_test]` Macro** - Automatic tracing and timeout for async tests
- **TestFixture** - Reusable test environment with consistent setup
- **Real Effect Handlers** - Tests use actual implementations, not mocks
- **Property Testing** - Validate invariants with proptest
- **Integration Testing** - End-to-end protocol validation

The testing approach emphasizes simplicity and fidelity to production code. Tests use the same stateless effect handlers as production, ensuring high confidence in test results.

For simulation capabilities that enable fault injection and network modeling, see [Simulation Guide](806_simulation_guide.md). Learn about the effect system in [Effect System Guide](106_effect_system_and_runtime.md). Review the async refactor progress in [project documentation](../work/).
