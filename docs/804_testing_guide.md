# Testing Guide

This guide covers how to write tests for Aura protocols using the testing infrastructure. It includes unit testing, integration testing, property-based testing, conformance testing, and runtime harness validation.

For infrastructure details, see [Test Infrastructure Reference](117_testkit.md).

## 1. Core Philosophy

Aura tests follow four principles:
1. **Effect-based**: Tests use effect traits, never direct impure functions
2. **Real handlers**: Tests run actual protocol logic through real handlers
3. **Deterministic**: Tests produce reproducible results
4. **Comprehensive**: Tests validate both happy paths and error conditions

Direct usage of `SystemTime::now()`, `thread_rng()`, `File::open()`, or `Uuid::new_v4()` is forbidden. These operations must flow through effect traits.

## 2. The `#[aura_test]` Macro

The macro provides async test setup with tracing and timeout:

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

The macro wraps the test body with tracing initialization and a 30-second timeout. Create fixtures explicitly.

## 3. Test Fixtures

Fixtures provide consistent test environments with deterministic configuration.

### Creating Fixtures

```rust
use aura_testkit::infrastructure::harness::TestFixture;

let fixture = TestFixture::new().await?;
let device_id = fixture.device_id();
let context = fixture.context();
```

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

### Deterministic Identifiers

Use deterministic identifier generation:

```rust
use aura_core::identifiers::AuthorityId;

let auth1 = AuthorityId::from_entropy([1u8; 32]);
let auth2 = AuthorityId::from_entropy([2u8; 32]);
```

Incrementing byte patterns create distinct but reproducible identifiers.

## 4. Unit Tests

Unit tests validate individual functions or components:

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

Unit tests should be fast and focused, testing one behavior per function.

## 5. Integration Tests

Integration tests validate complete workflows:

```rust
use aura_agent::runtime::AuraEffectSystem;
use aura_agent::AgentConfig;

#[aura_test]
async fn test_threshold_workflow() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;
    let device_ids: Vec<_> = (0..5)
        .map(|i| DeviceId::new_from_entropy([i as u8 + 1; 32]))
        .collect();

    let effect_systems: Result<Vec<_>, _> = (0..5)
        .map(|_| AuraEffectSystem::testing(&AgentConfig::default()))
        .collect();

    let result = execute_protocol(&effect_systems?, &device_ids).await?;
    assert!(result.is_complete());
    Ok(())
}
```

## 6. Property-Based Testing

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

## 7. GuardSnapshot Pattern

The guard chain separates pure evaluation from async execution, enabling testing without async runtime.

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

## 8. TUI and CLI Testing

### TUI State Machine Tests

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

### CLI Handler Testing

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

## 9. Conformance Testing

Conformance tests validate that implementations produce identical results across environments.

### Conformance Lanes

CI runs two lanes:

**Strict lane** (native vs WASM cooperative):
```bash
just ci-conformance-strict
```

**Differential lane** (native threaded vs cooperative):
```bash
just ci-conformance-diff
```

Run both:
```bash
just ci-conformance
```

### Mismatch Taxonomy

| Type | Description | Fix |
|------|-------------|-----|
| `strict` | Byte-level difference | Remove hidden state or ordering-sensitive side effects |
| `envelope_bounded` | Outside declared envelopes | Add or correct envelope classification |
| `surface_missing` | Required surface not present | Emit observable, scheduler_step, and effect |

### Reproducing Failures

```bash
AURA_CONFORMANCE_SCENARIO=scenario_name \
AURA_CONFORMANCE_SEED=42 \
cargo test -p aura-agent \
  --features choreo-backend-telltale-vm \
  --test telltale_vm_parity test_name \
  -- --nocapture
```

## 10. Runtime Harness

The runtime harness executes real Aura instances in PTYs for end-to-end validation.

### Harness Overview

The harness supports:
- **Scripted mode**: Predefined steps from a scenario file
- **Agent mode**: LLM-driven execution toward goals

### Run Config

```toml
schema_version = 1

[run]
name = "local-loopback-smoke"
pty_rows = 40
pty_cols = 120
seed = 4242

[[instances]]
id = "alice"
mode = "local"
data_dir = "artifacts/harness/state/local-loopback/alice"
device_id = "alice-dev-01"
bind_address = "127.0.0.1:41001"
```

### Scenario File

```toml
schema_version = 1
id = "discovery-smoke"
execution_mode = "scripted"

[[steps]]
id = "launch"
action = "launch_instances"
timeout_ms = 5000

[[steps]]
id = "send"
action = "send_keys"
instance = "alice"
keys = "hello\n"
timeout_ms = 2000

[[steps]]
id = "wait"
action = "wait_for"
instance = "alice"
pattern = "hello"
timeout_ms = 2000
```

### Running the Harness

```bash
# Lint before running
just harness-lint -- --config configs/harness/local-loopback.toml \
  --scenario scenarios/harness/local-discovery-smoke.toml

# Execute
just harness-run -- --config configs/harness/local-loopback.toml \
  --scenario scenarios/harness/local-discovery-smoke.toml

# Replay for deterministic reproduction
just harness-replay -- --bundle artifacts/harness/local-loopback-smoke/replay_bundle.json
```

