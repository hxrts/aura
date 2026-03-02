# Testing Guide

This guide covers how to write tests for Aura protocols using the testing infrastructure. It focuses on practical patterns and workflows.

## Core Philosophy

Aura tests follow four principles. Tests use effect traits, never direct impure functions. Tests run actual protocol logic through real handlers. Tests produce deterministic results. Tests validate both happy paths and error conditions.

All test code must follow the same effect system guidelines as production code. Direct usage of `SystemTime::now()`, `thread_rng()`, `File::open()`, or `Uuid::new_v4()` is forbidden. These operations must flow through effect traits.

See [Test Infrastructure Reference](117_testkit.md) for the complete infrastructure documentation.

## Using the aura_test Macro

The `#[aura_test]` macro provides async test setup with tracing and timeout.

```rust
use aura_macros::aura_test;
use aura_testkit::*;

#[aura_test]
async fn test_basic_operation() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;
    let result = some_operation(&fixture).await?;
    assert!(result.is_valid());
    Ok(())
}
```

The macro wraps the test body with tracing initialization and a 30-second timeout. It does not provide automatic effect system initialization or context injection. You must create fixtures explicitly.

## Test Fixtures

Fixtures provide consistent test environments with deterministic configuration.

### Creating Fixtures

```rust
use aura_testkit::infrastructure::harness::TestFixture;

let fixture = TestFixture::new().await?;
let device_id = fixture.device_id();
let context = fixture.context();
```

The default fixture creates deterministic identifiers and initializes effect handlers with in-memory storage.

### Custom Configuration

```rust
use aura_testkit::infrastructure::harness::{TestFixture, TestConfig};

let config = TestConfig {
    name: "threshold_test".to_string(),
    deterministic_time: true,
    capture_effects: true,
    timeout: Some(Duration::from_secs(60)),
};
let fixture = TestFixture::with_config(config).await?;
```

Custom configuration enables time control and effect capture for inspection.

### Deterministic Identifiers

Tests must use deterministic identifier generation.

```rust
use aura_core::identifiers::AuthorityId;

let auth1 = AuthorityId::from_entropy([1u8; 32]);
let auth2 = AuthorityId::from_entropy([2u8; 32]);
```

Incrementing byte patterns create distinct but reproducible identifiers. Never use `Uuid::new_v4()` or other entropy-consuming methods.

## Writing Unit Tests

Unit tests validate individual functions or components.

```rust
#[aura_test]
async fn test_single_function() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;
    let input = TestInput::new(42);
    let output = process_input(&fixture, input).await?;
    assert_eq!(output.value, 84);
    Ok(())
}
```

Unit tests should be fast and focused. They test one behavior per test function.

## Writing Integration Tests

Integration tests validate complete workflows across components.

```rust
use aura_agent::runtime::AuraEffectSystem;
use aura_agent::AgentConfig;

#[aura_test]
async fn test_threshold_workflow() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;
    let device_ids: Vec<_> = (0..5)
        .map(|i| DeviceId::new_from_entropy([i as u8 + 1; 32]))
        .collect();
    let effect_systems: Vec<_> = (0..5)
        .map(|_| AuraEffectSystem::testing(&AgentConfig::default()))
        .collect();

    // Execute multi-phase protocol
    let result = execute_protocol(&effect_systems, &device_ids).await?;
    assert!(result.is_complete());
    Ok(())
}
```

Integration tests exercise real handlers and complete protocol flows.

## Property-Based Testing

Property tests validate invariants across diverse inputs using proptest.

### Synchronous Properties

```rust
use proptest::prelude::*;

fn arbitrary_message() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 1..=1024)
}

proptest! {
    #[test]
    fn message_roundtrip(message in arbitrary_message()) {
        let encoded = encode(&message);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(message, decoded);
    }
}
```

Proptest generates inputs and shrinks failures to minimal cases.

### Async Properties

```rust
proptest! {
    #[test]
    fn async_property(data in arbitrary_message()) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let fixture = create_test_fixture().await.unwrap();
            let result = async_operation(&fixture, data).await;
            assert!(result.is_ok());
        });
    }
}
```

Async property tests require explicit runtime creation within the test body.

## Property-Monitored Simulation Tests

For distributed liveness/safety checks, combine simulator scenarios with online property monitoring.

Use:

