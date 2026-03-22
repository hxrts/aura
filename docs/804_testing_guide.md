# Testing Guide

This guide covers how to write tests for Aura protocols using the testing infrastructure. It includes unit testing, integration testing, property-based testing, conformance testing, and runtime harness validation.

For infrastructure details, see [Test Infrastructure Reference](118_testkit.md). For the deterministic shared-flow design rules, see [User Flow Harness](121_user_flow_harness.md).

## 1. Core Philosophy

Aura tests follow four principles:
1. Effect-based: Tests use effect traits, never direct impure functions
2. Real handlers: Tests run actual protocol logic through real handlers
3. Deterministic: Tests produce reproducible results
4. Comprehensive: Tests validate both happy paths and error conditions

For parity-critical ownership work, completeness also means:

- compile-fail coverage where ownership/capability boundaries are enforced in
  types
- invariant tests for owner drop, stale-handle rejection, and terminality
- timeout/backoff tests for typed timeout failure, remaining-budget
  propagation, and bounded retries
- the relevant ownership `just ci-*` policy checks

### Harness Policy

Aura's runtime harness is the primary end-to-end validation lane. Default harness runs exercise the real Aura runtime with real TUI and web frontends. The goal is to catch integration failures in the actual product, not just prove a model.

The harness now has two distinct responsibilities:

- shared semantic lane:
  - executes parity-critical shared flows through the shared semantic command plane
  - waits on typed handles, readiness facts, runtime events, quiescence, and authoritative projections
  - is the primary lane for debugging production code paths
- frontend-conformance lane:
  - validates renderer-specific control wiring, DOM structure, PTY key mappings, and shell-level integration
  - may use renderer-specific mechanics intentionally
  - must not be the primary execution substrate for shared scenarios

Quint and other verification tools generate models, traces, and invariants. They are not a replacement for real frontends.

`aura-app` owns the shared semantic scenario, command-plane, and UI contracts. `aura-harness` consumes those contracts and submits shared semantic commands to real frontends. `aura-simulator` is the separate alternate runtime substrate.

Use this lane matrix when selecting harness mode.

| Lane | Backend | Command |
|------|---------|---------|
| Local deterministic | `mock` | `just harness-run -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/real-runtime-mixed-startup-smoke.toml` |
| Patchbay relay realism | `patchbay` | `just harness-run -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/real-runtime-mixed-startup-smoke.toml --network-backend patchbay` |
| Patchbay-vm relay realism | `patchbay-vm` | `just harness-run -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/real-runtime-mixed-startup-smoke.toml --network-backend patchbay-vm` |
| Browser | Playwright | `just harness-run-browser scenarios/harness/semantic-observation-browser-smoke.toml` |

All shared flows should use typed scenario primitives, typed semantic command submission, and structured snapshot/readiness waits.

Shared-semantic preflight is intentionally stricter than generic backend startup. A run config that includes SSH instances does not automatically qualify for the shared semantic lane; until a backend implements the shared semantic contract, SSH remains diagnostic-only / transport-only for harness purposes and shared-semantic scenarios must fail closed before execution.

`aura-app::ui_contract` is the canonical module for shared flow support. It defines `SharedFlowId`, `SHARED_FLOW_SUPPORT`, `SHARED_FLOW_SCENARIO_COVERAGE`, `UiSnapshot`, `compare_ui_snapshots_for_parity`, `OperationInstanceId`, and `RuntimeEventSnapshot`. Use semantic readiness and state assertions before using fallback text matching.

Direct usage of `SystemTime::now()`, `thread_rng()`, `File::open()`, or `Uuid::new_v4()` is forbidden. These operations must flow through effect traits instead.

### Shared UX Contract And Determinism

The shared UX contract is defined in [CLI and Terminal User Interface](117_user_interface.md). The `aura-app::ui_contract` module is the canonical authority for parity-critical UI identity, readiness semantics, and typed observation payloads.

For parity-critical shared-flow execution:

