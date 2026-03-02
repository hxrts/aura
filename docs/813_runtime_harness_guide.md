# Runtime Harness Guide

## Overview

The runtime harness executes one or more real Aura instances in PTYs.
It provides deterministic control over input, screen capture, restart, and replay.
Use it for end-to-end runtime validation, UI-level evidence, and replay bundles.
The harness complements the [simulator](118_simulator.md), which remains primary for protocol correctness.
It supports local and SSH-backed instances in the same run config.

The harness supports two execution modes:

1. **Scripted mode** runs predefined steps from a scenario file. Each step specifies an action, target instance, and timeout. Use scripted mode for regression testing and CI gates where the exact sequence is known.

2. **Agent mode** lets an LLM drive execution toward high-level goals. The harness exposes a tool API and the agent decides what actions to take based on screen state and constraints. Use agent mode for exploratory debugging and validating complex user flows.

## Prerequisites

Run commands from the repository root inside `nix develop`.
Use unique `data_dir` paths per local instance.

The harness uses two TOML file types:

1. Run configs define instance topology and resource limits
2. Scenario files define execution mode and steps (scripted) or goals (agent)

Baseline configs and scenarios live in `configs/harness/` and `scenarios/harness/`.

## Run Config

The run config defines instance topology and runtime limits.

```toml
schema_version = 1  # required

[run]
name = "local-loopback-smoke"
pty_rows = 40
pty_cols = 120
seed = 4242
max_cpu_percent = 95
require_remote_artifact_sync = false

[[instances]]
id = "alice"                       # must be unique per instance
mode = "local"
data_dir = ".tmp/harness/alice"
device_id = "alice-dev-01"
bind_address = "127.0.0.1:41001"   # must be unique per instance
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

```toml
schema_version = 1  # required
id = "mixed-topology-smoke"
goal = "Exercise local plus ssh-dry-run topology with state-machine execution."
execution_mode = "scripted"  # "scripted" or "agent"

[[steps]]
id = "launch"
action = "launch_instances"
timeout_ms = 5000

[[steps]]
id = "local-send"
action = "send_keys"
instance = "alice"
keys = "mixed-topology-msg\n"  # key payload to send
timeout_ms = 2000

[[steps]]
id = "local-wait"
action = "wait_for"
instance = "alice"
pattern = "mixed-topology-msg"   # required match pattern
timeout_ms = 2000

[[steps]]
id = "join-channel"
action = "send_chat_command"
instance = "alice"
command = "join slash-lab"
timeout_ms = 2000
```

Scenario step actions are now a typed enum at parse time.
Unknown action names fail TOML parsing before lint or execution.
`expect` still works for backward compatibility, but prefer action-specific fields:
`keys`, `pattern`, `command`, `key`, `source_instance`, and typed assertion fields.
`send_chat_command` now sends `Esc`, switches to Chat (`2`), performs a best-effort chat-pane wait, then types `i/<command>`.
This reduces cross-screen command flakiness without extra scenario boilerplate.

Use `request_id` to enforce strict scenario ordering.
Each present `request_id` must be strictly greater than prior `request_id`.

Typed action fields:

| Action | Preferred fields |
| --- | --- |
| `send_keys` | `keys` |
| `send_chat_command` | `command` |
| `wait_for` | `pattern` |
| `send_key` | `key`, `repeat` |
| `send_clipboard` | `source_instance` |
| `set_var` | `var`, `value` |
| `extract_var` | `var`, `regex`, `group`, `from` |
| `expect_toast` | `contains`, `level` |
| `expect_command_result` | `contains`, `level`, `consistency` |
| `expect_membership` | `channel`, `present`, `selected` |
| `expect_denied` | `reason`, `contains`, `contains_any` |
| `get_authority_id` | `var` |
| `list_channels` | optional `var` |
| `current_selection` | optional `var` |
| `list_contacts` | optional `var` |

## Slash Command Consistency

Slash commands now execute through a typed strong-command pipeline:
`ParsedCommand` -> `ResolvedCommand` -> `CommandPlan` -> `execute_planned`.

Completion semantics are explicit:

| Command class | Commands | Required consistency |
| --- | --- | --- |
| Membership | `/join`, `/leave`, `/part`, `/quit`, `/j` | `replicated` |
| Moderation | `/kick`, `/ban`, `/unban`, `/mute`, `/unmute` | `enforced` |
| Steward | `/op`, `/deop`, `/mode` | `enforced` |
| Other | `/msg`, `/me`, `/nick`, `/who`, `/whois`, `/topic`, `/invite`, `/pin`, `/unpin`, `/homeinvite`, `/homeaccept`, `/neighborhood`, `/nhadd`, `/nhlink` | `accepted` |

When a required stage is not reached before timeout, the command returns an explicit error instead of a success toast.
Harness waits should match the staged labels (`accepted`, `replicated`, `enforced`) or the explicit timeout error text.

## Common Commands

Use lint before long runs.
Then execute the run.
Use replay for deterministic reruns.

```bash
just harness-lint -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/local-discovery-smoke.toml
```

This command validates run and scenario semantics.
It fails on schema errors and unknown instances.
Invalid step action names fail earlier during TOML parsing.

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

```bash
just scenario3-e2e
```

This command generates an isolated temporary run config, lints `scenarios/harness/scenario3-irc-slash-commands-e2e.toml`, and runs it end to end.

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

Use `--prelude` to run a scripted scenario before entering interactive control.
This is the fastest way to automate repeatable setup and keep manual validation for variable parts.

```bash
cargo run -p aura-harness --bin tool_repl -- \
  --config configs/harness/local-loopback.toml \
  --prelude scenarios/harness/<your-prelude>.toml
