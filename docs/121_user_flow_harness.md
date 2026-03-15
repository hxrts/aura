# User Flow Harness

This document defines the harness contract for parity-critical user flows. It supplements [Testing Guide](804_testing_guide.md). Crate placement follows [Project Structure](999_project_structure.md).

## 1. Purpose

`aura-harness` is the multi-instance orchestration crate for end-to-end Aura validation. It starts local, browser, and SSH-backed instances. It runs shared scenarios against real frontends instead of renderer-specific scripts.

The default correctness lane is the real runtime with real TUI or web surfaces. The harness is not a replacement for simulator, Quint, or unit-level validation. Those systems provide supporting evidence and alternate execution environments.

## 2. Execution Lanes

The harness has two execution lanes.

- The shared semantic lane submits typed intent commands and waits on typed semantic contracts.
- The frontend-conformance lane validates renderer-specific wiring such as PTY keys, DOM selectors, focus movement, and control bindings.

Shared scenarios must run in the shared semantic lane. That lane must not issue raw UI requests such as `SendKeys`, `SendKey`, `ClickButton`, `FillInput`, or `FillField`. Frontend-conformance coverage may use those mechanics on purpose.

## 3. Scenario Sources

The scenario source of truth is `aura-app::scenario_contract`. Inventoried harness scenarios load and execute as semantic definitions. Shared scenarios keep typed semantic steps and do not carry mirrored frontend-conformance execution step graphs.

Frontend-conformance scenarios may still use typed `UiAction` mechanics such as key presses, text input, and modal dismissal. They are semantic files too. Renderer-specific intent belongs in the scenario definition, not in a second file format.

Semantic scenarios must not declare `execution_mode`. Compatibility-only executor fixtures must declare `execution_mode = "compatibility"` or `execution_mode = "agent"` explicitly. The harness no longer treats a missing mode as an implicit compatibility fallback.

Compatibility-only fixtures are renderer-mechanic coverage only. They must not encode product semantic intents such as account creation, contact acceptance, channel joins, or chat sends as compatibility actions.

Shared scenario governance depends on `scenarios/harness_inventory.toml`. The inventory classifies scenarios as shared, TUI conformance, or web conformance. Governance checks use that classification directly to enforce lane policy.

## 4. Backend Model

All backends implement `InstanceBackend`. That trait covers lifecycle, health checks, snapshots, log tails, and basic input. `RawUiBackend` adds renderer-driven actions for conformance coverage.

`LocalPtyBackend` and `PlaywrightBrowserBackend` also implement `SharedSemanticBackend`. They expose `shared_projection()`, `submit_semantic_command()`, and projection-event waits. Shared commands enter the real frontend update path instead of a harness-only shortcut.

`SshTunnelBackend` is orchestration-only. It validates SSH security defaults and tunnel setup. It does not implement the raw UI or shared semantic contracts.

## 5. Observation Model

`UiSnapshot` is the authoritative observation surface for parity-critical flows. Each snapshot carries `ProjectionRevision`, quiescence state, selections, lists, operations, toasts, and runtime events. Browser observation also carries render-heartbeat data through the browser bridge.

Parity-critical waits must bind to typed contracts. Those contracts include readiness, screen or modal visibility, runtime events, quiescence, operation handles, and strictly newer projections. Raw text matching and raw DOM scraping are diagnostics only.

Observation paths must be side-effect free. Reads must not repair state or retry hidden actions. Recovery remains explicit and separate from observation.

## 6. Semantic Command Plane

Shared commands are typed `IntentAction` requests. Examples include account creation, device enrollment, contact invitations, channel membership, and chat sends. Each command returns a typed response with submission metadata and an operation handle when the contract defines one.

The executor records projection baselines before command submission. Post-action waits require a strictly newer authoritative projection or another declared barrier. This rule prevents stale snapshots from satisfying success-path assertions.

Unsupported semantic commands must fail closed. The harness must not silently fall back to renderer-specific behavior in the shared semantic lane.

## 7. Scenario Execution

`ScenarioExecutor` enforces per-step budgets and an optional global budget. It records canonical trace events, state transitions, and step metrics. It also enforces shared-flow preconditions for account, contact, channel, and messaging phases.

Shared scenarios must declare convergence barriers before the next typed intent when the flow requires one. Governance validates those barriers against typed expectations. This keeps shared execution aligned with runtime events and authoritative state changes.

The executor also runs frontend-conformance scenarios through the semantic step model. Those runs still produce traces and diagnostics. They are not the primary parity oracle for shared business flows.

## 8. Determinism And Replay

Run configuration validation is config-first. Invalid run files, scenario files, capability mismatches, storage collisions, SSH policy violations, and browser runtime gaps fail before execution starts.

Determinism comes from `build_seed_bundle()`. The harness derives a run seed, scenario seed, fault seed, and per-instance seeds from the run configuration. The event stream uses monotonically increasing event identifiers.

Replay bundles store the run config, tool API version, tool action log, routing metadata, and seed bundle. Replay checks response shape compatibility and reruns the recorded actions against a fresh coordinator. Deterministic shared flows are expected to preserve semantic trace shape under identical inputs.

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
