# Runtime Harness Guide

## Purpose

The runtime harness executes one or more real Aura instances.
It provides deterministic control over terminal IO, runtime networking, and scenario execution.
It is an integration test system for production-like behavior.

The harness complements the simulator.
The simulator remains the primary system for protocol correctness and fault-space exploration.
See [Simulation Infrastructure Reference](118_simulator.md) for simulator design and usage.

## Scope

Use the harness when you need runtime and UI evidence.
Use it for local loopback, mixed local plus SSH topologies, and replay-based regression checks.
Use it when artifact bundles are required for postmortem analysis.

Do not use the harness as a replacement for simulator property testing.
Use simulator checks for semantic protocol guarantees.
Use harness checks for end-to-end runtime behavior.

## Prerequisites

Run commands from the repository root inside the project development environment.
The harness expects TOML run and scenario files.
It also expects `aura-harness` commands exposed through `just` recipes.

The default sample files are under `configs/harness/` and `scenarios/harness/`.
Use these files as the baseline for new topologies and scenarios.
Use unique local storage paths per instance.

## Inputs

The harness consumes two input files.
The run config defines instances, topology, and runtime budgets.
The scenario config defines ordered actions and assertions.

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
data_dir = ".aura-harness/alice"
bind_address = "127.0.0.1:41001"
command = "bash"
args = ["-lc", "cat"]
```

This run config starts one local PTY-backed instance.
The `seed` value drives deterministic scheduling and fault behavior.
Resource limits are optional and can emit violation events into artifacts.

```toml
schema_version = 1
id = "local-discovery-smoke"
goal = "validate basic operator path"
execution_mode = "scripted"

[[steps]]
id = "launch"
action = "noop"

[[steps]]
id = "wait-banner"
action = "wait_for"
instance = "alice"
expect = "Aura"
timeout_ms = 2000
```

This scenario expresses a simple scripted flow.
Each step has an explicit action and optional assertion fields.
`wait_for` failures become structured diagnostics in the artifact bundle.

## Common Commands

Use `harness-lint` before long runs.
Then run scenarios with `harness-run`.
Use replay for deterministic reruns without agent decisions.

```bash
just harness-lint -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/local-discovery-smoke.toml
```

This command validates schema and semantic constraints before process launch.
It fails fast on unresolved instances, invalid transitions, and capability mismatches.

```bash
just harness-run -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/local-discovery-smoke.toml
```

This command executes the scenario and prints a structured startup summary.
It writes artifacts under `artifacts/harness/<run-name>/`.
It also records replay and seed metadata for deterministic reruns.

```bash
just harness-run -- --config configs/harness/local-plus-ssh.toml --scenario scenarios/harness/local-discovery-smoke.toml
```

This command runs a mixed topology with one or more SSH instances.
Each SSH instance must set `ssh_host` and `remote_workdir`.
Tunnel rewrites are resolved through the routing layer and captured in artifacts.

```bash
just harness-replay -- --bundle artifacts/harness/local-loopback-smoke/replay_bundle.json
```

This command replays recorded tool actions without an LLM planner.
Use it to validate regressions and to reproduce flaky paths deterministically.
Replay enforces compatible schema and tool API versions.

## Interactive LLM Workflow

The harness also supports manual LLM-driven operation.
This mode is for interactive sessions where the operator reads output and decides the next action.
Use this mode for exploratory validation and for plain English runbooks.

### Mode Selection

Use the `tool` command for single-request smoke checks.
Use a long-lived `ToolApi` coordinator session for full multi-step manual operation.
The `tool` command restarts instances per invocation, so it is not a full interactive control plane by itself.

```bash
cargo run -p aura-harness -- tool --config configs/harness/local-loopback.toml --request-json '{"method":"screen","params":{"instance_id":"alice"}}'
```

This command sends one tool request and prints one response payload.
Use it to verify request format and expected response shape before running a longer interactive session.
Use `method` values such as `negotiate`, `screen`, `send_keys`, `wait_for`, `tail_log`, `restart`, and `kill`.

### Operator Loop

Recommended manual operator loop:

1. Start two instances and negotiate a compatible tool API version.
2. Read each actor screen and identify the next required UI action.
3. Send keys for one actor, then validate the expected state with `wait_for`.
4. Tail logs when screen evidence is ambiguous.
5. Record artifacts after each milestone.

### Invitation and Chat Checks

For out-of-band invitation scenarios, copy the invitation text from Actor A output and input it into Actor B.
Then verify both sides report an active relationship.
Store the invitation payload and both screens in session evidence.

For chat scenarios, use deterministic message tokens such as `msg-a-1` and `msg-b-1`.
Verify each token appears on both actors before sending the next token.
If delivery fails twice for the same step, stop the run and preserve artifacts for diagnosis.

## Artifact Bundle

Each run writes a machine-readable bundle.
The bundle location is `artifacts/harness/<run-name>/`.
Use these files as the primary debugging input.

Key files:

- `startup_summary.json`: run identity, instance list, and startup metadata.
- `events.json`: structured harness event stream.
- `replay_bundle.json`: recorded requests, responses, routing metadata, and seeds.
- `seed_bundle.json`: run, scenario, fault, and per-instance seeds.
- `resource_report.json`: CPU, memory, and file descriptor samples and violations.
- `remote_artifact_sync.json`: SSH artifact sync status, source metadata, and checksums.
- `timeout_diagnostics.json`: per-instance screen and log snapshots on timeout.

Remote sync can be required with `run.require_remote_artifact_sync = true`.
When required, incomplete sync state is reported explicitly.
Use this signal to fail CI when remote evidence is missing.

## CI Usage

The harness has dedicated CI entry points.
Use the build lane, contract lane, and replay lane together.
Keep harness contract tests green for framework stability.

```bash
just ci-harness-build
just ci-harness-contract
just ci-harness-replay
```

These commands compile the harness, run contract tests, and validate replay flow.
The contract suite covers PTY control, SSH lifecycle behavior, replay behavior, and artifact generation.
Use this set as the minimum harness gate for runtime-harness changes.

## Troubleshooting

If validation fails before launch, run `harness-lint` first.
Fix schema or semantic errors before retrying runtime execution.
Most startup failures are configuration errors, not runtime errors.

If a step times out, inspect `timeout_diagnostics.json` and `events.json`.
Compare raw and normalized screens when assertions depend on volatile text.
Use replay to confirm if the failure is deterministic.

If mixed topology runs fail, inspect `routing_metadata.json` and `remote_artifact_sync.json`.
Confirm tunnel rewrites and remote source paths are correct.
Confirm host key and fingerprint requirements match the configured SSH policy.

## Related Docs

See [Simulation Infrastructure Reference](118_simulator.md) for simulator architecture and deterministic protocol testing.
See [Testing Guide](805_testing_guide.md) for testing patterns and fixture guidance.
