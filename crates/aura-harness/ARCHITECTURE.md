# Aura Harness (Tooling)

## Purpose

Multi-instance orchestration harness for Aura runtime testing and operator workflows. Coordinates local PTY and SSH-backed instances, exposes a typed tool API, executes semantic scenarios against real frontends through a typed semantic command plane, and produces replay and artifact bundles.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Multi-instance orchestration and routing | Effect trait definitions or domain semantics |
| Typed semantic command-plane submission | Protocol safety rules |
| Replay bundle validation and deterministic seeds | Direct protocol mutation |
| Frontend-conformance coverage (quarantined) | Simulator-specific runner logic |
| Preflight capability and environment checks | Runtime-owned caches or coordinator state |
| Artifact bundles for CI and debugging | Parity-critical semantic lifecycle authorship |

## Dependencies

| Direction | Crate | What is consumed / produced |
|-----------|-------|-----------------------------|
| Consumes | `aura-app` | Shared scenario contract, semantic command types |
| Consumes | `aura-core` | Types, identifiers |
| Produces | — | Tool API responses, scenario reports, replay bundles, artifact bundles |

## Key Modules

- `config.rs` — Schema parsing for run config, semantic scenarios, and compatibility-only executor fixtures.
- `compatibility_step.rs` — Internal compatibility IR for frontend-conformance execution; limited to renderer-mechanic primitives.
- `coordinator.rs` — Multi-instance orchestration and per-instance command routing.
- `tool_api.rs` — Versioned request and typed response surface used by tests and automation.
- `executor.rs` — Semantic and compatibility scenario execution with deterministic budgets.
- `replay.rs` — Replay bundle validation and typed response conformance.
- `preflight.rs` — Capability, binary, storage, port, and SSH baseline checks plus semantic-lane admission.
- `backend/` — Local PTY and SSH backend adapters.

## Invariants

- Config-first execution: invalid run or scenario configs fail before instance startup.
- Instance isolation: each action is scoped by `instance_id` with unique `data_dir`.
- Deterministic seeds: identical run config and seed produce identical seed bundles.
- API compatibility: negotiation selects the highest shared tool API version or fails closed.
- Monotonic event identifiers: event stream IDs strictly increase and preserve append-only ordering.
- Bounded execution: step and global scenario budgets cap execution time with diagnostic timeouts.
- Secure SSH defaults: strict host key checking stays enabled with enforced fingerprint policy.
- Primary-lane policy: default lane targets real Aura runtime and real TUI/web surfaces.
- Semantic-command-plane execution: shared scenarios submit typed semantic commands and await typed readiness/handle/quiescence/projection contracts.
- Frontend-conformance isolation: renderer-specific mechanics are conformance-only and must not be the primary execution substrate for shared flows.
- Explicit lane capability contract: shared-semantic, raw-UI, and diagnostic-observation access are declared separately at preflight time; SSH is diagnostic-only until it implements the shared semantic contract.
- The harness is an `Observed` plus orchestration crate. It may submit commands, wait on typed handles/readiness, and read projections, but it must not author semantic lifecycle truth.
- Shared semantic execution must not keep a duplicate lifecycle graph, phase cache, or heuristic identifier repair path for accounts, contacts, channels, or messaging state.
- Parity-critical create/join/accept shared-channel flows must receive canonical operation handles and channel bindings from the authoritative submission/receipt path; post-hoc polling repair is forbidden.
- Raw renderer capture is diagnostic-only and is exposed through explicitly named `diagnostic_*` observation surfaces; typed `UiSnapshot` / `UiSnapshotEvent` remain the only authoritative shared-semantic observation plane, and diagnostic query APIs keep the `diagnostic_*` naming through the tool boundary.
- Time-bounded loops in shared semantic code are allowed only for infrastructure readiness, transport, or bounded observation waits whose owner is explicit; ownership transfer itself must not depend on settle windows or heuristic polling.
- Do not add backwards-compatibility, migration, fallback, or legacy code paths for removed shared-semantic harness behavior. Delete obsolete paths instead.

