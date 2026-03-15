# Aura Harness (Tooling) - Architecture and Invariants

## Purpose
Provide a multi-instance orchestration harness for Aura runtime testing and operator workflows.
The crate coordinates local PTY and SSH-backed instances, exposes a structured tool API, executes semantic scenarios against real frontends through a typed semantic command plane, and produces replay and artifact bundles.
By default it is intended to validate the real Aura runtime and real user interfaces, not to act as a simulator-specific runner.

## Inputs
- Run configuration files and semantic scenario files from the shared `aura-app` scenario contract.
- Instance backend configuration for local PTY and SSH tunnel modes.
- Tool API requests for semantic command submission, projection reads, readiness waits, lifecycle actions, and logs.
- Optional replay bundle payloads for deterministic re-execution.

## Outputs
- Startup summaries and negotiated tool API metadata.
- Structured tool API responses and action logs.
- Harness event streams with per-operation details.
- Scenario execution reports and transition traces.
- Replay bundles and replay outcomes.
- Preflight capability and environment reports.
- Artifact bundles for CI and debugging.
- Explicit separation between shared semantic-lane execution and frontend-conformance coverage.

## Key Modules
- `config.rs`: Schema parsing and translation from the shared semantic scenario contract into executor steps.
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
- Primary-lane policy: the default harness lane targets the real Aura runtime and real TUI/web surfaces; simulator-backed execution is an alternate deterministic lane, not the primary correctness oracle.
- Single-executor policy: real frontend execution flows through `aura-harness`; Quint and simulator feed semantic traces and runtime conditions, not direct UI-driving logic.
- Semantic-first execution: core shared scenarios are expressed in semantic
  actions and typed ids rather than raw selectors, raw keypresses, or label
  matching.
- Semantic-command-plane execution: shared scenarios submit typed semantic
  commands to frontend bridges and await typed readiness, handle, quiescence,
  runtime-event, or projection contracts rather than driving renderer I/O.
- Frontend-conformance isolation: PTY keys, selector clicks, focus-stepping,
  and other renderer-specific mechanics are conformance-only and must not be
  the primary execution substrate for shared flows.
- Semantic-first observation: structured `UiSnapshot` and render-heartbeat data
  are authoritative; DOM/text fallbacks are diagnostics only and must not
  resolve parity-critical success-path observation.
- Parity-critical waits must resolve against documented readiness, event, or
  quiescence contracts rather than raw sleep/poll heuristics.
- Observation surfaces are read-only; recovery and retries remain explicit and
  separate from state reads.
- Parity-critical export and observation paths must not rely on placeholder
  identifiers, override caches, or heuristic success/event synthesis.
- The harness is an `Observed` plus orchestration crate for shared semantic
  flows. It may submit commands, wait on typed handles/readiness, and read
  projections, but it must not author semantic lifecycle truth.

## Ownership Model

For shared semantic flows, `aura-harness` should use:

- `Observed`
  - typed projection reads
  - typed readiness waits
  - handle observation
  - timeout diagnostics
- narrow `ActorOwned` orchestration only where necessary
  - executor/coordinator processes may own multi-instance orchestration state

It must not use:

- harness-local semantic ownership for parity-critical operations
- fallback mutation or repair of semantic state during observation
- raw renderer I/O as a substitute for missing semantic ownership

The correct split is:

- the harness may own orchestration of test instances and wait logic
- authoritative operation lifecycle and readiness remain in product
  coordinators
- the harness consumes typed move-owned handles/tokens but does not create or
  advance them outside approved command-plane surfaces

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
- [Project Structure](../../docs/999_project_structure.md#invariant-traceability) defines canonical naming.

### InvariantSharedFlowExecutionIsSemantic
Shared harness scenarios remain portable across TUI and web because they target
the shared semantic contract and shared command plane rather than frontend
mechanics.

Enforcement locus:
- `config.rs` parses the shared scenario contract from `aura-app`.
- `executor.rs` submits typed semantic commands, tracks handles/readiness, and
  waits on authoritative projection changes.
- `backend/` implements the shared semantic command-plane bridge per frontend.
- policy checks reject raw mechanics in the core shared scenario set.

Failure mode:
- Scenarios become backend-specific and require per-frontend special-casing.
- Browser/TUI parity regresses and core scenarios stop being reusable.
- Product debugging gets coupled to renderer I/O timing and harness-specific
  choreography bugs.

Verification hooks:
- `bash scripts/check/shared-flow-policy.sh`
- `cargo test -p aura-harness --lib tui_semantic_actions_emit_expected_tool_requests`
- `cargo test -p aura-harness --lib browser_driver_maps_shared_controls_to_selectors`

### InvariantSharedSemanticLaneIsNotRendererDriven
The main shared-flow lane debugs production workflows through semantic command
submission and authoritative projections rather than incidental frontend I/O.

Enforcement locus:
- shared semantic backend traits expose submit/observe/projection surfaces.
- frontend-conformance-only helpers are isolated from shared semantic execution.
- CI policy checks reject raw PTY/selector/text mechanics in shared-flow paths.

Failure mode:
- harness failures become ambiguous mixes of product bugs and renderer I/O
  races.
- shared matrix stability depends on focus, timing, or DOM/PTY structure.

Verification hooks:
- `bash scripts/check/shared-flow-policy.sh`
- `just ci-harness-matrix`

### InvariantObservationUsesAuthoritativeSemanticState
The harness observes semantic state first and uses DOM/text fallbacks only for
debugging.

Enforcement locus:
- browser backend consumes pushed `UiSnapshot` and `RenderHeartbeat` data.
- tool API snapshot endpoints return structured shared-contract payloads.

Failure mode:
- Timeouts become ambiguous because the harness cannot distinguish semantic
  state drift from renderer drift.
- Browser/TUI failures require ad hoc scraping and manual interpretation.
- Observation reads mutate state or silently repair it, making failures
  non-deterministic and hard to attribute.

Verification hooks:
- Playwright driver self-test
- browser backend contract tests
## Boundaries
- This crate is tooling and test infrastructure. It is not part of the runtime layer stack.
- It does not define Aura effect traits, domain semantics, or protocol safety rules.
- It drives instances through process boundaries and typed semantic/tool API
  surfaces rather than direct protocol mutation.
- Frontend-conformance coverage may still use renderer-specific I/O, but that
  coverage is explicitly separate from the shared semantic lane.
- It may use direct OS operations for orchestration, capture, and preflight checks by design.

## Migration State
- Canonical direction: shared flows are expressed as semantic scenarios from `aura-app`, then executed through `aura-harness`.
- High-value migrated baseline scenarios:
  - `scenarios/harness/semantic-observation-tui-smoke.toml`
  - `scenarios/harness/semantic-observation-browser-smoke.toml`
  - `scenarios/harness/real-runtime-mixed-startup-smoke.toml`
- Direct Quint-to-TUI execution paths have been removed. Quint now emits semantic traces rather than frontend-driving scripts.
- Remaining legacy harness scenarios under `scenarios/harness/` are repository corpus pending full semantic conversion. They are audited by `just harness-migration-audit` and are not part of the supported runner input surface anymore.

### Migration Sequence
1. Define shared semantic contracts in `aura-app`.
2. Route all real frontend execution through `aura-harness`.
3. Convert high-value smoke and CI scenarios first.
4. Inventory the remaining legacy scenario corpus and migrate shared flows in priority order.
5. Remove or fail policy checks on any remaining parallel execution paths or legacy scenario dialects.