- shared scenarios must submit typed semantic commands through the frontend bridge
- shared scenarios must not use raw PTY keys, raw selector clicks, raw label matching, or incidental focus stepping as primary mechanics
- frontend-specific UI I/O remains valuable, but it belongs in frontend-conformance coverage rather than the main shared semantic lane
- unsupported semantic commands must fail closed and diagnostically
- command submission must enter the frontend through its real update/event path, not render-coupled polling or ad hoc harness shims

### Shared Semantic Ownership Model

Parity-critical shared semantic flows must use one explicit ownership category. Do not mix categories casually inside the same flow. The four ownership categories (`Pure`, `MoveOwned`, `ActorOwned`, `Observed`) are defined in [Ownership Model](122_ownership_model.md).

For shared semantic flows, the default expectation is:

- `aura-app` owns authoritative semantic operation coordination and typed
  lifecycle/error publication
- `aura-agent` owns long-lived runtime/service actors and other actor-owned
  async state
- `aura-terminal` and `aura-web` submit commands and observe lifecycle; they do
  not own terminal semantic truth
- `aura-harness` consumes typed handles, readiness, and projections; it does
  not mutate semantic lifecycle directly

If a migrated parity-critical flow needs both actor and move semantics, the split must stay explicit:

- the actor owns mutable lifecycle state
- move-owned handles/tokens define which caller may advance or transfer it

If that split is not explicit, the flow is not considered correct by construction.

For parity-critical observation:

- `UiSnapshot` and render-convergence data are authoritative
- observation surfaces must be side-effect free
- recovery and retries must be explicit and separate from observation
- DOM/text fallback paths are diagnostics only and must not become success-path observation behavior
- browser semantic observation must fail closed when the published snapshot is
  unavailable; it must not silently repair by reading a live controller/model
  snapshot behind the harness bridge
- channel-binding responses must either carry authoritative context materialization or fail explicitly; selected ids or labels alone are not semantic bindings
- channel list item ids and selected-channel snapshot ids must stay keyed by canonical channel ids when the runtime projection already provides them; harness/browser code should not round-trip through display labels on those paths
- diagnostic tool/query surfaces should say `diagnostic_*` at the API boundary when they are derived from screen/DOM capture rather than authoritative semantic state
- onboarding must publish through the same semantic snapshot path as the rest of the UI
- placeholder IDs, override-backed exports, and heuristic success/event synthesis are not acceptable correctness paths

For parity-critical waits and assertions:

- waits must bind to declared readiness, event, or quiescence conditions
- waits may also bind to typed operation handles or strictly newer authoritative projections when the shared contract defines them
- when a runtime bridge surface exposes typed lifecycle such as
  `DiscoveryTriggerOutcome`, `CeremonyProcessingOutcome`, or an explicit
  mutation outcome, tests should assert those variants directly instead of
  treating `Ok(())` as sufficient proof of progress
- executor-side follow-on waits should carry typed submission evidence from the issued receipt into the declared contract barriers; do not keep a second harness-local convergence graph
- projection-based semantic waits may resume across bounded browser/runtime restarts only by clearing stale freshness baselines and re-entering typed snapshot observation; runtime-event, toast, and exact operation-state waits still fail closed across restarts
- semantic issue success must come from typed command receipts and authoritative runtime facts, not from visible homes, modal closure, message appearance, selected-list state, or `Submitting`
- shared semantic harness core should decode typed `ToolPayload` and bridge structs directly; keep raw `serde_json::Value` plumbing at outer CLI/browser adapters only
- raw sleeps, redraw polling, DOM scraping, and fallback text matching are diagnostics only
- harness mode may change instrumentation and render stability, but it must not change business-flow semantics

### Ownership Test Expectations

When a change introduces or modifies a parity-critical ownership boundary, the test plan should include the applicable items below.

- compile-fail tests for private constructors, capability misuse, or stale
  move-owned handles when the boundary is type-enforced
- invariant tests proving owner drop reaches explicit failure or cancellation
- invariant tests proving terminal lifecycle does not regress on the same
  logical instance
- invariant tests proving observed layers do not author semantic lifecycle
- invariant tests proving frontend-local submission yields immediately to the
  app-owned workflow owner after handoff
- timeout/backoff tests proving local wall-clock policy only changes budget and
  diagnostics, not semantic success/failure rules
