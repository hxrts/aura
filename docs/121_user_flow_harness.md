# User Flow Harness

This document defines the harness contract for parity-critical user flows. It supplements [Testing Guide](804_testing_guide.md). Crate placement follows [Project Structure](999_project_structure.md).

## 1. Purpose

`aura-harness` is the multi-instance orchestration crate for end-to-end Aura validation. It starts local, browser, and SSH-backed instances. It runs shared scenarios against real frontends instead of renderer-specific scripts.

The default correctness lane is the real runtime with real TUI or web surfaces. The harness is not a replacement for simulator, Quint, or unit-level validation. Those systems provide supporting evidence and alternate execution environments.

## 2. Execution Lanes

The harness defines two execution lanes:

- **Shared semantic lane**: submits typed intent commands and waits on typed semantic contracts.
- **Frontend-conformance lane**: validates renderer-specific wiring such as PTY keys, DOM selectors, focus movement, and control bindings.

See [Testing Guide](804_testing_guide.md) for lane selection and execution details.

## 3. Scenario Sources

The scenario source of truth is `aura-app::scenario_contract`. Scenario taxonomy:

- **Shared semantic scenarios**: typed semantic steps, no `execution_mode` declaration, no renderer-specific mechanics.
- **Frontend-conformance scenarios**: typed `UiAction` mechanics (key presses, text input, modal dismissal), must declare `execution_mode` explicitly.
- **Compatibility fixtures**: quarantined renderer-mechanic coverage only, must declare `execution_mode = "compatibility"` or `execution_mode = "agent"`.

Inventoried scenarios are classified in `scenarios/harness_inventory.toml` as shared, TUI conformance, or web conformance.

See [Testing Guide](804_testing_guide.md) for scenario authoring and governance.

## 4. Backend Model

The harness defines the following backend interfaces:

- **`InstanceBackend`**: lifecycle, health checks, snapshots, log tails, and basic input.
- **`RawUiBackend`**: renderer-driven actions for conformance coverage.
- **`SharedSemanticBackend`**: `shared_projection()`, `submit_semantic_command()`, and projection-event waits. Implemented by `LocalPtyBackend` and `PlaywrightBrowserBackend`.
- **`SshTunnelBackend`**: orchestration-only (SSH security defaults and tunnel setup).

See [Testing Guide](804_testing_guide.md) for backend implementation.

## 5. Observation Model

`UiSnapshot` is the authoritative observation surface for parity-critical flows. Observation contracts:

- Snapshots carry `ProjectionRevision`, quiescence state, selections, lists, operations, toasts, and runtime events.
- Parity-critical waits bind to typed contracts (readiness, visibility, events, quiescence, operation handles, strictly newer projections). Raw text matching and DOM scraping are diagnostics only.
- Observation paths are side-effect free. Reads do not repair state or retry hidden actions.

See [Testing Guide](804_testing_guide.md) for observation patterns.

## 6. Semantic Command Plane

Shared commands are typed `IntentAction` requests (account creation, device enrollment, contact invitations, channel membership, chat sends). Contracts:

- Each command returns a typed response with submission metadata and an optional operation handle.
- Post-action waits require a strictly newer authoritative projection or another declared barrier.
- Unsupported semantic commands fail closed. No silent fallback to renderer-specific behavior.

See [Testing Guide](804_testing_guide.md) for semantic command usage.

## 7. Scenario Execution

`ScenarioExecutor` enforces per-step budgets and an optional global budget. Contracts:

- Shared scenarios must declare convergence barriers before the next typed intent when the flow requires one.
- The executor records canonical trace events, state transitions, and step metrics.
- Frontend-conformance scenarios produce traces and diagnostics but are not the primary parity oracle for shared business flows.

See [Testing Guide](804_testing_guide.md) for scenario execution details.

## 8. Determinism And Replay

Determinism contracts:

- Configuration validation is config-first: invalid inputs fail before execution starts.
- Determinism derives from `build_seed_bundle()` (run seed, scenario seed, fault seed, per-instance seeds). Event streams use monotonically increasing identifiers.
- Replay bundles store run config, tool API version, action log, routing metadata, and seed bundle. Deterministic shared flows preserve semantic trace shape under identical inputs.

See [Testing Guide](804_testing_guide.md) for determinism and replay details.

## 9. Runtime Substrate

The harness supports `real` and `simulator` runtime substrates. The real substrate is the default lane. The simulator substrate is an alternate deterministic runtime controller for fault injection and transcript capture.

Simulator substrate runs currently support local instances only. Browser instances are not allowed in simulator mode. Shared user-flow correctness still belongs to the real frontend lane even when simulator support is enabled for controlled experiments.

## 10. Governance And Policy

Harness governance is typed first. `aura-harness` exposes governance checks for shared scenario contracts, scenario-shape enforcement, barrier legality, user-flow coverage, UI parity metadata, and wrapper integrity.

The main repository policy entry points are `scripts/check/shared-flow-policy.sh`, `scripts/check/user-flow-policy-guardrails.sh`, and `scripts/check/user-flow-guidance-sync.sh`. The corresponding aggregate commands are `just ci-shared-flow-policy`, `just ci-user-flow-policy`, and `just ci-harness-matrix-inventory`.

Harness mode may add instrumentation and render-stability hooks. It must not change parity-critical business semantics. Allowlisted exceptions must carry owner, justification, and design-note metadata.

## 11. Boundaries

`aura-harness` is tooling. It is not the authority for domain semantics, effect traits, or protocol safety rules. Those contracts remain owned by `aura-core`, `aura-app`, and the other runtime and specification crates.

The harness drives instances through process boundaries and typed tool surfaces. It must not mutate protocol state out of band. Shared UX identifiers, parity metadata, and observation shapes remain owned by `aura-app::ui_contract`.