- `AuraPropertyMonitor` for tick-by-tick checks
- `PropertyMonitoringConfig` in `SimulationScenarioConfig`
- `default_property_suite` for protocol-class defaults

```rust
let monitoring = PropertyMonitoringConfig::new(
    ProtocolPropertyClass::Sync,
    ProtocolPropertySuiteIds { session, context },
)
.with_snapshot_provider(|tick| snapshot_for_tick(tick));

let config = SimulationScenarioConfig {
    property_monitoring: Some(monitoring),
    ..SimulationScenarioConfig::default()
};

let results = env.run_scenario("sync".into(), "sync with monitors".into(), config).await?;
assert!(results.property_violations.is_empty());
```

If a property fails, `results.property_violations` contains tick-local diagnostics suitable for CI artifacts.

## GuardSnapshot Pattern

The guard chain separates pure evaluation from async execution. This enables testing authorization logic without async runtime.

### Testing Pure Guard Logic

```rust
#[test]
fn test_cap_guard_denies_unauthorized() {
    let snapshot = GuardSnapshot {
        capabilities: vec![],
        flow_budget: FlowBudget { limit: 100, spent: 0, epoch: 0 },
        ..Default::default()
    };
    let result = CapGuard::evaluate(&snapshot, &SendRequest::default());
    assert!(result.is_err());
}
```

Guard evaluation is synchronous and pure. No effect handlers or async runtime needed.

### Testing Flow Budget

```rust
#[test]
fn test_flow_guard_blocks_over_budget() {
    let snapshot = GuardSnapshot {
        flow_budget: FlowBudget { limit: 100, spent: 95, epoch: 0 },
        ..Default::default()
    };
    let request = SendRequest { cost: 10, ..Default::default() };
    let result = FlowGuard::evaluate(&snapshot, &request);
    assert!(matches!(result.unwrap_err(), GuardError::BudgetExceeded));
}
```

The snapshot contains all state needed for guard evaluation. Tests inject specific states to verify edge cases.

## TUI Testing

The TUI uses a deterministic state machine approach. See [Test Infrastructure Reference](117_testkit.md) for `MockRuntimeBridge` details.

### State Machine Tests

```rust
mod support;
use support::TestTui;
use aura_terminal::tui::screens::Screen;

#[test]
fn test_screen_navigation() {
    let mut tui = TestTui::new();
    tui.assert_screen(Screen::Block);
    tui.send_char('2');
    tui.assert_screen(Screen::Neighborhood);
}
```

The `TestTui` wrapper provides methods for sending events and asserting state.

### Property-Based TUI Tests

```rust
proptest! {
    #[test]
    fn escape_exits_insert_mode(screen in 0..7u8) {
        let mut tui = TestTui::new();
        tui.send_char((b'1' + screen) as char);
        tui.send_char('i');
        tui.send_escape();
        tui.assert_normal_mode();
    }
}
```

Property tests verify TUI invariants across input combinations.

### ITF Trace Replay

```rust
use aura_terminal::testing::itf_replay::ITFTraceReplayer;

#[test]
fn test_replay_quint_trace() {
    let replayer = ITFTraceReplayer::new();
    let result = replayer
        .replay_trace_file("verification/quint/tui_trace.itf.json")
        .expect("Replay failed");
    assert!(result.all_states_match);
}
```

ITF traces from Quint model checking validate TUI behavior against the formal specification.

## CLI Testing

CLI handlers use the thin shell pattern with `CliOutput`.

### Handler Testing

```rust
use aura_terminal::handlers::{CliOutput, HandlerContext};

#[tokio::test]
async fn test_status_handler() {
    let ctx = create_test_handler_context().await;
    let output = status::handle_status(&ctx).await.unwrap();
    let lines = output.stdout_lines();
    assert!(lines.iter().any(|l| l.contains("Authority")));
}
```

Handlers return structured `CliOutput` instead of printing directly. Tests assert on the structured output.

### CliOutput Pattern

```rust
let mut output = CliOutput::new();
output.section("Status");
output.kv("Authority", auth_id.to_string());
output.kv("Device", device_id.to_string());
```

The `CliOutput` type collects output lines for later rendering. This separates logic from I/O.

## Conformance Testing

Conformance tests validate that protocol implementations produce identical results across execution environments. The primary focus is native/WASM parity. All protocol transitions must be deterministic given the same inputs.