- the relevant ownership/time `just ci-*` policy checks in addition to crate
  tests

For shared semantic workflow changes specifically:

- `aura-app::workflows` is the authoritative publication owner
- `aura-terminal`, `aura-web`, and `aura-harness` must not retain a parallel
  terminal publication path after handoff
- review and test plans should name the terminal owner explicitly and treat
  frontend layers as submit/observe boundaries

Use physical time for local deadline and backoff policy. Do not use wall-clock timeouts as the primary proof of distributed completion or ordering.

For failure analysis:

- prefer canonical action/event/state traces and structured timeout diagnostics
- treat final text or screenshot inspection as supporting evidence, not the primary oracle
- replay bundles should compare typed tool-response payload meaning, not just top-level `Ok` vs `Error` shape

For ownership cleanup discipline:

- every shared UX or harness contract hardening change should remove obsolete
  compatibility code, stale allowlist entries, and transitional comments in the
  same milestone or the next explicit cleanup pass
- prefer extending typed governance in `cargo run -p aura-harness --bin aura-harness --quiet -- governance ...` over adding standalone shell policy logic

### Ownership Cleanup Tasks

Each parity-critical ownership change must include explicit cleanup work for the abstraction it replaces. Do not treat the ownership model as additive.

For every migrated flow, include these cleanup questions and complete the matching deletion work in the same migration or the next named cleanup task:

- delete actor wrappers around purely local/value transitions that should stay
  `Pure`
- delete shared mutable ownership state where a `MoveOwned` handoff or
  owner-token surface is the correct model
- delete detached callback/task ownership for state that should instead live
  under one `ActorOwned` coordinator

If a change leaves one of those old abstractions in place, record it as
explicit ownership cleanup debt with the owning module and removal milestone. Do not
hide it behind "temporary" ambient lifecycle helpers, duplicate readiness
emitters, or shell-local terminal state.

The authoritative written update map for these surfaces lives in
`scripts/check/user-flow-guidance-sync.sh` and is enforced by `just ci-user-flow-policy`.
Ownership-model policy for the shared semantic lane is enforced through the
final CI entrypoints:

- `just ci-ownership-policy` for the aggregate ownership/runtime boundary lane
- `just ci-harness-ownership-policy` for the harness-specific ownership policy
- `just ci-user-flow-policy` for shared UX governance and documentation sync

Testing/enforcement split:

- prefer `trybuild` compile-fail coverage when the misuse is fundamentally an
  API-shape or visibility violation
- prefer Rust-native lint binaries in `aura-macros` when the misuse is a
  syntax-level boundary or naming/flow-shape rule
- keep shell scripts for repo-wide governance, integration topology, or
  end-to-end harness policy that cannot realistically be proved at compile time
- when a stronger contract lands, remove the superseded legacy helper,
  compatibility branch, migration shim, or stale regression fixture rather than
  leaving both paths active

The authoritative frontend matrix for converted shared scenarios comes from `scenarios/harness_inventory.toml` and is enforced by `just ci-harness-matrix-inventory`. Allowlisted harness-mode hooks must carry explicit owner, justification, and design-note references in `scripts/check/user-flow-policy-guardrails.sh`. Changes to the browser harness bridge request/response or observation surface must update both `crates/aura-web/ARCHITECTURE.md` and this guide so compatibility expectations stay explicit. The current browser compatibility surface includes the explicit `stage_runtime_identity` bootstrap handoff entrypoint plus the page-owned semantic submission queue (`window.__AURA_DRIVER_SEMANTIC_ENQUEUE__`). The bootstrap staging/handoff promise is completion-based: callers may treat it as confirmation that the owned bootstrap/rebootstrap transition finished, not merely that the request was queued. Channel-returning bridge responses now distinguish weak selected-channel ids from authoritative channel bindings; a payload that lacks context is not a binding. Browser harness failures also surface explicit publication-state diagnostics through `window.__AURA_UI_PUBLICATION_STATE__` and `window.__AURA_RENDER_HEARTBEAT_PUBLICATION_STATE__`; those globals are diagnostic-only and do not replace the authoritative `UiSnapshot`/`RenderHeartbeat` payloads. Browser-owned semantic snapshot publication should flow through one helper aligned with `UiController::publish_ui_snapshot`, and browser-owned maintenance polling should share one bounded helper for sleep/cancellation/pause reporting so those paths stay uniform and clearly non-semantic. Parity exceptions must remain typed metadata in `aura-app::ui_contract` with a reason code, scope, affected surface, and authoritative doc reference.

