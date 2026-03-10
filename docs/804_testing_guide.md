# Testing Guide

This guide covers how to write tests for Aura protocols using the testing infrastructure. It includes unit testing, integration testing, property-based testing, conformance testing, and runtime harness validation.

For infrastructure details, see [Test Infrastructure Reference](117_testkit.md).

## 1. Core Philosophy

Aura tests follow four principles:
1. Effect-based: Tests use effect traits, never direct impure functions
2. Real handlers: Tests run actual protocol logic through real handlers
3. Deterministic: Tests produce reproducible results
4. Comprehensive: Tests validate both happy paths and error conditions

### Harness Policy

Aura's runtime harness is the primary end-to-end validation lane.
Default harness runs exercise the real Aura runtime with real TUI and webfront ends.
The goal is to catch integration failures in the actual product, not just prove a model.

Quint and other verification tools generate models, traces, and invariants.
They are not a replacement for real frontends.

`aura-app` owns the shared semantic scenario and UI contracts.
`aura-harness` consumes those contracts and drives real frontends.
`aura-simulator` is the separate alternate runtime substrate.

Use this lane matrix when selecting harness mode.

| Lane | Backend | Command |
|------|---------|---------|
| Local deterministic | `mock` | `just harness-run -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/local-discovery-smoke.toml` |
| Patchbay relay realism | `patchbay` | `just harness-run -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/scenario2-social-topology-e2e.toml --network-backend patchbay` |
| Patchbay-vm relay realism | `patchbay-vm` | `just harness-run -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/scenario2-social-topology-e2e.toml --network-backend patchbay-vm` |
| Browser | Playwright | `just harness-run-browser scenarios/harness/local-discovery-smoke.toml` |

All shared flows should use typed scenario primitives and structured snapshot waits.

`aura-app::ui_contract` is the canonical module for shared flow support.
It defines `SharedFlowId`, `SHARED_FLOW_SUPPORT`, `SHARED_FLOW_SCENARIO_COVERAGE`,
`UiSnapshot`, `compare_ui_snapshots_for_parity`, `OperationInstanceId`, and
`RuntimeEventSnapshot`.
Use semantic readiness and state assertions before using fallback text matching.

Direct usage of `SystemTime::now()`, `thread_rng()`, `File::open()`, or `Uuid::new_v4()` is forbidden. These operations must flow through effect traits instead.

### Shared UX Contract And Determinism

For parity-critical shared flows, `aura-app::ui_contract` is the authoritative
contract surface. It owns:

- canonical screen, modal, control, field, list, and operation identifiers
- focus and selection semantics
- shared-flow support and coverage metadata
- `UiSnapshot`, `RenderHeartbeat`, and typed runtime-event shapes

The web shell and the TUI must consume that contract rather than deriving local
IDs, local focus semantics, or ad hoc flow metadata.

For parity-critical observation:

- `UiSnapshot` and render-convergence data are authoritative
- observation surfaces must be side-effect free
- recovery and retries must be explicit and separate from observation
- DOM/text fallback paths are diagnostics only and must not become success-path observation behavior
- onboarding must publish through the same semantic snapshot path as the rest of the UI
- placeholder IDs, override-backed exports, and heuristic success/event synthesis are not acceptable correctness paths

For parity-critical waits and assertions:

- waits must bind to declared readiness, event, or quiescence conditions
- raw sleeps, redraw polling, DOM scraping, and fallback text matching are diagnostics only
- harness mode may change instrumentation and render stability, but it must not change business-flow semantics

For failure analysis:

- prefer canonical action/event/state traces and structured timeout diagnostics
- treat final text or screenshot inspection as supporting evidence, not the primary oracle

The authoritative written update map for these surfaces lives in
`scripts/check/ux-guidance-sync.sh` and is enforced by `just ci-ux-policy`.
The authoritative frontend matrix for converted shared scenarios comes from
`scenarios/harness_inventory.toml` and is enforced by
`just ci-harness-matrix-inventory`.

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
        .map(|i| {
            AuraEffectSystem::simulation_for_named_test_with_salt(
                &AgentConfig::default(),
                "test_threshold_workflow",
                i as u64,
            )
        })
        .collect();

    let result = execute_protocol(&effect_systems?, &device_ids).await?;
    assert!(result.is_complete());
    Ok(())
}
```

Use `simulation_for_test*` helpers for all tests. For multi-instance tests from one callsite, use `simulation_for_named_test_with_salt(...)` and keep the identity and salt stable. This allows failures to be replayed deterministically.

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

### Quint Trace Usage

Quint traces are model artifacts. Export them through the shared semantic
scenario contract and execute real TUI/web flows through `aura-harness`
rather than replaying Quint traces directly against the TUI implementation.

## 9. Conformance Testing

Conformance tests validate that implementations produce identical results across environments.

### Conformance Lanes

CI runs two lanes.

Strict lane (native vs WASM cooperative):
```bash
just ci-conformance-strict
```

Differential lane (native threaded vs cooperative):
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

The harness is the single executor for real frontend scenarios. Scripted mode uses the shared semantic scenario contract. Agent mode uses LLM-driven execution toward goals.

Shared flows should be authored semantically once, then executed through the
harness using either the TUI or browser driver. Do not create a second
frontend execution path for MBT or simulator replay.

Core shared scenarios should use semantic actions and state-based assertions.
Avoid raw selector steps, raw `press_key` steps, and label-based browser clicks
except in dedicated low-level driver tests.

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
id = "discovery-smoke"
goal = "Validate semantic harness observation against a real TUI"

[[steps]]
id = "launch"
action = "launch_actors"
timeout_ms = 5000

[[steps]]
id = "nav-chat"
actor = "alice"
action = "navigate"
screen_id = "chat"
timeout_ms = 2000

[[steps]]
id = "chat-ready"
actor = "alice"
action = "readiness_is"
readiness = "ready"
timeout_ms = 2000
```