### InvariantHarnessDeterministicReplayInputs

Harness replay depends on validated config, negotiated API version, and deterministic seed bundles.

Enforcement locus:
- `src/replay.rs` validates schema and tool API compatibility before replay.
- `src/determinism.rs` derives stable seeds from run configuration.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- `just test-crate aura-harness` and `just check-arch`

Contract alignment:
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines reproducibility assumptions for testing.
- [Project Structure](../../docs/999_project_structure.md#invariant-traceability) defines canonical naming.

### InvariantSharedFlowExecutionIsSemantic

Shared harness scenarios remain portable across TUI and web because they target the shared semantic contract rather than frontend mechanics.

Enforcement locus:
- `config.rs` parses the shared scenario contract from `aura-app`.
- `executor.rs` submits typed semantic commands, tracks handles/readiness, and waits on authoritative projection changes.
- `backend/` implements the shared semantic command-plane bridge per frontend.
- Policy checks reject raw mechanics in the core shared scenario set.

Failure mode:
- Scenarios become backend-specific and require per-frontend special-casing.
- Browser/TUI parity regresses and core scenarios stop being reusable.

Verification hooks:
- `bash scripts/check/shared-flow-policy.sh`
- `cargo test -p aura-harness --lib tui_semantic_actions_emit_expected_tool_requests`
- `cargo test -p aura-harness --lib browser_driver_maps_shared_controls_to_selectors`

### InvariantSharedSemanticLaneIsNotRendererDriven

The main shared-flow lane debugs production workflows through semantic command submission and authoritative projections rather than incidental frontend I/O.

Enforcement locus:
- Shared semantic backend traits expose submit/observe/projection surfaces.
- Frontend-conformance-only helpers are isolated from shared semantic execution.
- CI policy checks reject raw PTY/selector/text mechanics in shared-flow paths.

Failure mode:
- Harness failures become ambiguous mixes of product bugs and renderer I/O races.
- Shared matrix stability depends on focus, timing, or DOM/PTY structure.

Verification hooks:
- `bash scripts/check/shared-flow-policy.sh`
- `just ci-harness-matrix`

### InvariantObservationUsesAuthoritativeSemanticState

The harness observes semantic state first and uses DOM/text fallbacks only for debugging.

Enforcement locus:
- Browser backend consumes pushed `UiSnapshot` and `RenderHeartbeat` data.
- Tool API snapshot endpoints return structured shared-contract payloads.

Failure mode:
- Timeouts become ambiguous because the harness cannot distinguish semantic state drift from renderer drift.
- Observation reads mutate state or silently repair it, making failures non-deterministic.

Verification hooks:
- Playwright driver self-test
- Browser backend contract tests

## Ownership Model

Reference: [docs/122_ownership_model.md](../../docs/122_ownership_model.md)

For shared semantic flows, `aura-harness` uses `Observed` for typed projection reads, readiness waits, handle observation, and timeout diagnostics. Narrow `ActorOwned` orchestration is permitted for executor/coordinator processes that own multi-instance orchestration state. The harness must not author semantic lifecycle truth; it consumes typed move-owned handles/tokens but does not create or advance them outside approved command-plane surfaces.

### Inventory

| Path | Category | Authoritative owner | May mutate | Observe only |
|------|----------|---------------------|------------|--------------|
| Harness coordinator / multi-instance orchestration | `ActorOwned` | `HarnessCoordinator` and executor-owned orchestration state | coordinator/executor orchestration code | scenarios, CI, diagnostics |
| Shared semantic command submission results and handles | `Observed` over upstream `MoveOwned` handles | frontend/product command plane | backend adapters store handle references only | waits, diagnostics, replay |
| Readiness waits and projection reads | `Observed` | product readiness coordinators and authoritative facts | harness-local timeout/trace bookkeeping only | scenario authors, CI |
| Frontend-conformance raw mechanics | frontend-local only, never shared semantic ownership | frontend under test | conformance adapters only | shared semantic lane must not depend on them |

### Capability-Gated Points

- Shared semantic command-plane submission and handle propagation must consume the authoritative `aura-app` contract rather than inventing harness-local semantic ownership.
- Readiness waits and projection reads may track timeout/trace metadata locally, but may not mutate or repair product semantic truth.
- Frontend-conformance helpers are explicitly quarantined from the shared semantic lane and may not bypass typed command/observation surfaces.
- Weak identifiers may help diagnostics, but they may not be upgraded into authoritative channel/context bindings after handoff.
- Diagnostic screen and DOM captures may support failure attribution, but they may not satisfy shared-semantic success conditions or repair missing authoritative state.
- Remaining wall-clock bounds in shared semantic code must name their owner and infrastructure reason; they must not implement semantic migration or legacy fallback behavior.

### Verification Hooks

- `cargo check -p aura-harness`
- `cargo test -p aura-harness -- --nocapture`
- `just ci-actor-lifecycle`
- `just ci-move-semantics`
- `just ci-timeout-policy`
- `just ci-semantic-owner-awaits`
- `just ci-best-effort-side-effects`

## Contributor Guidance

- Treat `SharedSemanticBackend` plus `UiSnapshot` / `UiSnapshotEvent` as the primary shared-semantic contract. If you need raw screen or DOM data, the API and variable names must say `diagnostic`.
- When a parity-critical command result needs an operation handle, channel binding, or other owned token, require it in the immediate typed receipt. Do not add later polling, re-resolution, or inferred repair.
- If a cleanup removes an old harness path, delete it. Do not preserve it behind compatibility branches, migration helpers, or fallback adapters.

## Testing

### Strategy

Deterministic replay, semantic flow execution, and tool API correctness are the primary concerns. Replay now compares typed tool-response meaning rather than only `Ok`/`Error` shape. Tests are organized into `tests/phases/` for the phased harness evolution (phase 1 through 5), `tests/holepunch/` for NAT traversal tiers, and top-level contract tests.

### Commands

```
cargo test -p aura-harness
```

### Coverage matrix

| What breaks if wrong | Invariant | Test location | Status |
|---------------------|-----------|--------------|--------|
| Tool API maps wrong operation | DeterministicReplayInputs | `tests/phases/phase1_tool_api.rs` | Covered |
| State machine invalid transition | SharedFlowExecutionIsSemantic | `tests/phases/phase3_state_machine.rs` | Covered |
| Reliability under failure | — | `tests/phases/phase4_reliability.rs` | Covered |
| API negotiation wrong | — | `tests/phases/phase5_api_negotiation.rs` | Covered |
| Routing replay diverges | DeterministicReplayInputs | `tests/phases/phase2_routing_replay.rs` | Covered |
| Phase 1 regression re-introduced | — | `tests/phases/phase1_regression.rs` | Covered |
| Phase 2 regression re-introduced | — | `tests/phases/phase2_regression.rs` | Covered |
| Phase 5 regression re-introduced | — | `tests/phases/phase5_regression.rs` | Covered |
| Holepunch E2E patchbay broken | — | `tests/holepunch/holepunch_e2e_runtime_patchbay.rs` | Covered |
| Holepunch tier 2 broken | — | `tests/holepunch/holepunch_tier2_patchbay.rs` | Covered |
| Holepunch stress fails | — | `tests/holepunch/holepunch_tier3_stress.rs` | Covered |
| Local loopback contract fails | ObservationUsesAuthoritativeSemanticState | `tests/contract_local_loopback.rs` | Covered |
| Contract suite fails | SharedFlowExecutionIsSemantic | `tests/contract_suite.rs` | Covered |

## References

- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md)
- [Ownership Model](../../docs/122_ownership_model.md)
- [Testing Guide](../../docs/804_testing_guide.md)
- [Project Structure](../../docs/999_project_structure.md)