### Shared Semantic Ownership Inventory

Use this as the authoritative ownership map for the shared semantic stack. If code does not match this table, treat it as ownership cleanup debt rather than as an acceptable alternate pattern.

| Subsystem | Crate / locus | Ownership category | Authoritative owner | May mutate | May observe only |
|-----------|----------------|--------------------|---------------------|------------|------------------|
| Semantic command / handle contract | `aura-app::ui_contract`, `aura-app::scenario_contract` | `Pure` + `MoveOwned` | `aura-app` contract surfaces | `aura-app` contract/workflow modules | `aura-terminal`, `aura-web`, `aura-harness` |
| Semantic operation lifecycle | `aura-app::workflows::*`, authoritative semantic facts | `MoveOwned` | authoritative workflow coordinator per operation | workflow/coordinator modules in `aura-app` | frontend render crates, harness |
| Channel / invitation / delivery readiness | `aura-app::workflows::*`, runtime-backed readiness coordinators | `ActorOwned` | single-owner readiness coordinator | coordinator modules and their sanctioned runtime hooks | shell/subscription/render code, harness |
| Runtime-facing async service state | `aura-agent::runtime::*`, `aura-agent::handlers::*` | `ActorOwned` | runtime service / coordinator actor | actor/service owner and sanctioned commands | `aura-app`, frontends, harness |
| TUI command ingress | `aura-terminal::tui::harness_state`, update loop, dispatch path | `ActorOwned` ingress + `Observed` rendering | TUI update/event loop | ingress/update-loop code only | shell render code, harness reads |
| TUI shell / callbacks / subscriptions | `aura-terminal::tui::screens`, `callbacks`, `subscriptions` | `Observed` | downstream of authoritative workflow/runtime state | local UI state only, never semantic terminal truth | harness, user-visible rendering |
| Browser harness bridge | `aura-web::harness_bridge` | `ActorOwned` bridge mechanics + `Observed` publication | browser bridge installation / ingress | bridge module only | Playwright/harness, browser render layer |
| Harness executor / wait model | `aura-harness::executor`, `backend::*`, `tool_api` | `Observed` + orchestration-local `ActorOwned` state | harness coordinator for test orchestration | harness orchestration state only, never product semantic truth | scenario authors, CI, diagnostics |
| Ownership transfer / stale-owner invalidation | operation handles, owner tokens, consumed handoff records | `MoveOwned` | the current token/record holder | sanctioned transfer APIs only | projections, render, harness diagnostics |

The required split is:

- actor-owned subsystems own long-lived mutable async state and lifecycle

Reactive subscription policy for tests:
- subscribing before registration must fail with `ReactiveError::SignalNotFound`
- tests must not treat an empty stream as equivalent to "signal not registered"
- lagging subscribers are allowed to miss intermediate updates; assertions
  should target eventual newer snapshots, not lossless delivery
- move-owned handles/tokens invalidate stale holders by construction
- observed layers render and assert, but do not author semantic truth
- TUI-local semantic submission is limited to the sanctioned local-terminal and
  workflow-handoff owner wrappers
- browser bridge concurrency is limited to `WebTaskOwner`; it does not own
  parity-critical lifecycle
- Playwright stages browser runtime identity through the explicit bridge
  entrypoint before rebootstrap and submits semantic commands through the
  page-owned semantic queue instead of mutating browser lifecycle state
  directly
- authoritative readiness refresh remains private to `aura-app::workflows` and
  is compile-fail tested in both default and `signals` configurations

### Required Ownership Invariants

Ownership-model migrations are not complete until the following test classes exist for the affected parity-critical surface:

- compile-fail guards for private constructors, wrong-capability issuance, and
  stale-owner misuse where the boundary is enforced in types