### Running the Harness

```bash
# Lint before running
just harness-lint -- --config configs/harness/local-loopback.toml \
  --scenario scenarios/harness/semantic-observation-tui-smoke.toml

# Execute
just harness-run -- --config configs/harness/local-loopback.toml \
  --scenario scenarios/harness/semantic-observation-tui-smoke.toml

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
just ci-shared-flow-policy
```

`just ci-shared-flow-policy` validates the shared-flow contract end to end. It checks that `aura-app` shared-flow support declarations are internally consistent, that every fully shared flow has explicit parity-scenario coverage, and that required shell and modal ids still exist. It confirms browser control and field mappings still line up with the shared contract and that core shared scenarios have not drifted back to raw mechanics.

Use `just ci-ui-parity-contract` for the narrower parity gate. That lane validates shared screen/module mappings, shared-flow scenario coverage, and parity-manifest consistency without running a full scenario matrix.

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

Test one behavior per function and name tests descriptively. Use fixtures for common setup. Prefer real handlers over mocks.

Test error conditions explicitly. Avoid testing implementation details. Focus on observable behavior.

Keep tests fast. Parallelize independent tests.

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

Patchbay is the authoritative NAT-realism backend for holepunch validation.
Use native `patchbay` on Linux CI and Linux developers when capabilities are available.
Use `patchbay-vm` on macOS and as Linux fallback to run the same scenarios in a Linux VM.
Keep deterministic non-network logic in `mock` backend tests to preserve fast feedback.

Implementation follows three tiers.
Tier 1 covers deterministic and property tests in `aura-testkit` for retry and path-selection invariants.
Tier 2 covers Patchbay integration scenarios in `aura-harness` for PR gating.
Tier 3 covers Patchbay stress and flake detection suites on scheduled CI.

When a scenario fails, triage artifacts in this order.
1. Check `network_backend_preflight.json` to confirm selected backend and fallback reason.
2. Check `startup_summary.json` and `scenario_report.json` for run context and failing step.
3. Check `events.json` and backend timeline artifacts for event ordering.
4. Check namespace and network dumps and pcap files for packet and routing diagnosis.
5. Check agent logs for authority-local failures and retry state transitions.

For harness-specific state debugging, treat `timeout_diagnostics.json` as the first failure bundle.
It includes semantic state snapshots, render readiness, and runtime event history.

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
just harness-lint-browser scenarios/harness/semantic-observation-browser-smoke.toml

# Run browser scenarios
just harness-run-browser scenarios/harness/semantic-observation-browser-smoke.toml

# Replay the latest browser run bundle
just harness-replay-browser
```

Browser harness artifacts are written under:

```text
artifacts/harness/browser/
```

When debugging browser failures, check `web-serve.log` for bundle and runtime startup issues. Check `preflight_report.json` for browser prerequisites including Node, Playwright, and app URL. Check `timeout_diagnostics.json` for authoritative and normalized snapshots and per-instance log tails. Playwright screenshots and traces are stored under each instance `data_dir` in `playwright-artifacts/`.

`timeout_diagnostics.json` is now the primary authoritative failure bundle. In addition to `UiSnapshot`, it should be treated as the first source for runtime event history through `runtime_events`. It contains operation lifecycle and instance ids. It provides render and readiness diagnostics along with browser and TUI backend log tails.

For browser runs, the harness observes the semantic state contract first and
uses DOM/text fallbacks only for diagnostics. If semantic state and rendered UI
diverge, treat that as a product or frontend contract bug rather than papering
over it with text-based assertions.

### Frontend Shell Roadmap

`aura-ui` is the shared Dioxus UI core. It supports web-first delivery today and future multi-target shells.

1. `aura-web` (current): browser shell and harness bridge
2. Desktop shell (future): desktop-specific shell reusing `aura-ui`
3. Mobile shell (future): mobile-specific shell reusing `aura-ui`

## Related Documentation

- [Test Infrastructure Reference](117_testkit.md) - Infrastructure details
- [Simulation Guide](805_simulation_guide.md) - Fault injection testing
- [Verification Guide](806_verification_guide.md) - Formal methods