### Conformance Lanes

CI runs two conformance lanes with different comparison strategies.

The strict lane compares native cooperative and WASM cooperative execution:

```bash
just ci-conformance-strict
```

This lane uses the `native_coop` and `wasm_coop` executors. Both use cooperative scheduling. Differences indicate platform-specific behavior.

The differential lane compares native threaded and native cooperative execution:

```bash
just ci-conformance-diff
```

This lane detects scheduling-dependent behavior. It also runs model-level differential test suites.

Run both lanes together:

```bash
just ci-conformance
```

Any undeclared divergence blocks merge.

### Corpus Policy

The conformance corpus defines test inputs for parity checking.

Fixed seeds provide stable regression coverage:

```bash
AURA_CONFORMANCE_SEED=42 cargo test conformance
```

Rotating seeds provide broader coverage over time:

```bash
AURA_CONFORMANCE_ROTATING_WINDOW=100 cargo test conformance
```

CI uses a window of recent seeds that advances with each run.

Seeds can derive from Quint ITF traces:

```bash
AURA_CONFORMANCE_ITF_TRACE=trace.itf.json cargo test conformance
```

This couples conformance coverage to formal model coverage.

### Reproducing Failures

Native reproduction:

```bash
AURA_CONFORMANCE_SCENARIO=scenario_name \
AURA_CONFORMANCE_SEED=42 \
cargo test -p aura-agent \
  --features choreo-backend-telltale-vm \
  --test telltale_vm_parity test_name \
  -- --nocapture
```

WASM reproduction:

```bash
AURA_CONFORMANCE_SCENARIO=scenario_name \
AURA_CONFORMANCE_SEED=42 \
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
cargo test -p aura-agent \
  --target wasm32-unknown-unknown \
  --features web,choreo-backend-telltale-vm \
  --test telltale_vm_parity test_name \
  -- --nocapture
```

### Replay Debugging

Parity lanes emit trace artifacts under `artifacts/choreo-parity` (CI uploads them as workflow artifacts). Verify one artifact locally with:

```bash
aura replay --trace-file artifacts/choreo-parity/native_replay/<scenario>__seed_<seed>.json
```

For automated lane comparison in simulator/test workflows, use `aura-simulator::DifferentialTester` with `strict` or `envelope_bounded` profiles.

### Troubleshooting Conformance

**Unclassified envelope kind**: Add the kind to `AURA_EFFECT_ENVELOPE_CLASSIFICATIONS` with appropriate class.

**Missing required surface**: Ensure runtime captures all three surfaces (observable, scheduler_step, effect) before computing digest.

**Native/Threaded mismatch**: Remove ordering-sensitive hidden state. Normalize only declared commutative or algebraic envelopes.

**WASM runner schema mismatch**: Align `wasm-bindgen-test-runner` version with workspace `wasm-bindgen` version.

## Test Organization

Organize tests by category with consistent naming.

```rust
#[cfg(test)]
mod tests {
    mod unit {
        #[aura_test]
        async fn test_single_function() -> aura_core::AuraResult<()> {
            Ok(())
        }
    }

    mod integration {
        #[aura_test]
        async fn test_full_workflow() -> aura_core::AuraResult<()> {
            Ok(())
        }
    }

    mod properties {
        proptest! {
            #[test]
            fn invariant_holds(input in any::<u64>()) {
                assert!(input == input);
            }
        }
    }
}
```

The `aura-terminal` crate uses naming prefixes for test files. Unit tests use `unit_*`. Integration tests use `integration_*`. Verification tests use `verification_*`.

## Running Tests

```bash
# All tests
just test

# Specific crate
just test-crate aura-agent

# With output
cargo test --workspace -- --nocapture

# TUI state machine tests
cargo test --package aura-terminal --test unit_state_machine

# ITF replay tests
cargo test --package aura-terminal --features testing --test verification_itf_replay
```

## Best Practices

Test one behavior per function. Name tests descriptively. Use fixtures for common setup. Prefer real handlers over mocks. Test error conditions explicitly.

Avoid testing implementation details. Focus on observable behavior. Keep tests fast. Parallelize independent tests.

## Related Documentation

See [Test Infrastructure Reference](117_testkit.md) for infrastructure details. See [Simulation Guide](806_simulation_guide.md) for fault injection testing.