- dynamic invariant tests proving owner drop reaches explicit terminal failure
  or cancellation
- dynamic invariant tests proving terminal states do not regress on the same
  logical operation instance
- handle/instance tests proving stale handles do not match or advance the wrong
  operation instance after transfer or replacement
- concurrency tests for actor-owned coordinators where lost updates or multiple
  live owners are plausible
- timeout/backoff invariant tests proving typed timeout failure,
  remaining-budget propagation, bounded attempts, and local-choice scaling

If a flow changes ownership model or timeout policy and these test classes do not move with it, treat the migration as incomplete.
- move-owned surfaces own exclusive right-to-act and ownership transfer
- observed surfaces render, wait, and diagnose without authoring semantic truth

Do not use this table to justify ambient shared ownership. If a subsystem needs both actor and move semantics, the actor owns mutable lifecycle state while the move-owned handle or token defines who may advance or transfer it.

### Release And Update Matrix Expectations

OTA and module release/update validation must follow the same semantic-lane contract as other parity-critical shared flows. The OTA contract requirements (typed command/control surfaces, scoped activation lifecycle, and rollback semantics) are defined in [Distributed Maintenance Architecture](116_maintenance.md). Each release row in [Flow Coverage](997_flow_coverage.md) must map to those typed lifecycle surfaces.

Frontend-conformance coverage may validate release-screen wiring, but it does not satisfy OTA/module lifecycle validation on its own.

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
use aura_core::types::identifiers::AuthorityId;

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

When shared flows export data through runtime events, the event payload is part of the contract. Invitation and device-enrollment code capture should come from `RuntimeFact` payloads in `UiSnapshot.runtime_events`, not clipboard scraping or frontend-local heuristics. Shared chat waits should likewise bind to semantic selection state so the harness targets the single shared channel instead of falling back to incidental render order.

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
Use native `patchbay` on Linux CI and Linux developers when capabilities are available. Use `patchbay-vm` on macOS and as Linux fallback to run the same scenarios in a Linux VM. Keep deterministic non-network logic in `mock` backend tests to preserve fast feedback.
`patchbay-vm` relies on the explicit harness work/artifact directories and `QEMU_VM_WORK_DIR`; the removed `.qemu-vm` redirect path is no longer part of the supported workflow.

Implementation follows three tiers. Tier 1 covers deterministic and property tests in `aura-testkit` for retry and path-selection invariants. Tier 2 covers Patchbay integration scenarios in `aura-harness` for PR gating. Tier 3 covers Patchbay stress and flake detection suites on scheduled CI.

When a scenario fails, triage artifacts in this order.
1. Check `network_backend_preflight.json` to confirm selected backend and fallback reason.
2. Check `startup_summary.json` and `scenario_report.json` for run context and failing step.
3. Check `events.json` and backend timeline artifacts for event ordering.
4. Check namespace and network dumps and pcap files for packet and routing diagnosis.
5. Check agent logs for authority-local failures and retry state transitions.

For harness-specific state debugging, treat `timeout_diagnostics.json` as the first failure bundle. It includes semantic state snapshots, render readiness, and runtime event history.

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

For mixed-runtime debugging, inspect `runtime_events` before logs when a code exchange or chat handoff fails. The expected evidence is a typed event payload, the selected semantic target in the snapshot, and only then supporting browser or TUI render diagnostics.

For browser runs, the harness observes the semantic state contract first and uses DOM/text fallbacks only for diagnostics. If semantic state and rendered UI diverge, treat that as a product or frontend contract bug rather than papering over it with text-based assertions.

### Frontend Shell Roadmap

`aura-ui` is the shared Dioxus UI core. It supports web-first delivery today and future multi-target shells.

1. `aura-web` (current): browser shell and harness bridge
2. Desktop shell (future): desktop-specific shell reusing `aura-ui`
3. Mobile shell (future): mobile-specific shell reusing `aura-ui`

## Related Documentation

- [Test Infrastructure Reference](118_testkit.md) - Infrastructure details
- [Simulation Guide](805_simulation_guide.md) - Fault injection testing
- [Verification Guide](806_verification_guide.md) - Formal methods
