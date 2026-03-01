# Aura Harness (Tooling) - Architecture and Invariants

## Purpose
Provide a multi-instance orchestration harness for Aura runtime testing and operator workflows.
The crate coordinates local PTY and SSH-backed instances, exposes a structured tool API, runs scripted scenarios, and produces replay and artifact bundles.

## Inputs
- Run configuration and scenario configuration files.
- Instance backend configuration for local PTY and SSH tunnel modes.
- Tool API requests for screen capture, input injection, waits, lifecycle actions, and logs.
- Optional replay bundle payloads for deterministic re-execution.

## Outputs
- Startup summaries and negotiated tool API metadata.
- Structured tool API responses and action logs.
- Harness event streams with per-operation details.
- Scenario execution reports and transition traces.
- Replay bundles and replay outcomes.
- Preflight capability and environment reports.
- Artifact bundles for CI and debugging.

## Key Modules
- `config.rs`: Schema parsing and semantic validation for run and scenario inputs.
- `coordinator.rs`: Multi-instance orchestration and per-instance command routing.
- `tool_api.rs`: Versioned request and response surface used by tests and automation.
- `executor.rs`: Scenario state machine execution with deterministic budgets.
- `replay.rs`: Replay bundle validation and shape-based response conformance.
- `preflight.rs`: Capability, binary, storage, port, and SSH baseline checks.
- `backend/`: Local PTY and SSH backend adapters.

## Invariants
- Config-first execution: invalid run or scenario configs fail before instance startup.
- Instance isolation: each action is scoped by `instance_id` and local `data_dir` values are unique.
- Deterministic seeds: identical run config and seed produce identical seed bundles.
- API compatibility: negotiation selects the highest shared tool API version or fails closed.
- Monotonic event identifiers: event stream IDs strictly increase and preserve append-only ordering.
- Bounded execution: step and global scenario budgets cap execution time and fail with diagnostics on timeout.
- Secure SSH defaults: strict host key checking stays enabled and fingerprint policy is enforced when required.

### Detailed Specifications

### InvariantHarnessDeterministicReplayInputs
Harness replay depends on validated config, negotiated API version, and deterministic seed bundles.

Enforcement locus:
- src replay validates schema and tool API compatibility before replay.
- src determinism derives stable seeds from run configuration.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-harness and just check-arch

Contract alignment:
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines reproducibility assumptions for testing.
- [Aura System Invariants](../../docs/005_system_invariants.md) defines canonical naming.
## Boundaries
- This crate is tooling and test infrastructure. It is not part of the runtime layer stack.
- It does not define Aura effect traits, domain semantics, or protocol safety rules.
- It drives instances through process, PTY, and tool API surfaces rather than direct protocol mutation.
- It may use direct OS operations for orchestration, capture, and preflight checks by design.

