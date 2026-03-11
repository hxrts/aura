---
name: testing
description: Testing patterns, fixtures, harness workflows, and deterministic test construction. Use when writing tests, running harness scenarios, or debugging test failures.
---

# Testing Guide

## Quick Start

**Unit tests:**
```rust
use aura_macros::aura_test;
use aura_testkit::*;

#[aura_test]
async fn test_my_feature() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;
    // Test with real effect system
    Ok(())
}
```

**Harness scenarios:**
```bash
just harness-run -- --config configs/harness/local-loopback.toml \
  --scenario scenarios/harness/local-discovery-smoke.toml
```

## Deterministic Test Construction

### AuraEffectSystem Helpers (Required)

All tests MUST use seeded helper constructors:

```rust
use aura_agent::runtime::AuraEffectSystem;
use aura_agent::AgentConfig;

// Single instance
let effects = AuraEffectSystem::simulation_for_test(&AgentConfig::default())?;

// Named test with salt (for multiple instances)
let alice = AuraEffectSystem::simulation_for_named_test_with_salt(
    &AgentConfig::default(),
    "my_test",
    0, // salt for alice
)?;
let bob = AuraEffectSystem::simulation_for_named_test_with_salt(
    &AgentConfig::default(),
    "my_test",
    1, // salt for bob
)?;
```

**Banned constructors in test code:**
- `AuraEffectSystem::testing()` - legacy, no seed
- `AuraEffectSystem::simulation()` - raw constructor
- `AuraEffectSystem::testing_for_authority()` - legacy

### Deterministic Identifiers

```rust
use aura_core::identifiers::{AuthorityId, ContextId};
use uuid::Uuid;

// Deterministic identifiers for tests
let auth_id = AuthorityId::new_from_entropy([1u8; 32]);
let ctx_id = ContextId::from_uuid(Uuid::from_bytes([2u8; 16]));

// Multiple distinct IDs - use incrementing bytes
let auth1 = AuthorityId::new_from_entropy([1u8; 32]);
let auth2 = AuthorityId::new_from_entropy([2u8; 32]);
```

**Never use in tests:**
- `Uuid::new_v4()` - system entropy
- `rand::random()` - non-deterministic
- `thread_rng()` - system entropy

## Testing Infrastructure (aura-testkit)

### Layer Guidelines

**Layer 4-7:** Use aura-testkit fixtures and mocks
```rust
use aura_testkit::*;
use aura_macros::aura_test;

#[aura_test]
async fn test_protocol() -> aura_core::AuraResult<()> {
    let fixture = create_test_fixture().await?;
    let effects = fixture.effects();
    Ok(())
}
```

**Layer 1-3:** Use deterministic identifier constructors (avoid circular deps)

### Available Fixtures

- `create_test_fixture()` - Pre-configured effect system
- `test_key_pair(seed)` - Deterministic keypairs
- Test account/authority builders with seeds
- Multi-device scenario fixtures

### Mock Handlers

- `MockCryptoHandler`, `MockTimeHandler`, `MockStorageHandler`
- Controllable, stateful for deterministic testing
- Time control, network simulation, failure injection

## Runtime Harness

The harness executes real Aura instances in PTYs for end-to-end validation.
Use the [Testing Guide](../../../docs/804_testing_guide.md) sections on harness scenarios for command and policy details.

Shared UX rules for parity-critical flows:
- semantic ids, focus semantics, and action metadata come from `aura-app::ui_contract`
- waits must bind to readiness, runtime-event, or quiescence contracts
- observation must be side-effect free; retries/recovery are separate behaviors
- allowlisted harness-mode hooks need owner, justification, and design-note metadata
- browser harness bridge surface changes must update compatibility guidance in `crates/aura-web/ARCHITECTURE.md` and `docs/804_testing_guide.md`
- parity exceptions require structured metadata with reason, scope, affected surface, and doc reference
- raw sleeps, row-index addressing, DOM scraping, and text matching are not the primary correctness path
- shared scenarios stay actor-based and semantic-only; legacy scripted scenario mechanics belong only in quarantined non-shared fixtures
- extend typed validator domains first and keep `scripts/check/` wrappers thin instead of adding bespoke shell policy logic
- shared UX policy/documentation updates are checked by `just ci-ux-policy`

### Running Scenarios

```bash
# Lint before running
just harness-lint -- --config configs/harness/local-loopback.toml \
  --scenario scenarios/harness/local-discovery-smoke.toml

# Execute scenario
just harness-run -- --config configs/harness/local-loopback.toml \
  --scenario scenarios/harness/local-discovery-smoke.toml

# Replay for reproduction
just harness-replay -- --bundle artifacts/harness/local-loopback-smoke/replay_bundle.json
```

### Browser Harness

```bash
# Check WASM/frontend compilation
just web-check

# Serve web app
just web-serve

# Run browser scenarios (in separate shell)
just harness-run-browser scenarios/harness/local-discovery-smoke.toml
just harness-replay-browser
```

### Scenario File Format

```toml
schema_version = 1
id = "discovery-smoke"
execution_mode = "scripted"

[[steps]]
id = "launch"
action = "launch_actors"
timeout_ms = 5000

[[steps]]
id = "send"
action = "navigate"
actor = "alice"
screen_id = "chat"
timeout_ms = 2000

[[steps]]
id = "wait"
action = "readiness_is"
actor = "alice"
readiness = "ready"
timeout_ms = 2000
```

Use semantic actions and expected state in shared flows.
Examples include `navigate`, `fill_field`, `readiness_is`, `control_visible`, and `toast_contains`.
This keeps scenarios portable across TUI and browser backends.

### Interactive Mode

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

### Network Backends

```bash
# Deterministic local backend
--network-backend mock

# Native Linux Patchbay (requires Linux + capabilities)
--network-backend patchbay

# Cross-platform VM runner (macOS/Linux)
--network-backend patchbay-vm
```

For backend policy and failure triage, see the [Testing Guide](../../../docs/804_testing_guide.md) harness section.

## Property-Based Testing

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

## GuardSnapshot Pattern

Test guard chain logic synchronously:

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

## Test Commands

```bash
# All tests
just test

# Specific crate
just test-crate aura-agent

# With output
cargo test --workspace -- --nocapture

# TUI state machine tests
cargo test --package aura-terminal --test unit_state_machine

# Harness CI
just ci-harness-build
just ci-harness-contract
just ci-harness-replay
```

## Debugging Test Failures

**Non-deterministic behavior:**
- Check for `Uuid::new_v4()`, `rand::random()`, `thread_rng()`
- Use `simulation_for_named_test_with_salt()` helpers

**Timeout in simulation:**
- Check session type projections for deadlocks
- Add logging to identify stuck participants

**Harness failures:** Check in order:
1. `network_backend_preflight.json` - backend selection
2. `startup_summary.json` / `scenario_report.json` - run context
3. `events.json` / `timeline.json` - event ordering
4. `*.pcap` files - packet diagnosis

## Key Files

- Test infrastructure: `crates/aura-testkit/src/`
- Testing guide: `docs/804_testing_guide.md`
- Harness configs: `configs/harness/`
- Harness scenarios: `scenarios/harness/`
