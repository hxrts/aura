# Runtime Harness Guide

## Purpose

The runtime harness executes one or more real Aura instances in PTYs.
It provides deterministic control over input, screen capture, restart, and replay.
It is used for end-to-end runtime validation and UI-level evidence.

The harness complements the simulator.
The simulator remains the primary system for protocol correctness and fault-space exploration.
See [Simulation Infrastructure Reference](118_simulator.md) for simulator design and usage.

## Scope

Use the harness when you need runtime behavior with real binaries.
Use it for local loopback and mixed local plus SSH topology checks.
Use it when you need replay bundles and timeout diagnostics.

Do not use the harness as a replacement for simulator property testing.
Use simulator checks for semantic guarantees.
Use harness checks for runtime integration behavior.

## Prerequisites

Run commands from the repository root inside `nix develop`.
Use TOML run configs and TOML scenario files.
Use unique `data_dir` paths per local instance.

Use the baseline files in `configs/harness/` and `scenarios/harness/`.
Treat `work/` as scratch.
Do not rely on `work/` as an authoritative spec.

## Run Config

The run config defines instance topology and runtime limits.
`schema_version` is required and must be `1`.
Every instance must have a unique `id` and `bind_address`.

```toml
schema_version = 1

[run]
name = "local-loopback-smoke"
pty_rows = 40
pty_cols = 120
seed = 4242
max_cpu_percent = 95
require_remote_artifact_sync = false

[[instances]]
id = "alice"
mode = "local"
data_dir = ".tmp/harness/alice"
device_id = "alice-dev-01"
bind_address = "127.0.0.1:41001"
demo_mode = false

[instances.lan_discovery]
enabled = true
bind_addr = "127.0.0.1"
broadcast_addr = "127.0.0.1"
port = 19433
```

This example mirrors the current local sample config.
Resource limits are optional.
Remote sync enforcement is controlled by `run.require_remote_artifact_sync`.

## Scenario File

The scenario file defines scripted or agent-mode step execution.
`schema_version` is required and must be `1`.
Supported `execution_mode` values are `scripted` and `agent`.

```toml
schema_version = 1
id = "mixed-topology-smoke"
goal = "Exercise local plus ssh-dry-run topology with state-machine execution."
execution_mode = "scripted"

[[steps]]
id = "launch"
action = "launch_instances"
timeout_ms = 5000

[[steps]]
id = "local-send"
action = "send_keys"
instance = "alice"
expect = "mixed-topology-msg\n"
timeout_ms = 2000

[[steps]]
id = "local-wait"
action = "wait_for"
instance = "alice"
expect = "mixed-topology-msg"
timeout_ms = 2000
```

For `send_keys`, the value in `expect` is the key payload sent to the instance.
For `wait_for`, the value in `expect` is the required match pattern.
Scenario lint rejects unknown instance references and unsupported action names.

## Common Commands

Use lint before long runs.
Then execute the run.
Use replay for deterministic reruns.

```bash
just harness-lint -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/local-discovery-smoke.toml
```

This command validates run and scenario semantics.
It fails on schema errors, unknown instances, and unsupported step actions.

```bash
just harness-run -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/local-discovery-smoke.toml
```

This command runs instances and optional scenario execution.
It writes an artifact bundle under `artifacts/harness/<run-name>/`.
It also records replay and seed metadata.

```bash
just harness-replay -- --bundle artifacts/harness/local-loopback-smoke/replay_bundle.json
```

This command replays the recorded action log without planner decisions.
Use it to reproduce deterministic failures and verify regression fixes.
Replay checks bundle and tool API compatibility.

## Interactive LLM Workflow

Use interactive mode for manual runbooks and exploratory debugging.
Use a persistent `tool_repl` session for multi-step control.
Do not use the one-shot `tool` subcommand for long sessions.

```bash
cargo run -p aura-harness --bin tool_repl -- --config work/harness/manual/two_tui_run_bind_run91.toml
```

This starts a long-lived JSON-line REPL.
The process keeps instances alive until `quit` or `exit`.
This is the correct mode for full manual end-to-end flows.

```json
{"method":"negotiate","params":{"client_versions":["1.0","0.2"]}}
{"method":"screen","params":{"instance_id":"alice"}}
{"method":"send_keys","params":{"instance_id":"alice","keys":"3n"}}
{"method":"wait_for","params":{"instance_id":"alice","pattern":"Create Invitation","timeout_ms":4000}}
```

Send one JSON request per line.
The current supported API versions are `1.0`, `0.2`, and `0.1`.
`wait_for` matches against normalized screen text.

```json
{"method":"send_key","params":{"instance_id":"alice","key":"enter"}}
{"method":"tail_log","params":{"instance_id":"alice","lines":50}}
{"method":"restart","params":{"instance_id":"bob"}}
{"method":"kill","params":{"instance_id":"bob"}}
```

Use `send_key` for named keys such as `enter`, `esc`, `tab`, and arrows.
Use `tail_log` for runtime diagnostics.
Set `log_path` in the run config if you need non-empty `tail_log` output.

## Manual Invitation and Chat Flow

The harness supports a complete manual invitation and chat flow.
Use two instances and deterministic message tokens.
Capture evidence after each phase.

```text
Phase 1:
1) Create invitation on Alice in Contacts.
2) Press c on the invitation modal to copy the full code.
3) Import the code on Bob and confirm Contacts (1) on both sides.

Phase 2:
1) Create a channel on Alice.
2) In member selection, verify "1 selected" before continuing.
3) Confirm Channels (1) appears on Bob.

Phase 3:
1) Exchange msg-a-1, msg-b-1, msg-a-2, msg-b-2.
2) Confirm each token appears on both screens before sending the next token.
```

This is the validated manual checklist for the current harness and TUI workflow.
For headless capture of copied invitation text, set `AURA_CLIPBOARD_FILE` in each instance `env`.
Read the file after pressing `c` to get the full out-of-band payload.

## Artifacts

Each `run` writes a machine-readable bundle.
The default root is `artifacts/harness/<run-name>/`.
Use this directory as the primary debugging source.

Key files:
- `startup_summary.json`
- `preflight_report.json`
- `events.json`
- `initial_screens.json`
- `replay_bundle.json`
- `seed_bundle.json`
- `resource_report.json`
- `remote_artifact_sync.json`
- `scenario_report.json` when a scenario is provided
- `timeout_diagnostics.json` when scenario execution times out

These files provide startup metadata, event history, and deterministic replay data.
`timeout_diagnostics.json` includes authoritative, raw, and normalized screen captures.
Use these together to diagnose flaky pattern matching.

## CI Usage

Use the dedicated harness lanes in CI.
Keep these lanes green for harness stability.
Use replay in CI to validate deterministic reproduction.

```bash
just ci-harness-build
just ci-harness-contract
just ci-harness-replay
```

These commands build the crate, run contract tests, and execute replay validation.
This set is the minimum gate for runtime harness changes.
Run them before landing harness modifications.

## Troubleshooting

If startup fails, run `just harness-lint` first.
Most early failures are config validation failures.
Fix schema or instance wiring before rerunning.

If `wait_for` times out, check `timeout_diagnostics.json` and `events.json`.
Confirm you matched the right screen string after normalization.
If needed, rerun from `replay_bundle.json` to confirm determinism.

If `tail_log` returns an empty list, set `log_path` per instance.
`tail_log` reads from configured log files only.
It does not scrape PTY output directly.

## Related Docs

See [Simulation Infrastructure Reference](118_simulator.md) for simulator architecture.
See [Testing Guide](805_testing_guide.md) for testing patterns and fixture guidance.
