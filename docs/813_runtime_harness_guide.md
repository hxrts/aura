# Runtime Harness Guide

## Purpose

The runtime harness runs one or more real Aura instances and exposes deterministic control primitives.
It is an integration test system for terminal IO, runtime networking, and scenario execution.
It complements the simulator, which remains the primary protocol correctness engine.

See [Simulation Infrastructure Reference](118_simulator.md) for simulator scope and usage.

## Local run

Use a run config and a scenario config.
The run config defines instances, addresses, and storage roots.
The scenario config defines a high-level flow and assertions.

```bash
just harness-run -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/local-discovery-smoke.toml
```

This command validates the run config and scenario config.
It writes startup artifacts under `artifacts/harness/<run-name>/`.
It prints a structured startup summary for CI logs and local debugging.

## SSH run

Use a mixed topology config when one or more instances run on remote hosts.
Each SSH instance must declare `ssh_host` and `remote_workdir`.
Tunnel mappings remain explicit in the run config.

```bash
just harness-run -- --config configs/harness/local-plus-ssh.toml --scenario scenarios/harness/local-discovery-smoke.toml
```

This command uses the same scenario engine as local mode.
It validates SSH-specific fields before execution starts.
It keeps topology changes in TOML rather than code edits.

## Troubleshooting

Run lint before full execution when diagnosing config failures.
Lint performs schema and semantic checks and exits before process launch.
This reduces noise when topology or storage settings are invalid.

```bash
just harness-lint -- --config configs/harness/local-loopback.toml --scenario scenarios/harness/local-discovery-smoke.toml
```

Use `just harness-checkpoint` before creating checkpoint commits.
The helper runs lint and tests and writes a commit message template to `.git/HARNESS_COMMIT_MESSAGE.txt`.
This keeps checkpoint workflow consistent across phases.