### Interactive Mode

Use `tool_repl` for manual validation:

```bash
cargo run -p aura-harness --bin tool_repl -- \
  --config configs/harness/local-loopback.toml
```

Send JSON requests:
```json
{"id":1,"method":"screen","params":{"instance_id":"alice"}}
{"id":2,"method":"send_keys","params":{"instance_id":"alice","keys":"3n"}}
{"id":3,"method":"wait_for","params":{"instance_id":"alice","pattern":"Create","timeout_ms":4000}}
```

### Harness CI

```bash
just ci-harness-build
just ci-harness-contract
just ci-harness-replay
```

## 11. Test Organization

Organize tests by category:

```rust
#[cfg(test)]
mod tests {
    mod unit {
        #[aura_test]
        async fn test_single_function() -> aura_core::AuraResult<()> { Ok(()) }
    }

    mod integration {
        #[aura_test]
        async fn test_full_workflow() -> aura_core::AuraResult<()> { Ok(()) }
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

### Running Tests

```bash
# All tests
just test

# Specific crate
just test-crate aura-agent

# With output
cargo test --workspace -- --nocapture

# TUI state machine tests
cargo test --package aura-terminal --test unit_state_machine
```

## 12. Best Practices

- Test one behavior per function
- Name tests descriptively
- Use fixtures for common setup
- Prefer real handlers over mocks
- Test error conditions explicitly
- Avoid testing implementation details
- Focus on observable behavior
- Keep tests fast
- Parallelize independent tests

## 13. Holepunch Backends and Artifact Triage

Use the harness `--network-backend` option to select execution mode:

```bash
# Deterministic local backend
cargo run -p aura-harness --bin aura-harness -- \
  run --config configs/harness/local-loopback.toml \
  --network-backend mock

# Native Linux Patchbay (requires Linux + userns/capabilities)
cargo run -p aura-harness --bin aura-harness -- \
  run --config configs/harness/local-loopback.toml \
  --network-backend patchbay

# Cross-platform VM runner (macOS/Linux)
cargo run -p aura-harness --bin aura-harness -- \
  run --config configs/harness/local-loopback.toml \
  --network-backend patchbay-vm
```

Harness writes backend resolution details to:

```text
artifacts/harness/<run>/network_backend_preflight.json
```

Patchbay is the authoritative NAT-realism backend for holepunch validation:

- Use native `patchbay` on Linux CI and Linux developer machines when capabilities are available.
- Use `patchbay-vm` on macOS (and as Linux fallback) to run the same scenarios in a Linux VM.
- Keep deterministic non-network logic in `mock` backend tests to preserve fast feedback.

Recommended implementation tiers:

1. Tier 1: deterministic/property tests in `aura-testkit` for retry/path-selection invariants.
2. Tier 2: Patchbay integration scenarios in `aura-harness` for PR gating.
3. Tier 3: Patchbay stress/flake detection suites on scheduled CI.

When a scenario fails, triage in this order:

1. `network_backend_preflight.json` to confirm selected backend and fallback reason.
2. `startup_summary.json` and `scenario_report.json` for run context and failing step.
3. `events.json` and backend timeline artifacts (`timeline.json` / `event_timeline.json`) for event ordering.
4. Namespace/network dumps (`ip-*`, `nft*`) and `*.pcap` files for packet/routing diagnosis.
5. Agent logs for authority-local failures and retry state transitions.

## 14. Browser Harness Workflow (WASM + Playwright)

Use this flow to run harness scenarios in browser mode:

```bash
# 1) Check wasm/frontend compilation
just web-check

# 2) Install/update Playwright driver deps
cd crates/aura-harness/playwright-driver
npm ci
npm run install-browsers
npm test
cd ../..

# 3) Serve the web app
just web-serve
```

In a second shell:

```bash
# Lint browser run/scenario config
just harness-lint-browser scenarios/harness/local-discovery-smoke.toml

# Run browser scenarios
just harness-run-browser scenarios/harness/local-discovery-smoke.toml
just harness-run-browser scenarios/harness/scenario1-invitation-chat-e2e.toml
just harness-run-browser scenarios/harness/home-roles.toml

# Replay the latest browser run bundle
just harness-replay-browser
```

Browser harness artifacts are written under:

```text
artifacts/harness/browser/
```

Key files when debugging browser failures:

1. `web-serve.log` for bundle/build/runtime startup issues.
2. `preflight_report.json` for browser prerequisites (`node`, Playwright, app URL).
3. `timeout_diagnostics.json` for authoritative/normalized snapshots and per-instance log tails.
4. Playwright screenshots/traces under each instance `data_dir` (`playwright-artifacts/`).

### Frontend Shell Roadmap

`aura-ui` is the shared Dioxus UI core for web-first delivery today and future multi-target shells:

1. `aura-web` (current): browser shell and harness bridge.
2. `aura-desktop` (future): desktop shell reusing `aura-ui`.
3. `aura-mobile` (future): mobile shell reusing `aura-ui`.

## Related Documentation

- [Test Infrastructure Reference](117_testkit.md) - Infrastructure details
- [Simulation Guide](805_simulation_guide.md) - Fault injection testing
- [Verification Guide](806_verification_guide.md) - Formal methods