```

Use fresh per-run data directories in the run config (or a temporary generated config) so prelude account setup is deterministic.

When the prelude completes, the REPL stays active with the same instances and data directories.
The process emits `prelude_complete scenario_id=<id>` to stderr before accepting JSON requests.

`tool_repl` enforces an idle timeout by default (`--idle-timeout-ms 600000`).
If no requests arrive before the timeout, it automatically stops all instances and exits.
Set `--idle-timeout-ms 0` to disable idle shutdown.
Use `--require-request-id --strict-request-id-order` to enforce ordered request envelopes.

```json
{"id":1,"method":"negotiate","params":{"client_versions":["1.0","0.2"]}}
{"id":2,"method":"screen","params":{"instance_id":"alice"}}
{"id":3,"method":"send_keys","params":{"instance_id":"alice","keys":"3n"}}
{"id":4,"method":"wait_for","params":{"instance_id":"alice","pattern":"Create Invitation","timeout_ms":4000}}
```

Send one JSON request per line.
The current supported API versions are `1.0`, `0.2`, and `0.1`.
`wait_for` matches against normalized screen text.
When an input request includes `id`, responses include the same `id`.
`get_authority_id` first parses the active screen and then falls back to local secure-store state for local instances.

```json
{"id":5,"method":"send_key","params":{"instance_id":"alice","key":"enter"}}
{"id":6,"method":"get_authority_id","params":{"instance_id":"alice"}}
{"id":7,"method":"list_channels","params":{"instance_id":"alice"}}
{"id":8,"method":"current_selection","params":{"instance_id":"alice"}}
{"id":9,"method":"list_contacts","params":{"instance_id":"alice"}}
```

Use `send_key` for named keys such as `enter`, `esc`, `tab`, and arrows.
Use introspection methods to avoid brittle screen scraping in interactive runs.
Use these typed methods instead of ad-hoc external parsing scripts.
`get_authority_id` responses include a `source` field (`screen` or `local_state`).

```json
{"id":10,"method":"tail_log","params":{"instance_id":"alice","lines":50}}
{"id":11,"method":"restart","params":{"instance_id":"bob"}}
{"id":12,"method":"kill","params":{"instance_id":"bob"}}
```

Use `tail_log` for runtime diagnostics.
Set `log_path` in the run config if you need non-empty `tail_log` output.

## Manual Invitation and Chat Flow

The harness supports a complete manual invitation and chat flow.
Use two instances and deterministic message tokens.
Capture evidence after each phase.

The flow proceeds in three phases. In phase one, create an invitation on Alice in Contacts. Press `c` on the invitation modal to copy the full code. Import the code on Bob and confirm that Contacts shows one entry on both sides.

In phase two, create a channel on Alice. Verify that member selection shows one selected before continuing. Confirm that Channels shows one entry on Bob.

In phase three, exchange messages using tokens `msg-a-1`, `msg-b-1`, `msg-a-2`, and `msg-b-2`. Confirm each token appears on both screens before sending the next token.

Harness local instances now default to clipboard isolation:

- `AURA_CLIPBOARD_MODE=file_only`
- `AURA_CLIPBOARD_FILE=<data_dir>/.harness-clipboard.txt`

That default prevents harness-driven copy actions from writing to your system clipboard.

To customize paths or override behavior, set per-instance clipboard env values:

- `AURA_CLIPBOARD_MODE=file_only`
- `AURA_CLIPBOARD_FILE=<instance-specific path>`

With `file_only`, TUI copy actions never write to the system clipboard. Read the configured file after pressing `c` to get the out-of-band payload.

```toml
[[instances]]
id = "alice"
env = [
  "AURA_CLIPBOARD_MODE=file_only",
  "AURA_CLIPBOARD_FILE=.tmp/harness/alice-clipboard.txt",
]

[[instances]]
id = "bob"
env = [
  "AURA_CLIPBOARD_MODE=file_only",
  "AURA_CLIPBOARD_FILE=.tmp/harness/bob-clipboard.txt",
]
```

## Artifacts

Each `run` writes a machine-readable bundle.
The default root is `artifacts/harness/<run-name>/`.
Use this directory as the primary debugging source.

The bundle includes `startup_summary.json`, `preflight_report.json`, `events.json`, and `initial_screens.json`. It also includes `replay_bundle.json`, `seed_bundle.json`, `resource_report.json`, and `remote_artifact_sync.json`. When a scenario is provided, `scenario_report.json` is written. When scenario execution times out, `timeout_diagnostics.json` is written.

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

If you observe many `aura tui` processes, inspect parent `tool_repl` sessions first.
Use process listings to identify long-idle REPL parents and terminate those parents.
With current lifecycle handling, stopping a `tool_repl` parent stops the instances it owns.

## Related Docs

See [Simulation Infrastructure Reference](118_simulator.md) for simulator architecture.
See [Testing Guide](805_testing_guide.md) for testing patterns and fixture guidance.
