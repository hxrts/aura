---
name: harness-run
description: Run harness test scenarios (local, browser, or Patchbay modes).
disable-model-invocation: true
---

# Harness Test Runner

Execute end-to-end test scenarios through the harness.

The harness now runs through the shared semantic contracts in `aura-app`:
- semantic scenario steps
- shared UI/control identifiers
- structured `UiSnapshot` observation
- shared-flow support declarations and policy checks
- shared-flow parity assertions and scenario coverage declarations

For core shared scenarios, do not fall back to raw keypresses, raw CSS selectors,
or label-based button matching. Those are debugging tools, not the primary
contract.
Do not add new parity-critical waits based on sleeps, redraw polling, or text
scraping. Use the shared semantic contract, readiness/event barriers, and the
authoritative observation surfaces instead.

## Usage

```
/harness-run <scenario>              # Run scenario in local mode
/harness-run --browser <scenario>    # Run in browser mode
/harness-run --patchbay <scenario>   # Run via Patchbay relay using `patchbay` backend
```

## Scenario Locations

```
scenarios/harness/
├── local-discovery-smoke.toml       # Basic peer discovery
├── guardian-recovery.toml           # Recovery flow
├── consensus-fast-path.toml         # Consensus scenarios
└── ...
```

## Running Scenarios

### Local Mode (Default)

```bash
just harness-run scenarios/harness/local-discovery-smoke.toml
```

### Browser Mode

Requires Playwright setup:

```bash
# One-time setup
cd crates/aura-harness/playwright-driver
npm ci
npm run install-browsers

# Run browser scenario
just harness-run-browser scenarios/harness/local-discovery-smoke.toml
```

### Patchbay Mode

For testing through the relay service:

```bash
just harness-run -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/scenario2-social-topology-e2e.toml --network-backend patchbay
``` 

## Debugging Failures

### Check Order

1. **Build errors**: `just build` first
2. **Web serve logs**: `web-serve.log`
3. **Preflight report**: `preflight_report.json`
4. **Timeout diagnostics**: `timeout_diagnostics.json`
5. **Shared-flow policy**: `just ci-shared-flow-policy`
6. **UI parity contract**: `just ci-ui-parity-contract`
7. **Browser artifacts**: `playwright-artifacts/`

`timeout_diagnostics.json` is now the primary failure bundle for harness runs. It
includes:
- structured `UiSnapshot`
- runtime event history
- per-instance log tails
- render/readiness diagnostics
- browser screenshots and trace references when applicable

### Common Issues

| Symptom | Cause | Fix |
|---------|-------|-----|
| "Port in use" | Web server already running | Kill existing process |
| Browser timeout | App not responding | Check `web-serve.log` |
| Shared flow policy failure | Scenario drifted from semantic contract | Run `just ci-shared-flow-policy` and remove raw mechanics |
| UI parity contract failure | Shared web/TUI parity metadata drifted | Run `just ci-ui-parity-contract` and update `SHARED_FLOW_SUPPORT` / `SHARED_FLOW_SCENARIO_COVERAGE` |
| Discovery fails | mDNS not available | Use Patchbay mode |

## Scenario Format

```toml
schema_version = 1
id = "my-test"
goal = "Validate a shared flow semantically"

[[steps]]
id = "launch"
action = "launch_actors"
timeout_ms = 5000

[[steps]]
id = "open-chat"
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

Core shared scenarios should use semantic actions and state-based assertions:
- `navigate`
- `activate_control`
- `fill_field`
- `dismiss_transient`
- `readiness_is`
- `expect_state`

Avoid for core shared scenarios:
- raw `press_key`
- raw selector lookups
- label-based button clicks
- text scraping as the primary assertion path

## Shared-Flow Policy

Run:

```bash
just ci-shared-flow-policy
```

This validates:
- shared-flow support declarations in `aura-app`
- required app-shell and modal ids
- browser control/field mappings
- that core shared scenarios are still authored semantically

Run:

```bash
just ci-ux-policy
```

This validates:
- shared UX docs stay in sync via `scripts/check/ux-guidance-sync.sh`
- new `AURA_HARNESS_MODE` branches do not spread beyond allowlisted surfaces
- new sleeps/polling, parity-remap helpers, and row-index export patterns do not creep into guarded paths

Run:

```bash
just ci-ui-parity-contract
```

This validates:
- shared web/TUI screen/module parity declarations
- shared-flow scenario coverage declarations
- parity manifest consistency for fully shared flows

## Output

- Exit code 0: All assertions passed
- Exit code 1: Test failure
- Artifacts in `target/harness-output/`
