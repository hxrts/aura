# UX Workflow Testing Feedback And Remediation Plan

This document captures a manual review of the webapp, TUI, and runtime-harness UX workflow testing system.

Scope:
- Read-only review
- No app, harness, build, or test execution
- Focus on structural issues, flakiness, non-determinism, opacity, redundant code, and tech debt

How to use this document:
- The items are ordered by implementation dependency, not by severity alone.
- Each section includes remediation tasks, go-forward enforcement tasks, and success criteria.
- The goal is to make the harness simpler because the product surfaces are cleaner and more canonical, not because the harness keeps learning more exceptions.
- Foundational contract work should be specified once and then referenced by later sections; later sections should tighten ownership, tests, and enforcement rather than restating the same invariant in slightly different words.

## Priority order

1. Define one canonical shared UX contract and identifier model.
2. Give the TUI one authoritative semantic snapshot path.
3. Unify selection, focus, and action-precondition semantics across frontends.
4. Remove harness-only frontend behavior and hidden runtime shortcuts.
5. Integrate onboarding into the normal app state/publication model.
6. Make runtime events authoritative and strengthen parity.
7. Separate observation from recovery in the browser driver and harness.
8. Replace polling/sleeps in the TUI lane with event-driven observation.
9. Clarify and harden the scenario/config contract boundary.
10. Replace shell-script governance with typed Rust validation.
11. Consider a TypeScript migration for the Playwright driver after the architecture is cleaner.

## Foundational contract first

This plan assumes a single typed shared contract is defined before most downstream cleanup and CI policy work proceeds. That contract should be concrete enough that later tasks can point at it directly instead of inventing local interpretations.

The foundational shared contract should define, at minimum:
- action request shape
- action handle shape
- authoritative transition fact
- terminal success fact
- terminal failure fact
- projection revision or sequence metadata
- canonical trace event shape

This document should treat that contract as the source of truth for:
- shared-flow semantics
- quiescence and completion conditions
- stale-state detection
- trace conformance
- parity-critical identifier ownership

Without that schema in place, downstream CI checks, docs updates, and frontend/harness cleanup tasks should be treated as provisional.

## Scenario/config boundary

The plan should preserve the semantic split between scenarios and execution bindings.

The intended model is:
- shared scenarios stay actor-based and frontend-neutral
- scenario files define semantic flows and actor roles
- config, lane, and matrix layers bind actors to concrete frontend/runtime backends

The goal is not to collapse scenarios and configs into one surface. The goal is to make the boundary explicit, typed, and enforceable so shared scenarios do not smuggle frontend identity or lane-specific mechanics into the semantic layer.

## CI implementation conventions

The CI-touching tasks in this document should follow the repository's existing CI structure and naming conventions.

Unless a section explicitly says otherwise, every CI task below should be implemented using these rules:

- Fast static, lint, policy, and metadata checks belong in `scripts/check/<domain>.sh` with kebab-case filenames and `set -euo pipefail`.
- Heavier harness execution lanes belong in `scripts/ci/<domain>.sh`.
- Every new CI gate should have a stable `just ci-<domain>` wrapper in the `justfile`; GitHub workflows should invoke `nix develop --command just ci-<domain>` rather than duplicating logic inline.
- Cheap repository-wide checks should extend `.github/workflows/ci.yml`.
- Harness-specific checks and real-frontend lanes should extend `.github/workflows/harness.yml`.
- Docs-, metadata-, and semantic-drift-oriented checks should extend `.github/workflows/docs.yml` when the primary concern is keeping written guidance aligned.
- Deep semantic, differential, or property-style lanes should extend the existing deep workflow families instead of inventing a new category.
- Job names should follow the existing short Title Case style, and new jobs should use the existing failure-bundle pattern with a replay command pointing at the corresponding `just ci-...` entry point.
- Prefer extending an existing domain lane (`ci`, `harness`, `docs`, `conform`) over adding a new top-level workflow when the scope already fits one of them.

Reusable patterns:
- Static contract/policy gate:
  - implement in `scripts/check/<domain>.sh`
  - expose as `just ci-<domain>`
  - wire into `ci.yml`, `harness.yml`, or `docs.yml` based on ownership and cost
- Focused crate contract test:
  - keep the test in the owning crate
  - call it from `just ci-<domain>`
  - place it in the smallest matching workflow
- Heavy harness or matrix lane:
  - implement in `scripts/ci/<domain>.sh`
  - expose as `just ci-<domain>`
  - wire into `harness.yml`, usually under `schedule` and `workflow_dispatch` when expensive
- Docs/metadata drift gate:
  - implement in `scripts/check/<domain>.sh`
  - expose as `just ci-<domain>`
  - wire into `docs.yml` if the gate is primarily documentation-centric

These conventions are the implementation guidance for every CI-related task in this document.

## Determinism by construction

These are explicit cross-cutting tasks that should exist in addition to the cleanup items below. The goal is to make the UX flow testing system correct and deterministic by design, rather than merely less flaky in practice.

This section is the canonical home for the main determinism invariants. Later sections should reference these invariants when they talk about architecture, docs, or CI enforcement, rather than redefining them.

CI implementation note:
- For this section, keep cheap schema, lint, and metadata enforcement in `scripts/check/` and `ci.yml`.
- Put crate-level determinism and side-effect contract tests in the owning crates behind `just ci-...` recipes.
- Put repeated-run, real-frontend, and matrix-style determinism checks in `scripts/ci/` and `.github/workflows/harness.yml`.

### Tasks to fix

- [x] Define the foundational typed shared UX contract schema:
  - action request
  - action handle
  - authoritative transition fact
  - terminal success fact
  - terminal failure fact
  - projection revision or sequence metadata
  - canonical trace event
- [x] Define the scenario/config boundary in typed metadata and validation:
  - shared scenarios are actor-based and frontend-neutral
  - lane/config/matrix layers own frontend and runtime binding
  - shared scenarios may not encode frontend identity or lane-specific mechanics
- [x] Define a shared quiescence contract for both frontends.
- [x] Make every parity-critical state transition emit a monotonic sequence number or revision.
- [x] Route all time, randomness, and generated IDs used by parity-critical UX through deterministic effects in harness mode.
- [x] Define action contracts for every shared flow:
  - preconditions
  - transition event
  - terminal success state
  - terminal failure state
- [x] Require parity-critical observation APIs to be side-effect free.
- [x] Record a canonical action/event/state trace for each scenario.
- [x] Add cross-frontend conformance tests on traces, not only final snapshots.
- [x] Ban hidden retries and recovery inside product flows and observation APIs.
- [x] Add invariants for snapshot exporters.
- [x] Add scenario determinism tests at the harness layer.
- [x] Introduce a single owner for waits and barriers.
- [x] Fail loudly on ambiguity instead of returning synthetic or recovered success-oriented state.
- [x] Make browser snapshot freshness revision-based, not cache-reset-based.
- [x] Define a single browser snapshot cache owner and lifecycle.
- [x] Invalidate browser cache automatically on session-boundary transitions.
- [x] Require post-action browser observation to wait for newer state, not merely any cached snapshot.
- [x] Add stale-cache detection between browser render heartbeats and semantic snapshots.
- [x] Ban flow-specific browser cache invalidation logic in harness actions and frontend flows.
- [x] Add browser cache conformance tests for freshness and stale-state rejection.
- [x] Keep browser snapshot publication and caching bounded with coalescing and latest-state replacement.

### Tasks to enforce go-forward

- [x] Add CI validation that shared scenarios remain actor-based and frontend-neutral, and that frontend/runtime binding is declared only in the config or matrix layer.
- [x] Add a typed shared-flow metadata schema that fails CI if any new shared UX flow lacks:
  - semantic preconditions
  - authoritative transition event
  - deterministic completion condition
  - trace shape
- [x] Add CI tests that rerun selected deterministic scenarios multiple times with the same seed and compare semantic traces.
- [x] Add CI tests that fail if parity-critical observation paths read wall clock time, unseeded randomness, or non-deterministic ID generation.
- [x] Add CI checks that reject retries, sleeps, or recovery helpers in parity-critical code outside approved owner modules.
- [x] Add contract tests that observation endpoints are side-effect free.
- [x] Add exporter tests that reject placeholder IDs, inferred success events, contradictory focus/selection state, and impossible screen/modal combinations.
- [x] Add CI validation that any fallback path used for parity-critical behavior is registered in the shared contract metadata; otherwise it must fail diagnostically.
- [x] Add CI checks that reject new browser cache-reset logic outside the single cache owner module.
- [x] Add contract tests that browser post-action polling must observe a strictly newer snapshot or heartbeat revision than the pre-action baseline.
- [x] Add CI checks that browser cache lifecycle boundaries are declared in shared metadata or architecture docs when authority/device/session semantics change.

### Success criteria

- Every parity-critical wait resolves against a documented quiescence or completion condition, not sleeps, redraw stability, or heuristic idle detection.
- The harness can prove it observed newer state via revisions or sequence numbers and does not need to guess whether a snapshot is stale.
- Parity-critical flows do not depend on wall clock timing, scheduler luck, random IDs, or hidden recovery behavior.
- Scenario failures can be diagnosed from canonical traces rather than only final-state diffs.
- Re-running the same deterministic scenario with the same seed produces the same semantic trace.
- Browser snapshot freshness is proven by monotonic revision or generation advancement, not by manually clearing driver caches inside individual flows.
- Browser cache invalidation is owned by one lifecycle-aware module and happens only at declared session-boundary transitions.
- Mutating browser actions cannot complete against stale onboarding or pre-submit state because post-action observation requires newer heartbeat/snapshot revisions.
- Browser snapshot publication remains bounded under render churn and cannot build an unbounded stale-publication backlog.

## Documentation and agent guidance updates

These updates should happen alongside the implementation work so the project’s written guidance matches the actual system and prevents regression through process drift.

CI implementation note:
- Documentation and metadata drift checks from this section should normally use `scripts/check/` plus `just ci-...` wrappers and land in `.github/workflows/docs.yml`.
- If a check is primarily enforcing code-contract updates rather than prose quality, keep the validator in the owning domain workflow and only reuse `docs.yml` for the written-surface component.

### Tasks to fix

- [x] Update `docs/804_testing_guide.md` to document:
  - the canonical shared UX contract
  - determinism-by-construction requirements
  - authoritative event/barrier rules
  - side-effect-free observation requirements
  - trace-based conformance expectations
- [x] Update `docs/997_ux_flow_coverage.md` to distinguish:
  - parity-critical shared flows
  - frontend-specific flows
  - required action contracts
  - required barrier/event ownership
- [x] Update architecture docs where relevant, especially:
  - `docs/001_system_architecture.md`
  - `docs/999_project_structure.md`
  to describe where shared UX contract code, harness validation, and deterministic observation responsibilities live.
- [x] Add or update a dedicated harness/UX determinism design note in `docs/` if the testing guide becomes too overloaded.
- [x] Update the repository `AGENTS.md` guidance so future agents are told:
  - parity-critical IDs must come from shared contract helpers
  - harness mode may not change product execution semantics
  - parity-critical waits must cite authoritative events or quiescence conditions
  - new shared UX flows must update docs and contract metadata together
- [x] Update `.claude/skills/` where relevant so project-specific skills teach:
  - the canonical UX contract approach
  - determinism and trace expectations
  - what not to do when extending the harness or frontends
  - required documentation updates when shared UX flows change

### Tasks to enforce go-forward

- [x] Add CI validation that shared UX or harness changes touching contract, barrier, parity-surface, or determinism code also update the mapped authoritative docs metadata.
- [x] Add CI validation that contributor workflow changes touching shared UX implementation rules also update the mapped `AGENTS.md` guidance.
- [x] Add CI validation that changes to project-specific shared UX workflows also update the mapped `.claude/skills/` guidance when applicable.
- [x] Add CI validation that rejects behavior changes to parity-critical flows without corresponding documentation or contract updates.
- [x] Add a machine-readable map naming which docs are authoritative for:
  - shared UX contract
  - harness determinism rules
  - scenario coverage rules
  - contributor/agent implementation expectations
  and validate in CI that updates touch the required targets.

### Success criteria

- `docs/` accurately describes the implemented harness, parity contract, and determinism rules.
- `AGENTS.md` steers future contributors and coding agents away from reintroducing local IDs, harness-only product paths, heuristic waits, and undocumented shared-flow behavior.
- `.claude/skills/` teaches the same constraints and workflows that the codebase expects.
- Shared UX or harness changes cannot land cleanly without updating the relevant written guidance.

## Architecture docs and enforcement gaps

This section captures gaps between what the architecture and testing docs say, what they fail to say, and what the repository actually enforces today.

This section should reference the foundational contract and determinism sections above rather than introduce competing definitions. Its role is to make ownership and architectural consequences explicit.

CI implementation note:
- Source-level architectural drift checks should reuse the existing `scripts/check/` style and, where possible, extend current harness/parity/shared-flow gates rather than introducing one-off scripts.
- Behavioral proof for architecture claims should live in focused crate tests or harness-domain lanes, then be surfaced through `just ci-...` recipes and the smallest matching workflow.

### Missing invariants to add to architecture docs

#### A. Shared quiescence and monotonic revision model

Problem:
- The docs describe the system as deterministic and semantic-first, but they do not state that parity-critical UI state must expose a shared quiescence/completion model and monotonic revisions or sequence numbers.

### Tasks to fix

- [x] Add an explicit invariant to `crates/aura-harness/ARCHITECTURE.md` that parity-critical waits must resolve against a documented quiescence/completion contract.
- [x] Add an explicit invariant to `crates/aura-ui/ARCHITECTURE.md` and `crates/aura-web/ARCHITECTURE.md` that published semantic state must carry monotonic revision or sequence information sufficient to detect stale observations.
- [x] Update `docs/804_testing_guide.md` to document the shared quiescence/revision contract and how shared-flow waits are supposed to use it.

### Tasks to enforce go-forward

- [x] Add CI validation that parity-critical observation payloads include revision/sequence metadata once the contract is introduced.
- [x] Add tests that stale snapshots are detectable by revision comparison rather than inferred heuristics.
- [x] Add a typed wait/barrier API that requires a quiescence or completion contract reference and reject parity-critical waits outside that API.

### Success criteria

- The architecture docs explicitly define quiescence and stale-state detection semantics.
- The harness can detect stale state by contract, not guesswork.
- New parity-critical waits cannot be introduced without an authoritative completion model.

#### B. Side-effect-free observation and explicit recovery separation

Problem:
- The docs say semantic observation is authoritative, but they do not clearly state that observation must be side-effect free and must not repair state.

### Tasks to fix

- [x] Add an explicit invariant to `crates/aura-harness/ARCHITECTURE.md` that observation APIs are side-effect free and recovery is a separate explicit mechanism.
- [x] Add a matching invariant to `crates/aura-web/ARCHITECTURE.md` for browser harness bridge behavior.
- [x] Update `docs/804_testing_guide.md` to document that observation must not mutate application state or invoke hidden recovery.

### Tasks to enforce go-forward

- [x] Add contract tests that call observation endpoints repeatedly and fail if any semantic state or driver state changes.
- [x] Add CI validation that new recovery behavior is added only through an explicit recovery registry/API, not inside observation helpers.
- [x] Add tests that default observation fails diagnostically before any retry or repair behavior occurs.

### Success criteria

- Architecture docs explicitly separate observation from recovery.
- Observation APIs are mechanically testable as side-effect free.
- Hidden recovery inside state reads is rejected in CI.

#### C. Harness mode cannot change product execution semantics

Problem:
- The frontend architecture docs do not explicitly state that harness mode may change instrumentation/observation, but may not change business-flow execution semantics.

### Tasks to fix

- [x] Add an explicit invariant to `crates/aura-terminal/ARCHITECTURE.md` that harness mode must not bypass or replace normal user-visible execution paths for parity-critical flows.
- [x] Add corresponding language to `crates/aura-web/ARCHITECTURE.md` and `docs/804_testing_guide.md` so this rule is stated consistently across frontend and harness docs.

### Tasks to enforce go-forward

- [x] Add CI search/lint checks that reject `AURA_HARNESS_MODE` branches in product action handlers outside approved observation/instrumentation paths.
- [x] Add CI validation that any allowlisted harness-specific hook has an owner, justification, and design-note reference.

### Success criteria

- The docs explicitly ban harness-only execution semantics for parity-critical flows.
- CI rejects new harness-mode product-path bypasses.

#### D. Canonical shared identifiers, focus semantics, and action contracts

Problem:
- `aura-app` says it owns the shared contract, but the frontend architecture docs do not explicitly state the inverse invariant that parity-critical IDs, focus semantics, and action contracts must not be derived locally.

### Tasks to fix

- [x] Add explicit invariants to `crates/aura-ui/ARCHITECTURE.md` and `crates/aura-terminal/ARCHITECTURE.md` that parity-critical identifiers, focus semantics, and action metadata must come from `aura-app::ui_contract`.
- [x] Update `crates/aura-app/ARCHITECTURE.md` to state that the shared contract includes canonical identifiers, focus semantics, action contracts, and frontend classification metadata.
- [x] Update `docs/804_testing_guide.md` to document these constraints for shared-flow work.

### Tasks to enforce go-forward

- [x] Add lint or CI search checks that reject parity-critical ID construction from ad hoc string formatting in frontend crates.
- [x] Add CI validation that new shared actions are rejected unless they declare focus/selection semantics in shared contract metadata.
- [x] Add parity contract tests that fail when frontends export mismatched parity-critical IDs or focus semantics.

### Success criteria

- The docs make canonical contract ownership explicit on both sides: `aura-app` owns it and frontends must consume it.
- CI rejects local reinvention of parity-critical identifiers and action semantics.

#### E. No placeholder IDs, override caches, or heuristic event synthesis for parity-critical state

Problem:
- The current docs do not explicitly prohibit placeholder IDs, export-time override caches, or heuristic runtime-event synthesis for parity-critical semantic state.

### Tasks to fix

- [x] Add explicit invariants to `crates/aura-terminal/ARCHITECTURE.md` and `crates/aura-harness/ARCHITECTURE.md` that parity-critical semantic exports must not rely on placeholder IDs, view-only override caches, or heuristic success/event inference.
- [x] Update `docs/804_testing_guide.md` to document that parity-critical semantic state must come from authoritative frontend/runtime state.

### Tasks to enforce go-forward

- [x] Add exporter invariant tests that reject placeholder IDs, override-backed parity-critical lists, and heuristic runtime events.
- [x] Add CI validation that new parity-critical export helpers cannot depend on override caches outside explicit debug-only modules.

### Success criteria

- The docs forbid the exact TUI export patterns that currently create ambiguity.
- CI catches regressions toward placeholder or heuristic semantic state.

#### F. Onboarding must use the same semantic snapshot/publication path

Problem:
- The current docs do not state that onboarding must use the same canonical snapshot/publication path as the rest of the UI.

### Tasks to fix

- [x] Add an explicit invariant to `crates/aura-ui/ARCHITECTURE.md` and `crates/aura-web/ARCHITECTURE.md` that onboarding state is exported through the same semantic snapshot/publication mechanism as all other screens.
- [x] Update `docs/804_testing_guide.md` to document onboarding as part of the same semantic UI contract, not a separate publication model.

### Tasks to enforce go-forward

- [x] Add CI validation that new onboarding states are declared in the shared snapshot model.
- [x] Add tests that reject onboarding-only publication paths for parity-critical observation.

### Success criteria

- The docs explicitly forbid a separate onboarding publication pipeline.
- CI catches onboarding-specific parity/export drift.

### Policy-level guidance that should be made programmatic

#### G. Shared flows must use typed primitives and structured semantic waits

Problem:
- The testing guide says shared flows should use typed scenario primitives and structured snapshot waits, and should prefer semantic readiness over fallback text matching, but current enforcement is incomplete.

### Tasks to fix

- [x] Update `docs/804_testing_guide.md` and `crates/aura-harness/ARCHITECTURE.md` to narrow this from a preference statement into a hard invariant for parity-critical flows.

### Tasks to enforce go-forward

- [x] Add CI validation that rejects parity-critical raw waits, raw text assertions, and raw mechanic actions outside explicitly classified non-shared scenarios.
- [x] Add CI validation that scenario actions must resolve through typed semantic primitives in the canonical scenario model.
- [x] Add tests that parity-critical executor waits resolve through semantic wait helpers only.

### Success criteria

- The docs no longer frame semantic waits as best practice; they define them as required.
- CI rejects parity-critical regressions back to raw mechanics.

#### H. UX flow coverage expectations need stronger enforcement

Problem:
- The UX coverage report describes PR-gate expectations, but current enforcement is largely filename-heuristic-based and accepts docs-only updates as satisfying flow-relevant changes.

### Tasks to fix

- [x] Update `docs/997_ux_flow_coverage.md` to distinguish:
  - traceability/reporting requirements
  - actual CI enforcement guarantees
  - residual heuristic limitations that still need replacement

### Tasks to enforce go-forward

- [x] Replace filename-regex flow inference in `scripts/check/ux-flow-coverage.sh` with typed metadata mapping from shared flows to owned source areas where possible.
- [x] Add CI validation that parity-critical flow changes require either:
  - updated scenario files
  - updated typed coverage metadata
  - an explicit allowlisted waiver
  rather than accepting a docs touch alone.
- [x] Remove or tightly restrict the env-var skip path for coverage checks in CI.

### Success criteria

- The docs accurately describe what coverage CI actually guarantees.
- Coverage enforcement depends on typed ownership metadata instead of filename heuristics.
- Docs-only touches no longer satisfy flow-coverage expectations for real behavior changes.

#### I. Semantic-first observation is only partially enforced today

Problem:
- The architecture docs say `UiSnapshot`/heartbeat are authoritative and DOM/text fallbacks are diagnostics only, but current checks mainly verify API presence and scenario syntax, not real semantic-first behavior.

### Tasks to fix

- [x] Update `crates/aura-harness/ARCHITECTURE.md` and `crates/aura-web/ARCHITECTURE.md` to state explicitly that fallback DOM/text paths must not be used as success-path behavior for parity-critical observation.

### Tasks to enforce go-forward

- [x] Add CI validation that parity-critical observation code paths cannot resolve success through DOM/text fallback APIs.
- [x] Add tests that fallback DOM/text paths are reachable only in diagnostic/error-reporting modes.
- [x] Strengthen `scripts/check/harness-ui-state-evented.sh` or replace it with typed tests that verify behavioral semantics, not just symbol existence.

### Success criteria

- Semantic-first observation is enforced behaviorally, not just by naming conventions.
- DOM/text fallbacks cannot silently become correctness paths.

#### J. Browser harness bridge compatibility and determinism claims are under-enforced

Problem:
- `aura-web` says harness bridge methods are deterministic and backwards-compatible, but the current docs point to vague verification hooks rather than a strong compatibility gate.

### Tasks to fix

- [x] Update `crates/aura-web/ARCHITECTURE.md` to define what backwards compatibility means for the browser harness bridge:
  - versioning expectations
  - allowed additive changes
  - breaking-change process

### Tasks to enforce go-forward

- [x] Add a versioned contract test suite for the browser harness bridge request/response surface.
- [x] Add CI validation that changes to the exposed harness bridge API require version or compatibility metadata updates.
- [x] Add deterministic behavior tests for bridge methods that return semantic state or render convergence signals.

### Success criteria

- The web architecture doc makes compatibility requirements concrete.
- CI enforces harness bridge contract stability instead of relying on broad self-tests.

#### K. Terminal typed-contract guidance is not strongly enforced

Problem:
- `aura-terminal` says internal command/modal contracts should prefer typed IDs and structured variants over free-form strings, but that is mostly guidance today.

### Tasks to fix

- [x] Update `crates/aura-terminal/ARCHITECTURE.md` to identify which command, modal, and operational response surfaces are parity-critical and therefore must be typed.

### Tasks to enforce go-forward

- [x] Add lint or CI checks that reject new parity-critical command/modal surfaces using raw `String` identifiers where typed IDs exist.
- [x] Add tests that parity-critical operational responses use structured enums/variants instead of stringly-typed status text.

### Success criteria

- The terminal architecture doc is precise about which surfaces must be typed.
- CI rejects new stringly-typed parity-critical contracts.

#### L. Shared UI shape uniformity is under-specified and under-enforced

Problem:
- `aura-ui` says shared flow shapes are uniform, but current enforcement is still relatively narrow and does not fully cover parity-critical IDs, focus semantics, or list/entity shape consistency.

### Tasks to fix

- [x] Update `crates/aura-ui/ARCHITECTURE.md` to define uniform shared-flow shape in terms of:
  - canonical IDs
  - focus semantics
  - list/entity shape
  - operation/runtime-event shape

### Tasks to enforce go-forward

- [x] Add parity contract tests that compare parity-critical IDs, focus semantics, list shapes, and runtime-event shapes across frontends.
- [x] Add CI validation that new parity-critical UI surface additions must be registered in `aura-app::ui_contract` and reflected in parity tests.

### Success criteria

- The `aura-ui` architecture doc defines uniformity precisely enough to test.
- Parity enforcement covers more than just uniqueness/addressability and selector mapping.

## Additional enforcement gaps from `work/harness_determinism.md`

This section captures items from `work/harness_determinism.md` that still need stronger automatic enforcement. These are mostly cases where the plan states a desired property or success criterion, but the repository should enforce it mechanically through CI, typed APIs, or contract tests.

This section is intentionally downstream of the foundational contract and determinism sections. Items here should add missing enforcement for already-defined invariants, not restate the invariants themselves.

CI implementation note:
- Static repository-boundary rules in this section should prefer `scripts/check/` and existing harness/shared-flow/parity lanes.
- Real frontend execution, bridge behavior, and matrix enforcement should extend the existing `harness.yml` family and the `just ci-harness-*` command surface.

### M. No duplicated core UI identity strings outside the shared contract

Problem:
- The determinism plan requires that new duplicated string constants for core UI identities not be introduced outside the shared contract, but that should be mechanically enforced rather than left as an informal success criterion.

### Tasks to fix

- [x] Add an explicit shared-helper boundary for parity-critical UI identity construction so frontends and harness code consume canonical helpers instead of hand-rolled strings.

### Tasks to enforce go-forward

- [x] Add a lint or CI search check that rejects new parity-critical UI identity string literals outside approved shared helper modules.
- [x] Add contract tests that compare generated parity-critical IDs across crates and fail on drift.

### Success criteria

- Core UI identity strings are emitted only from shared helper surfaces.
- CI rejects new duplicated parity-critical identity strings outside the contract layer.

### N. Read-only observation surfaces must be provably read-only

Problem:
- The determinism plan requires a read-only web UI state endpoint and machine-readable observation channels, but that should be backed by tests proving they do not mutate state or expose action surfaces.

### Tasks to fix

- [x] Define observation-surface contracts for web and TUI that explicitly separate read-only state export from any action API.

### Tasks to enforce go-forward

- [x] Add contract tests that repeated reads of web/TUI observation endpoints produce no semantic state mutation or action side effects.
- [x] Add CI validation that observation-surface modules do not export action methods on the same interface.

### Success criteria

- Observation endpoints are mechanically verified as read-only.
- CI rejects observation/action API blending.

### O. No row-index addressing in parity-critical state

Problem:
- The determinism plan says list items and selections must be id-based, but this should be enforced at the snapshot/export boundary and in scenario validation.

### Tasks to fix

- [x] Define snapshot/export invariants forbidding row-index addressing for parity-critical selections and list references.

### Tasks to enforce go-forward

- [x] Add snapshot invariant tests that fail if parity-critical selections are represented by row indexes or implicit ordering.
- [x] Add CI validation that shared scenarios cannot target parity-critical list items by row index.

### Success criteria

- Parity-critical selections and list references are always ID-based.
- CI rejects regressions to row-index-based addressing.

### P. Actions must not execute before readiness gates pass

Problem:
- The determinism plan says harnesses should not interact until readiness is true, but this should be enforced by the action/wait API rather than being advisory.

### Tasks to fix

- [x] Define typed action preconditions that bind parity-critical actions to specific readiness or quiescence gates.

### Tasks to enforce go-forward

- [x] Add CI validation that parity-critical action execution goes through a typed API that checks readiness/preconditions.
- [x] Add tests that action execution before readiness fails diagnostically instead of implicitly retrying or racing.

### Success criteria

- Parity-critical actions cannot be issued without satisfying declared readiness gates.
- Precondition violations fail explicitly and deterministically.

### Q. Harness mode must not alter core business-flow semantics

Problem:
- The determinism plan says real-runtime harness mode should improve determinism without changing semantics, but that needs stronger mechanical enforcement.

### Tasks to fix

- [x] Define an allowlisted set of harness-mode changes limited to observation, timing discipline, rendering stability, and instrumentation.

### Tasks to enforce go-forward

- [x] Add CI checks that reject harness-mode branches changing parity-critical business-flow execution paths.
- [x] Add differential tests that compare harness-mode and non-harness-mode semantic outcomes for representative parity-critical flows.

### Success criteria

- Harness mode is constrained to non-semantic behavior.
- CI catches harness-mode business-flow drift.

### R. Single frontend execution layer must be enforced at the repository boundary

Problem:
- The determinism plan says the harness is the single frontend execution layer, but this should be guarded mechanically so no parallel frontend-driving path reappears.

### Tasks to fix

- [x] Define a repository-level ownership boundary for frontend-driving code paths and scenario execution entry points.

### Tasks to enforce go-forward

- [x] Add CI checks that reject new frontend-driving execution stacks outside approved harness driver paths.
- [x] Add CI checks that reject new scenario executors or parallel scenario dialects without shared-contract integration.

### Success criteria

- Frontend execution paths are centralized and machine-guarded.
- Parallel frontend-driving stacks cannot quietly reappear.

### S. Debug/legacy fallback paths must be kept out of correctness paths

Problem:
- The determinism plan says fallback DOM/PTY scraping should be debug-only, but this should be enforced behaviorally rather than only documented.

### Tasks to fix

- [x] Classify fallback observation and interaction paths explicitly as:
  - authoritative
  - bounded secondary
  - diagnostics-only

### Tasks to enforce go-forward

- [x] Add CI validation that parity-critical success paths cannot resolve through diagnostics-only fallbacks.
- [x] Add tests that fallback paths emit explicit diagnostics when used and cannot silently override authoritative state.

### Success criteria

- Legacy/debug fallbacks cannot silently become correctness mechanisms.
- The active observation path is attributable and testable.

### T. Mirrored frontend structure and naming need stronger machine checks

Problem:
- The determinism plan requires mirrored structure and shared definition naming across web and TUI, but enforcement should be stronger than documentation plus a lightweight script.

### Tasks to fix

- [x] Define machine-readable metadata for parity-critical screen/module mappings and canonical naming expectations.

### Tasks to enforce go-forward

- [x] Add CI validation that parity-critical screen/module mappings exist and point to current implementations.
- [x] Add CI validation that shared definition names resolve to canonical contract identifiers and reject drift unless explicitly classified as an exception.

### Success criteria

- Parity-critical structure and naming are validated from typed metadata rather than informal conventions.
- Drift is caught before parity scenarios fail.

### U. Parity exceptions must carry required structured metadata

Problem:
- The determinism plan says parity exceptions should be typed and justified, but justification should be structured metadata, not just prose in docs or comments.

### Tasks to fix

- [x] Extend parity exception declarations to include required metadata fields such as:
  - owning reason code
  - scope
  - affected flow/screen
  - approving doc or issue reference

### Tasks to enforce go-forward

- [x] Add CI validation that new parity exceptions cannot be declared without complete structured metadata.
- [x] Add CI validation that parity checks fail if an observed divergence is not covered by a declared exception.

### Success criteria

- Exceptions are typed, structured, and machine-auditable.
- Undeclared parity drift fails cleanly.

### V. Parity-related docs updates should be CI-enforced

Problem:
- The determinism plan includes documentation for parity rules, but parity model changes should require corresponding doc/metadata updates automatically.

### Tasks to fix

- [x] Define the authoritative parity documentation/update map covering docs, contract metadata, and support declarations.

### Tasks to enforce go-forward

- [x] Add CI validation that changes to parity-critical flow support, exceptions, classifications, or naming mappings update the required docs/metadata targets.

### Success criteria

- Parity model changes cannot land without corresponding authoritative updates.
- Docs and contract metadata stay in sync.

### W. Post-render authoritative `UiSnapshot` must be behaviorally enforced

Problem:
- The determinism plan says `UiSnapshot` should be post-render authoritative, but that should be enforced with direct convergence tests, not just architectural intent.

### Tasks to fix

- [x] Define explicit render-convergence invariants for published semantic state in both web and TUI.

### Tasks to enforce go-forward

- [x] Add tests that fail if semantic state is published ahead of the renderer for parity-critical screen/modal changes.
- [x] Add CI validation that parity-critical publication paths go through approved post-render/post-commit hooks only.

### Success criteria

- Published semantic state is guaranteed to reflect committed render state.
- Semantic/render split-brain regressions fail in tests immediately.

### X. Protocol readiness contracts must be required metadata

Problem:
- The determinism plan calls for protocol-level readiness contracts, but new shared intents should be unable to land without declared readiness semantics.

### Tasks to fix

- [x] Extend shared intent metadata to include required readiness/barrier declarations for parity-critical async flows.

### Tasks to enforce go-forward

- [x] Add CI validation that new parity-critical intents are rejected unless they declare readiness/barrier metadata.
- [x] Add tests that scenario waits for these intents bind only to declared readiness contracts.

### Success criteria

- Every parity-critical async intent has explicit readiness semantics.
- New shared flows cannot rely on implicit eventual convergence.

### Y. Scenario inventory/classification must drive matrix enforcement

Problem:
- The determinism plan ends with a clean matrix goal, but scenario classification should automatically determine required TUI/web matrix coverage in CI.

CI implementation note:
- Keep inventory-to-matrix validation as a fast `scripts/check/` gate with a `just ci-...` wrapper.
- Keep actual TUI/web matrix execution in the existing `just ci-harness-matrix-tui`, `just ci-harness-matrix-web`, and `harness.yml` scheduled/dispatch lanes rather than creating a separate workflow family.

### Tasks to fix

- [x] Make scenario inventory classification the authoritative source for required frontend matrix membership.

### Tasks to enforce go-forward

- [x] Add CI validation that every `shared` scenario is scheduled for both TUI and web lanes unless an explicit typed exception exists.
- [x] Add CI validation that `tui_only` and `web_only` scenarios appear only in their expected matrix lanes.
- [x] Add CI validation that matrix commands derive scenario sets from the inventory rather than ad hoc command lists.

### Success criteria

- The expected scenario matrix is machine-derived from one authoritative inventory.
- Frontend coverage drift shows up as a matrix validation failure, not tribal knowledge.

### Z. Harness-mode convergence/sync discipline needs typed enforcement

Problem:
- The determinism plan calls for explicit sync/discovery/post-operation convergence discipline, but these rules should be encoded as typed post-operation contracts instead of policy.

### Tasks to fix

- [x] Define typed post-operation convergence contracts for flows that require sync/discovery follow-up before the next intent.

### Tasks to enforce go-forward

- [x] Add CI validation that parity-critical flows with declared convergence contracts cannot proceed without satisfying them.
- [x] Add tests that missing sync/discovery prerequisites fail as explicit convergence-contract violations.

### Success criteria

- Convergence discipline is encoded in typed contracts, not implicit harness behavior.
- Shared flows do not advance on opportunistic sync luck.

### AA. Single deterministic app shell and root structure need runtime contract tests

Problem:
- The determinism plan requires one app root, one modal region, one toast region, and one active screen root, but this should be enforced with runtime contract tests, not just structural scripts.

### Tasks to fix

- [x] Define a runtime shell-structure contract for harness mode in the shared UI contract or web harness contract.

### Tasks to enforce go-forward

- [x] Add runtime/browser contract tests asserting exactly-one shell region semantics in harness mode.
- [x] Add CI validation that duplicate or ambiguous shell roots fail before scenario execution begins.

### Success criteria

- Ambiguous shell structure is caught as a first-class contract violation.
- Playwright never needs to guess which root is authoritative.

### AB. Typed backend intent methods must be prevented from bypassing the real UI

Problem:
- The determinism plan says typed backend intent methods should drive the real user-facing UI and not app-internal shortcuts. This is one of the highest-value constraints and should be enforced explicitly.

### Tasks to fix

- [x] Define an architectural rule for shared intent backends: they may translate semantic intents into frontend mechanics, but may not call app-internal shortcut paths that bypass the user-visible flow for parity-critical intents.

### Tasks to enforce go-forward

- [x] Add CI/search checks that reject calls from typed shared-intent backend implementations into banned app-internal shortcut APIs for parity-critical flows.
- [x] Add focused driver contract tests asserting that shared intent methods exercise the expected user-visible control path or modal flow rather than a shortcut.
- [x] Add allowlist-based validation for any exceptional bypass with explicit typed classification and justification metadata.

### Success criteria

- Typed intent backends still validate the real UI path.
- Shortcut execution paths cannot quietly re-enter shared-flow execution.

## 1. Canonical shared UX contract and identifiers

Problem:
- The web and TUI do not actually expose the same shared UI surface in several important places.
- Settings is the clearest example. Web settings sections are `Profile`, `GuardianThreshold`, `RequestRecovery`, `Devices`, `Authority`, `Appearance`, and `Info`. TUI settings sections are `Profile`, `Threshold`, `Recovery`, `Devices`, `Authority`, and `Observability`.
- Even where the frontends mean the same thing, they often derive IDs differently. Web uses canonical DOM-oriented IDs such as `guardian-threshold` and `request-recovery`. TUI derives IDs from section titles with lowercase plus underscore conversion.
- The current parity layer compensates by normalizing IDs and dropping frontend-specific items instead of comparing one canonical contract.

Why this matters:
- The harness is currently reconciling two related but structurally different frontends.
- Shared-flow claims are weaker than they look because the “shared” surface is partly aspirational.
- If identifiers are frontend-local formatting decisions, parity and scenarios will keep accumulating normalization logic.

References:
- `crates/aura-ui/src/model.rs:390`
- `crates/aura-ui/src/model.rs:438`
- `crates/aura-terminal/src/tui/types.rs:606`
- `crates/aura-terminal/src/tui/harness_state.rs:483`
- `crates/aura-app/src/ui_contract.rs:1198`
- `crates/aura-app/src/ui_contract.rs:1395`
- `crates/aura-ui/src/app.rs:4979`
- `crates/aura-terminal/src/tui/screens/settings/screen.rs:544`

### Tasks to fix

- [x] Define a parity-critical shared UX surface in `aura-app::ui_contract` that explicitly distinguishes:
  - shared settings sections
  - frontend-specific settings sections
  - canonical list item identifiers
  - canonical focus/control identifiers
- [x] Replace frontend-local string derivation for parity-relevant IDs with shared identifiers from the contract layer.
- [x] Decide whether `Appearance`, `Info`, and `Observability` should be:
  - truly shared
  - explicitly frontend-specific
  - moved out of the parity-critical settings surface
- [x] Remove `normalize_parity_item_id` exceptions that exist only because the two frontends currently disagree on names and ID formats.

### Tasks to enforce go-forward

- [x] Add a contract test that fails if a parity-critical settings section exists in one frontend but not the shared contract.
- [x] Add a contract test that fails if a parity-critical list emits an ID not produced by the shared identifier helpers.
- [x] Add a lint or CI search check that rejects ad hoc parity-relevant ID construction in frontend crates outside approved shared helper APIs.
- [x] Add CI validation that any new frontend-specific section or control is explicitly classified in `ui_contract` as parity-critical or non-parity-critical.

### Success criteria

- Web and TUI export the same identifiers for every parity-critical settings section and list item.
- `ui_contract` does not need settings-specific normalization or drop rules for parity.
- Shared-flow support matches actual product structure instead of encoding avoidable frontend exceptions.

## 2. Give the TUI one authoritative semantic snapshot path

Problem:
- The TUI semantic snapshot is reconstructed from multiple overlapping sources instead of one authoritative model.
- `semantic_ui_snapshot` mixes `TuiState`, `StateSnapshot`, override caches for contacts/devices/messages, synthetic fallback IDs such as `contact-{idx}`, and export-time operation/event synthesis.
- The contacts and chat screens publish harness overrides back into the snapshot layer, which means the harness is partly observing rendered view-model state rather than one canonical semantic state.

Why this matters:
- This is a direct source of non-determinism and opacity.
- The harness cannot trust that a TUI snapshot is a first-class product state; it is often a reconstruction that compensates for lag or missing data.
- Placeholder IDs and override caches are a sign that the exported semantic model is downstream of the real UI state instead of being the UI state.

References:
- `crates/aura-terminal/src/tui/harness_state.rs:170`
- `crates/aura-terminal/src/tui/harness_state.rs:248`
- `crates/aura-terminal/src/tui/harness_state.rs:275`
- `crates/aura-terminal/src/tui/harness_state.rs:483`
- `crates/aura-terminal/src/tui/harness_state.rs:540`
- `crates/aura-terminal/src/tui/harness_state.rs:577`
- `crates/aura-terminal/src/tui/screens/contacts/screen.rs:406`
- `crates/aura-terminal/src/tui/screens/chat/screen.rs:313`

### Tasks to fix

- [x] Define one TUI-owned semantic state model that is authoritative for harness export.
- [x] Remove harness override channels for contacts, devices, and messages from the steady-state export path.
- [x] Remove synthetic placeholder IDs such as `contact-{idx}` from parity-critical lists.
- [x] Move exported operation state and runtime-event production closer to the actual state transitions that own them, instead of synthesizing them during snapshot export.
- [x] Make `semantic_ui_snapshot` a projection of authoritative state, not a best-effort merger of independent caches.

### Tasks to enforce go-forward

- [x] Add a test that fails if TUI harness snapshots contain placeholder IDs for parity-critical entities.
- [x] Add a test that fails if the snapshot exporter depends on override caches for parity-critical lists.
- [x] Add a lint or CI search check that rejects new parity-critical export-time override channels outside approved debug-only modules.
- [x] Add a test that snapshot generation is pure projection logic with no semantic reconstruction branch for normal ready-state operation.

### Success criteria

- TUI snapshots can be explained as a direct serialization of authoritative UI state.
- The contacts, devices, and messages override helpers are gone or restricted to non-parity debug use only.
- Exported entity IDs are stable and never synthesized to paper over missing state.

## 3. Unify selection, focus, and action-precondition semantics

Problem:
- The TUI currently has split sources of truth for selection and navigation.
- Channel selection is UI-local state, the compatibility callback is effectively a no-op, and send-message dispatch has to recover the selected channel from multiple sources.
- When selection lags, the TUI retries in the background and shows a “Channel selection syncing” warning.
- Starting a chat navigates optimistically to Chat before the async operation has actually finalized.
- Web and TUI also export focus at different semantic resolutions. Web focus is usually just the screen root or onboarding root. TUI exports more specific focus states such as input focus and modal field focus.

Why this matters:
- If the frontends do not agree on what it means for something to be selected, focused, or ready for an action, the harness will always need retries and special waits.
- Parity currently ignores `focused_control` largely because the frontends are not aligned enough for it to be reliable.

References:
- `crates/aura-terminal/src/tui/callbacks/factories.rs:526`
- `crates/aura-terminal/src/tui/screens/app/shell.rs:1611`
- `crates/aura-terminal/src/tui/screens/app/shell.rs:1617`
- `crates/aura-terminal/src/tui/screens/app/shell.rs:1650`
- `crates/aura-terminal/src/tui/screens/app/shell.rs:1702`
- `crates/aura-terminal/src/tui/screens/app/shell.rs:2036`
- `crates/aura-ui/src/model.rs:1511`
- `crates/aura-terminal/src/tui/harness_state.rs:170`

### Tasks to fix

- [x] Define one shared semantic model for:
  - selected entity
  - active screen
  - focused control
  - action preconditions
- [x] Change TUI action dispatch so it depends on committed authoritative selection state instead of background recovery loops.
- [x] Remove optimistic screen transitions that advance before the underlying operation or data model has actually reached the required state.
- [x] Align web and TUI focus export so both frontends report focus at the same semantic granularity for parity-critical flows.

### Tasks to enforce go-forward

- [x] Add a parity test for `focused_control` once the shared focus model is aligned.
- [x] Add frontend tests that action handlers fail fast when their semantic preconditions are not satisfied, rather than retrying hidden recovery logic in the background.
- [x] Add CI validation that any new shared user action is rejected unless it declares selected-entity and focus semantics in the shared contract metadata.
- [x] Add tests that navigation side effects do not advance the screen unless the corresponding semantic state transition has actually committed.

### Success criteria

- Sending a message, starting a chat, and similar actions do not rely on fallback selection recovery loops.
- `focused_control` becomes parity-relevant and reliable.
- Frontend behavior no longer depends on “UI-local state plus async catch-up” for parity-critical actions.

## 4. Remove harness-only frontend behavior and hidden runtime shortcuts

Problem:
- The TUI contains explicit `AURA_HARNESS_MODE` branches that bypass normal UI pathways and call the runtime bridge directly for invitation-related flows.
- That means the harness is not always testing the same path that a real user follows through the product.

Why this matters:
- Harness-only product logic is high-value tech debt because it silently creates a second behavior surface.
- It makes failures harder to interpret and weakens the meaning of “real frontend execution.”

References:
- `crates/aura-terminal/src/tui/screens/app/shell.rs:2162`
- `crates/aura-terminal/src/tui/screens/app/shell.rs:2271`

### Tasks to fix

- [x] Remove TUI harness-mode branches that directly call runtime invitation APIs from the product path.
- [x] Ensure the harness drives the same user-visible flow the TUI uses outside harness mode.
- [x] If specialized observation is still needed, move it into harness-only observation/export code rather than product action code.

### Tasks to enforce go-forward

- [x] Add a CI search/lint check that rejects `AURA_HARNESS_MODE` branches inside product action handlers outside approved observation/instrumentation paths.
- [x] Add a search-based or lint-style CI check for new `AURA_HARNESS_MODE` branches inside product action handlers.
- [x] Add CI validation that any harness-specific execution-path hook is declared in an allowlist with an owning issue or design note; otherwise reject it.

### Success criteria

- The TUI uses the same execution path for parity-critical flows in harness and non-harness mode.
- `AURA_HARNESS_MODE` is limited to observation, determinism, and instrumentation concerns.

## 5. Integrate onboarding into the normal app state and publication model

Problem:
- Onboarding is structurally separate enough that both the product and harness have to patch around it.
- The web app manually publishes a synthetic onboarding snapshot with empty lists/messages and patched operation state.
- The browser harness bridge contains stale-onboarding recovery logic that prefers live state if the published onboarding snapshot appears stale.
- Elsewhere in the system, synthetic onboarding fallback behavior also exists on the TUI side of the harness.

Why this matters:
- Onboarding should not require a separate publication protocol if it is truly part of the same UI contract.
- Special-case onboarding logic is a persistent source of false greens and state publication drift.

References:
- `crates/aura-web/src/main.rs:465`
- `crates/aura-web/src/main.rs:484`
- `crates/aura-web/src/harness_bridge.rs:203`
- `crates/aura-harness/src/backend/local_pty.rs:421`

### Tasks to fix

- [x] Model onboarding in the same canonical semantic snapshot pipeline as the rest of the app.
- [x] Remove bespoke onboarding snapshot publication from the webapp.
- [x] Remove stale-onboarding recovery heuristics from the browser harness bridge once publication is authoritative.
- [x] Remove synthetic onboarding fallback snapshots from harness observation paths.

### Tasks to enforce go-forward

- [x] Add a contract test that onboarding snapshots are produced through the same export entry point as normal app screens.
- [x] Add a harness test that fails if live state and published state diverge during onboarding.
- [x] Add CI validation that any new onboarding UX state is declared in the shared snapshot model and reject separate onboarding-only publication paths in parity-critical code.

### Success criteria

- Onboarding state is exported through the same canonical mechanism as all other screens.
- The browser harness bridge no longer needs stale-onboarding detection.
- Harness observation does not fabricate onboarding snapshots to mask missing exports.

## 6. Make runtime events authoritative and strengthen parity

Problem:
- Shared-flow barriers are not backed by one authoritative runtime-event model.
- The TUI currently synthesizes runtime events from UI heuristics, such as:
  - contact link readiness when contacts exist
  - device enrollment code readiness when a modal is open or an operation is in flight
  - recipient peers resolved when a channel has more than one member
- Parity also ignores `focused_control`, `toasts`, and `runtime_events`, which means frontends can disagree on meaningful state and still pass.

Why this matters:
- Scenario waits can pass for the wrong reason.
- False greens become more likely.
- Parity is weaker than the semantic contract it claims to validate.

References:
- `crates/aura-terminal/src/tui/harness_state.rs:577`
- `crates/aura-terminal/src/tui/harness_state.rs:604`
- `crates/aura-terminal/src/tui/harness_state.rs:642`
- `crates/aura-app/src/ui_contract.rs:1317`
- `crates/aura-app/src/ui_contract.rs:1495`

### Tasks to fix

- [x] Define runtime events as authoritative facts produced by the owning frontend/runtime state transitions, not inferred during export.
- [x] Remove heuristic event synthesis from the TUI snapshot exporter for parity-critical events.
- [x] Expand parity comparison to include:
  - `focused_control`
  - `runtime_events`
  - selected toasts if they are part of user-visible flow semantics
- [x] Tighten scenario barriers so they wait on authoritative events instead of UI-derived approximations.

### Tasks to enforce go-forward

- [x] Add a test that fails if a parity-critical runtime event is emitted by export-time heuristics instead of a first-class state source.
- [x] Add contract tests for every shared-flow barrier that verify the same runtime event shape is emitted by both frontends.
- [x] Add CI validation that any new shared-flow barrier is rejected unless it references an authoritative event source declared in shared contract metadata.

### Success criteria

- Scenario barriers pass because authoritative events occurred, not because a heuristic guessed they probably did.
- Parity meaningfully covers the semantic state that shared scenarios rely on.
- Runtime-event mismatches become visible instead of being silently excluded from parity.

## 7. Separate observation from recovery in the browser driver and harness

Problem:
- The Playwright driver mixes observation with recovery logic.
- `ui_state` is not a passive query; it participates in repairing state.
- The driver includes startup retries, stale-onboarding repair, fallback click paths, fallback input paths, and navigation recovery.

Why this matters:
- This improves pass rate, but it also hides root causes.
- The observed path may already be a recovered path rather than the product’s actual behavior.
- Diagnosis quality drops because the harness mutates state while claiming to inspect it.

References:
- `crates/aura-harness/playwright-driver/playwright_driver.mjs:1596`
- `crates/aura-harness/playwright-driver/playwright_driver.mjs:1966`
- `crates/aura-harness/playwright-driver/playwright_driver.mjs:2031`
- `crates/aura-harness/playwright-driver/playwright_driver.mjs:2054`
- `crates/aura-harness/playwright-driver/playwright_driver.mjs:2206`
- `crates/aura-harness/playwright-driver/playwright_driver.mjs:2286`

### Tasks to fix

- [x] Split the driver API into clearly separate layers:
  - observation
  - action execution
  - explicit recovery utilities
- [x] Make `ui_state` and other observation endpoints side-effect free.
- [x] Reduce fallback click/input modes to a small, explicit set with clear logging and failure semantics.
- [x] Make recovery opt-in from the executor rather than implicit inside core observation calls.

### Tasks to enforce go-forward

- [x] Add tests that observation methods do not mutate the page or driver session state.
- [x] Add CI validation that any new recovery behavior is implemented through the explicit recovery action registry and emits structured logs.
- [x] Add tests that default driver behavior fails diagnostically before any retry path runs, and reject implicit recovery in observation APIs.

### Success criteria

- Observation is passive and reproducible.
- Recovery behavior is explicit, bounded, and visible in logs and reports.
- Browser failures are easier to attribute to product issues versus harness repair behavior.

## 8. Replace polling, sleeps, and file-based timing in the TUI lane

Problem:
- The TUI lane is still polling-and-sleep driven.
- Snapshot reading polls a JSON file in a loop.
- Missing files can produce synthetic fallback snapshots.
- PTY stabilization and some action flows depend on fixed delays.

Why this matters:
- Scheduler jitter, I/O timing, redraw latency, and machine speed affect results.
- This is a direct flakiness vector.
- File polling is a poor substitute for authoritative frontend observation when the rest of the system is trying to be semantic-first.

References:
- `crates/aura-harness/src/backend/local_pty.rs:205`
- `crates/aura-harness/src/backend/local_pty.rs:381`
- `crates/aura-harness/src/backend/local_pty.rs:421`
- `crates/aura-harness/src/executor.rs:2787`

### Tasks to fix

- [x] Replace file-polling snapshot discovery with an event-driven observation channel for the TUI.
- [x] Remove synthetic success-oriented fallback snapshots from missing-export paths.
- [x] Replace fixed sleeps with explicit waits on semantic state transitions or authoritative events.
- [x] Narrow PTY stabilization logic so it only handles rendering transport concerns, not semantic readiness.

### Tasks to enforce go-forward

- [x] Add a CI check or lint-style search for new `sleep`-based waits in parity-critical harness paths.
- [x] Add a test that missing TUI snapshot publication fails loudly instead of returning a synthetic snapshot.
- [x] Add a typed wait/barrier API that requires an authoritative event or state-transition reference and reject raw parity-critical waits outside that API in CI.

### Success criteria

- TUI harness readiness does not depend on filesystem polling or arbitrary sleeps.
- Missing observation state fails fast and diagnostically.
- The TUI lane is as event-driven as the browser lane for parity-critical semantics.

## 9. Collapse the dual scenario/config model

Problem:
- The harness still carries two scenario systems at once.
- `ScenarioStep` is a large kitchen-sink structure with many optional fields and overloaded semantics.
- Semantic and legacy-style representations both still matter, and conversion code bridges between them.

Why this matters:
- Canonical behavior is hard to locate.
- Bitrot is more likely because the same concept exists in multiple representations.
- Debugging gets harder because scenario meaning depends on both config shape and executor interpretation.

References:
- `crates/aura-harness/src/config.rs:169`
- `crates/aura-harness/src/config.rs:287`
- `crates/aura-harness/src/config.rs:376`
- `crates/aura-harness/src/config.rs:613`
- `crates/aura-harness/src/config.rs:647`

### Tasks to fix

- [x] Choose one canonical scenario representation for shared UX testing.
- [x] Remove overloaded or backward-compatibility-only fields from parity-critical scenario execution.
- [x] Delete conversion layers that exist solely to keep legacy and semantic representations alive in parallel.
- [x] Simplify executor inputs so scenario meaning is defined in one place.

### Tasks to enforce go-forward

- [x] Add schema tests that fail if new parity-critical scenario behavior is introduced only in compatibility fields.
- [x] Add CI validation that new shared-flow mechanics are rejected unless they are represented in the canonical scenario model.
- [x] Add a deprecation plan with explicit deadlines for any remaining non-canonical scenario fields.

### Success criteria

- Shared UX scenarios are authored and executed in one canonical model.
- The executor no longer needs to interpret multiple representations of the same scenario concept.
- Conversion-layer bugs like the `fault_loss` hardcode do not have room to recur.

## 10. Replace shell-script governance with typed Rust validation

Problem:
- Shared-scenario legality, mechanics restrictions, and UX-flow coverage are enforced by shell scripts and regexes.
- Some policy also depends on filename heuristics.

Why this matters:
- This is brittle and hard to evolve.
- Policy can drift away from the actual Rust scenario model.
- The system is harder to reason about because validation lives in several scripts with partially overlapping concerns.

References:
- `scripts/check/harness-shared-scenario-contract.sh:1`
- `scripts/check/harness-scenario-legality.sh:1`
- `scripts/check/harness-core-scenario-mechanics.sh:1`
- `scripts/check/ux-flow-coverage.sh:43`
- `scenarios/harness_inventory.toml:14`

### Tasks to fix

- [x] Move shared-scenario legality checks into typed Rust validation over the canonical scenario model.
- [x] Move UX-flow coverage mapping and inventory validation into typed Rust as well.
- [x] Replace filename-regex relevance inference with explicit metadata in the scenario or coverage model where possible.
- [x] Keep shell wrappers, if needed, as thin entry points that call one typed validator.

### Tasks to enforce go-forward

- [x] Add tests for the typed validator that cover known edge cases currently enforced by shell scripts.
- [x] Add CI validation that new governance rules cannot be introduced in standalone shell scripts without corresponding typed-validator support.
- [x] Add a CI check that rejects duplicated governance logic across multiple validation entry points unless explicitly allowlisted.

### Success criteria

- Governance rules are type-checked against the same model the executor uses.
- CI policy is easier to audit and less dependent on regexes and path heuristics.
- Shell scripts become wrappers, not the source of truth.

## 11. TypeScript migration for the Playwright driver

Problem:
- `playwright_driver.mjs` is large, stateful, and carries many loosely-typed payloads between Playwright, DOM state, driver commands, and semantic snapshots.

Why this matters:
- A TypeScript migration would improve maintainability and reduce accidental object-shape drift.
- It would make refactors safer, especially after the observation/recovery split is cleaned up.
- It will not, by itself, fix architectural flakiness.

References:
- `crates/aura-harness/playwright-driver/playwright_driver.mjs`

### Tasks to fix

- [x] Migrate the driver to TypeScript after the observation/recovery boundary is defined.
- [x] Define typed interfaces for:
  - UI snapshot payloads
  - driver RPC requests and responses
  - structured action results
  - explicit recovery results
- [x] Use the migration to split large untyped utility clusters into narrower modules.

### Tasks to enforce go-forward

- [x] Add type-checked build/lint coverage for the driver.
- [x] Add CI validation that new driver APIs use typed request/response contracts.
- [x] Add lint or type-level checks that reject ad hoc object payloads for cross-module driver communication without declared interfaces.

### Success criteria

- The driver has explicit types for all externally visible commands and payloads.
- Refactors to action execution, observation, and recovery helpers are safer.
- The migration lands after architectural cleanup, so it reduces complexity instead of just typing it.

## Definition of done for this workstream

This overall workstream is done when all of the following are true:

- Web and TUI agree on the parity-critical shared UX surface and identifiers.
- The TUI exports authoritative semantic state rather than reconstructed state with overrides and placeholders.
- Shared-flow barriers are backed by authoritative events, not export-time heuristics.
- Observation is passive, recovery is explicit, and TUI/browser waiting is event-driven.
- Shared scenarios use one canonical model and one typed governance layer.
- Harness-only product behavior is eliminated from parity-critical flows.

## Notes

- The highest-value implementation order is to fix product/frontend structure first, then simplify harness observation, then simplify scenario/governance infrastructure.
- Converting the Playwright driver to TypeScript is worth doing, but it should not be mistaken for the first or most important reliability fix.

## Cleanup and code removal opportunities

This section is specifically about subtraction. Once the stricter contracts, invariants, and CI enforcement in this document are in place, the repo should be able to delete or collapse a substantial amount of ad hoc, redundant, or compatibility-oriented code.

The goal is not to move complexity around. The goal is to remove whole categories of code that only exist because the current pipeline is ambiguous.

### 1. Collapse TUI export-time reconstruction and remove override caches

Problem:
- The TUI currently exports semantic state by merging `TuiState`, `StateSnapshot`, and override caches for contacts, devices, and messages.
- That creates both code duplication and ambiguity.

### Tasks to fix

- [x] Delete parity-critical uses of `publish_contacts_list_override`, `publish_devices_list_override`, and `publish_messages_override`.
- [x] Remove placeholder-ID generation such as synthetic `contact-{idx}` identifiers from parity-critical export paths.
- [x] Collapse `semantic_ui_snapshot` into a pure projection over one authoritative TUI semantic state model.
- [x] Remove export-time heuristic runtime-event synthesis for parity-critical events.

### Implementation guidance

- Start in `crates/aura-terminal/src/tui/harness_state.rs` and remove override-backed branches only after authoritative state is available.
- Keep any remaining override helpers explicitly debug-only and physically separate from the parity-critical export path.
- Add crate tests first, then delete the override plumbing from screen/subscription code.

### Success criteria

- TUI semantic export no longer depends on override caches for parity-critical state.
- Placeholder IDs and heuristic event synthesis are removed from the shared-flow path.
- The TUI snapshot exporter becomes materially smaller and easier to reason about.

### 2. Remove split TUI selection recovery state

Problem:
- The TUI maintains extra selection recovery state such as `selected_channel_id` mirrors and retry loops because the actual authoritative selection model is not strong enough.

### Tasks to fix

- [x] Remove duplicated selected-channel recovery state once authoritative selection is unified.
- [x] Delete background retry loops used only to recover lagging channel selection before send.
- [x] Remove optimistic navigation hacks that advance screens before semantic state has committed.
- [x] Collapse no-op compatibility callbacks that exist only to preserve old interfaces.

### Implementation guidance

- Simplify `crates/aura-terminal/src/tui/screens/app/shell.rs`, `.../shell/input.rs`, `.../shell/events.rs`, and `crates/aura-terminal/src/tui/callbacks/factories.rs` together so the selection model is changed once.
- Prefer deleting interfaces entirely over leaving compatibility callbacks that silently do nothing.

### Success criteria

- Message send and chat navigation no longer rely on fallback selection recovery.
- TUI dispatch code loses the retry/sync-warning branches that exist only to compensate for split state.
- The shell/callback wiring becomes simpler and more direct.

### 3. Remove harness-only execution branches from frontend code

Problem:
- The TUI currently contains harness-only runtime shortcuts for invitation flows and similar operations.

### Tasks to fix

- [x] Delete `AURA_HARNESS_MODE` execution-path branches from parity-critical frontend action handlers.
- [x] Remove runtime-bridge shortcut plumbing that exists only to support those harness-only branches.
- [x] Keep harness mode limited to observation, instrumentation, and deterministic environment settings.

### Implementation guidance

- Remove these branches only after the typed semantic backend path drives the real UI flow reliably.
- Search both `aura-terminal` and `aura-app` for harness-mode product-path behavior, not just the obvious shell handlers.

### Success criteria

- Harness mode no longer changes parity-critical product execution semantics.
- Frontend code loses harness-only shortcut branches and related plumbing.

### 4. Collapse onboarding special cases into the normal snapshot/publication path

Problem:
- Onboarding currently requires bespoke publication and stale-state recovery logic.

### Tasks to fix

- [x] Delete synthetic onboarding snapshot publication paths once onboarding uses the canonical semantic snapshot pipeline.
- [x] Remove stale-onboarding recovery logic from the web harness bridge.
- [x] Remove synthetic onboarding fallback handling from harness observation paths.

### Implementation guidance

- Change the frontend publication model first, then remove recovery code from `aura-web` and `aura-harness`.
- Do not leave stale compatibility branches behind “just in case”; gate the migration with tests and then delete the old path.

### Success criteria

- Onboarding no longer has a bespoke harness publication path.
- Web and harness code both lose onboarding-specific stale/fallback logic.

### 5. Remove settings-specific parity normalization and exception scaffolding where avoidable

Problem:
- The current parity layer normalizes settings IDs and drops incompatible sections because the two frontends diverge structurally.

### Tasks to fix

- [x] Delete `normalize_parity_item_id` special handling once settings IA and IDs are canonicalized.
- [x] Remove avoidable parity exceptions such as browser-only handling that exists only because of preventable frontend mismatch.
- [x] Delete frontend-local title-to-ID derivation for parity-critical settings surfaces.

### Implementation guidance

- Only keep explicit parity exceptions that correspond to deliberate, long-term environment-specific behavior.
- If a section is truly frontend-specific, classify it out of the parity-critical surface rather than normalizing around it.

### Success criteria

- The parity layer stops carrying settings-specific normalization hacks.
- Settings parity logic becomes smaller and more declarative.

### 6. Collapse observation fallback stacks and delete stale compatibility paths

Problem:
- The browser and TUI observation stacks still contain multiple semi-authoritative fallback layers.

### Tasks to fix

- [x] Delete fallback observation paths that are no longer needed once one authoritative projection model exists.
- [x] Remove any path that can silently return stale cached state for parity-critical observation.
- [x] Restrict raw DOM/PTY scraping to explicit diagnostics-only code paths.
- [x] Collapse renderer-heartbeat, semantic snapshot, and fallback reconciliation logic into one clear observation pipeline.

### Implementation guidance

- Treat browser and TUI separately at the transport layer, but unify the conceptual observation pipeline.
- Keep diagnostics capture available, but make it impossible for diagnostics-only paths to return success-oriented state.

### Success criteria

- The harness has one authoritative observation path per frontend.
- Fallback logic is minimal, explicit, and diagnostics-only.

### 7. Remove duplicated semantic action implementations across backends

Problem:
- Shared semantic actions are still implemented with materially different procedural logic in multiple places.

### Tasks to fix

- [x] Collapse shared semantic backend behavior onto one typed semantic backend interface per frontend.
- [x] Delete executor-side and backend-side duplicated choreography once the typed backend trait owns semantic intent execution.
- [x] Remove legacy semantic helper methods on backends that exist only as compatibility defaults.

### Implementation guidance

- Start from `crates/aura-harness/src/backend/mod.rs` and aggressively shrink the trait surface once the canonical shared semantic action interface is in place.
- Prefer removing unsupported generic methods instead of keeping a broad trait full of fallback/default behaviors.

### Success criteria

- Shared semantic execution is owned by a smaller, stricter backend interface.
- Duplicate semantic action procedures disappear from executor/backend layers.

### 8. Remove raw-mechanics support from shared scenario paths

Problem:
- The repo still carries raw-mechanics concepts, compatibility parsing, and scenario-level exceptions that should disappear from the shared suite.

### Tasks to fix

- [x] Delete legacy or compatibility scenario actions from shared-flow execution once the strict intent contract is fully rolled out.
- [x] Remove parser and executor branches that only exist to support old frontend-driving mechanics in shared scenarios.
- [x] Reclassify any remaining renderer-specific scenario as non-shared rather than preserving mixed semantics in the shared suite.

### Implementation guidance

- Keep non-shared renderer-specific scenarios explicit and quarantined.
- Remove dormant compatibility actions from code, not just from docs, once the inventory shows the shared suite no longer uses them.

### Success criteria

- The canonical shared scenario model no longer contains dead compatibility paths for raw UI mechanics.
- Shared scenario parsing and execution code becomes smaller and stricter.

### 9. Collapse script-level policy duplication into typed validators

Problem:
- The repo currently spreads harness/shared-flow/parity policy across multiple shell scripts, some of which overlap.

### Tasks to fix

- [x] Replace duplicated shell-script policy logic with typed Rust validators over the canonical contract/inventory models.
- [x] Reduce shell scripts to thin wrappers where retaining them preserves workflow stability.
- [x] Delete redundant script logic once typed validators fully cover the rule set.

### Implementation guidance

- Keep `just ci-*` entry points and workflow wiring stable while shrinking the shell layer under them.
- Consolidate by domain, not by script filename: one typed validator for scenario legality/intent, one for parity/coverage, etc.

### Success criteria

- Policy logic lives primarily in typed validators, not in scattered regex scripts.
- The number of shell scripts and duplicated checks decreases materially.

### 10. Simplify browser driver recovery and fallback behavior

Problem:
- The Playwright driver currently mixes action execution, observation, retries, stale-state recovery, and fallback interaction modes.

### Tasks to fix

- [x] Delete implicit recovery from observation methods once observation and recovery are split.
- [x] Remove fallback click/input modes that no longer serve a justified contract role.
- [x] Split the driver into smaller modules by responsibility and delete glue code that only existed to support the previous all-in-one design.

### Implementation guidance

- Do this before or during the TypeScript migration so the migration reflects the intended architecture instead of preserving the current monolith.
- Keep only the smallest explicit recovery surface needed for diagnosable failure handling.

### Success criteria

- The browser driver becomes smaller, more modular, and less self-healing.
- Observation code is passive and action code is explicit.

### 11. Remove stale parity, naming, and structure duplication

Problem:
- Some parity-related metadata and naming logic exists in multiple places today: docs, scripts, contract tables, and frontend-local conventions.

### Tasks to fix

- [x] Delete parity metadata duplication once one authoritative machine-readable source exists.
- [x] Remove frontend-local naming conventions for shared concepts once the contract owns them.
- [x] Delete obsolete structure-mirroring scripts or docs fragments after typed parity metadata fully replaces them.

### Implementation guidance

- Keep one authoritative source and make every other consumer derive from it.
- Prefer code generation or typed reads over manually mirrored tables where practical.

### Success criteria

- Shared-flow support, parity exceptions, and structure mappings have one source of truth.
- Manual mirrored metadata decreases substantially.

### 12. Remove obsolete tests, helpers, and compatibility shims after migration

Problem:
- As the pipeline becomes stricter, some tests and helpers that assert old fallback behavior or support compatibility modes will become dead weight.

### Tasks to fix

- [x] Delete tests that exist only to preserve legacy shared-scenario mechanics, old fallback observation semantics, or harness-only product paths.
- [x] Remove compatibility helper types, old “legacy/convenience” APIs used only by deprecated harness paths, and no-op adapters once callers are gone.
- [x] Prune comments and architecture notes that describe transitional behavior after the transition is complete.

### Implementation guidance

- Do not keep migration scaffolding indefinitely after the new path is stable.
- Every cleanup PR should ask: “what compatibility code became unnecessary because of this change?” and remove it in the same or immediately following PR.

### Success criteria

- Legacy and migration-only helpers do not linger after the new path is stable.
- The repo reflects one coherent harness/frontend pipeline rather than a cleaned-up core wrapped in old scaffolding.

### 13. Cleanup checkpoint

### Tasks to fix

- [x] After each major cleanup wave, run a dedicated deletion pass to remove:
  - dead methods
  - dead tests
  - dead scripts
  - stale docs text
  - compatibility comments
  - allowlists that no longer justify their own existence

### Implementation guidance

- Treat deletion as a first-class deliverable, not a nice-to-have.
- Tie each deletion pass to a completed contract-hardening milestone so the removal is safe and reviewable.

### Success criteria

- The codebase gets smaller as the harness pipeline gets stricter.
- The final architecture is not just more correct; it is also materially simpler and more coherent.

### 14. Delete legacy scenario/config compatibility code

Problem:
- `crates/aura-harness/src/config.rs` still carries a large legacy scenario/config surface, overloaded step structs, and conversion logic that exists to preserve multiple scenario models at once.

### Tasks to fix

- [x] Remove the legacy `ScenarioConfig` / `ScenarioStep` / `ScenarioAction` compatibility path once the semantic scenario file format fully owns shared scenarios.
- [x] Delete bidirectional conversion code between legacy and semantic scenario representations after the inventory and loader path are semantic-only.
- [x] Remove parser branches and executor branches that only exist to preserve backward compatibility for legacy shared-flow fields and actions.
- [x] Shrink `config.rs` so it owns inventory and semantic validation, not a second scenario language.

### Tasks to enforce go-forward

- [x] Add a typed validator that rejects new shared scenarios using the legacy compatibility schema.
- [x] Add a CI check in the existing `scripts/check/` + `just ci-*` pattern that fails when new legacy scenario types or conversion helpers are introduced in shared-flow code paths.
- [x] Keep non-shared migration fixtures explicit and quarantined so compatibility scaffolding cannot quietly re-enter the shared suite.

### Implementation guidance

- Start with `crates/aura-harness/src/config.rs` and `scenarios/harness_inventory.toml`.
- Land semantic-only inventory validation first, then remove unused compatibility parsing and conversion code in the same cleanup wave.
- Keep CI alignment with current repo conventions: fast schema/policy enforcement in `scripts/check/`, stable wrapper in `just`, and no new bespoke workflow shape unless the existing harness/docs workflows cannot absorb the check.

### Success criteria

- Shared scenario loading has one canonical semantic path.
- `config.rs` is materially smaller and no longer translates between parallel scenario models.
- The shared suite cannot regress back to the legacy schema without CI failing.

### 15. Shrink the backend trait and delete fallback-heavy shared execution APIs

Problem:
- `crates/aura-harness/src/backend/mod.rs` exposes a broad `InstanceBackend` trait with many default fallback methods for clicks, fills, list activation, semantic actions, and observation. That keeps too much compatibility behavior alive in the core surface.

### Tasks to fix

- [x] Replace the current broad shared backend surface with a smaller split between typed observation and typed action execution.
- [x] Delete default fallback methods that only exist to let unsupported backends compile while silently carrying dead behavior.
- [x] Move any genuinely debug-only or renderer-specific mechanics out of the shared semantic execution surface.
- [x] Remove executor paths that probe multiple backend methods for the same semantic intent.

### Tasks to enforce go-forward

- [x] Add a CI check that new shared semantic actions must be added through the canonical typed backend interface, not by introducing another raw backend escape hatch.
- [x] Add lint/search enforcement that raw selector/button/input helpers cannot be used from shared-flow execution paths once the strict interface lands.
- [x] Require any remaining non-shared raw backend helper to live in explicitly quarantined modules.

### Implementation guidance

- Use `crates/aura-harness/src/backend/mod.rs` as the contraction point.
- Prefer deleting methods entirely over keeping broad default implementations with `bail!` stubs.
- Keep enforcement in current CI shape: fast search/schema checks in `scripts/check/`, with heavier conformance behavior in existing harness CI lanes if needed.

### Success criteria

- Shared harness execution depends on a smaller, stricter backend contract.
- Backend trait sprawl and fallback default behavior decrease materially.
- New ad hoc backend escape hatches cannot land unnoticed.

### 16. Remove TUI override caches, placeholder exports, and synthetic runtime-event publication

Problem:
- `crates/aura-terminal/src/tui/harness_state.rs` and related subscriptions/screens still publish override-backed contacts/devices/messages and synthesize operations/runtime events from modal/UI heuristics.

### Tasks to fix

- [x] Delete `publish_contacts_list_override`, `publish_devices_list_override`, `publish_messages_override`, and the associated override caches once canonical post-render semantic export is in place.
- [x] Remove synthetic operation snapshots that are injected from modal state instead of exported from the real semantic operation model.
- [x] Remove heuristic runtime-event synthesis such as “contacts exist therefore contact link ready” and “member_count > 1 therefore recipient peers resolved”.
- [x] Eliminate placeholder/synthetic snapshot fallbacks in the TUI harness-export path.

### Tasks to enforce go-forward

- [x] Add snapshot-export invariant tests that fail if override caches, placeholder IDs, or inferred runtime-success facts reappear in parity-critical exports.
- [x] Add a CI check in the existing `scripts/check/` layer that flags new override-publish helpers in TUI harness-export code.
- [x] Require runtime-event export to map from authoritative semantic state only, with typed tests covering each parity-critical event family.

### Implementation guidance

- Focus first on `crates/aura-terminal/src/tui/harness_state.rs`, `crates/aura-terminal/src/tui/screens/app/subscriptions.rs`, `crates/aura-terminal/src/tui/screens/chat/screen.rs`, and `crates/aura-terminal/src/tui/screens/contacts/screen.rs`.
- Remove the exporter-side workaround only after the canonical signal/render/export pipeline can express the same information authoritatively.
- Keep CI consistent with existing conventions by putting invariant/search checks in `scripts/check/` and using existing harness lanes for any heavier semantic/export tests.

### Success criteria

- TUI harness export is derived from one authoritative semantic snapshot path.
- Override caches and heuristic runtime-event synthesis disappear from parity-critical export code.
- The TUI exporter becomes smaller, more transparent, and easier to reason about.

### 17. Unify channel selection and delete fallback dispatch glue

Problem:
- The TUI chat path still carries split selection state, selected-channel fallback IDs, index-based recovery, and retry glue that compensates for snapshot lag and inconsistent ownership of selection.

### Tasks to fix

- [x] Replace split channel-selection state with one canonical selection owner for render, dispatch, and export.
- [x] Remove `selected_channel_id` fallback plumbing and index-based channel recovery logic once selection is authoritative.
- [x] Delete retry-specific dispatch glue that only exists to reconstruct channel context after the fact.
- [x] Remove compatibility callbacks/adapters that are no-ops once the canonical channel-selection contract exists.

### Tasks to enforce go-forward

- [x] Add invariant tests that fail if dispatch resolves channels by row/index fallback in parity-critical flows.
- [x] Add a CI search/lint check that new shared-flow channel actions cannot read fallback selection caches outside the canonical selection owner.
- [x] Require parity-critical message actions to resolve against explicit semantic channel identity, not UI position.

### Implementation guidance

- Target `crates/aura-terminal/src/tui/screens/app/shell.rs`, `crates/aura-terminal/src/tui/screens/app/shell/input.rs`, `crates/aura-terminal/src/tui/screens/app/shell/events.rs`, and `crates/aura-terminal/src/tui/screens/chat/screen.rs`.
- Remove the fallback path only after the chat render/export pipeline agrees on a single selected channel identity.
- Prefer deleting shared `RwLock<Option<String>>` selection side channels instead of preserving them “just in case”.

### Success criteria

- Channel selection has one owner and one canonical identity.
- Row/index fallback and retry reconstruction logic are removed from parity-critical chat flows.
- TUI chat action handling becomes materially simpler.

### 18. Remove harness-only frontend behavior branches and startup shortcuts

Problem:
- Product frontend code still contains harness-mode branches and startup shortcuts that alter behavior instead of limiting harness mode to deterministic observation/instrumentation.

### Tasks to fix

- [x] Remove `AURA_HARNESS_MODE`-specific product flow branches from frontend runtime code once deterministic observation hooks exist.
- [x] Delete harness-only startup shortcuts that bypass normal UI convergence or normal user-facing workflow progression.
- [x] Move any remaining harness-specific behavior behind observation/instrumentation boundaries instead of product action handlers.
- [x] Remove props and plumbing that only exist to support harness-only frontend execution shortcuts.

### Tasks to enforce go-forward

- [x] Add a CI search check, following existing `scripts/check/` conventions, that forbids new `AURA_HARNESS_MODE` branches in product flow modules outside an allowlisted instrumentation surface.
- [x] Add tests that compare harness mode versus normal mode for parity-critical flows and fail if semantics diverge.
- [x] Keep any allowlist short, explicit, and reviewed as deletion debt rather than permanent architecture.

### Implementation guidance

- Start with `crates/aura-terminal/src/tui/screens/app/shell.rs`, `crates/aura-harness/src/provisioning.rs`, and any frontend module currently branching directly on harness env vars.
- Do not move special cases around; delete them after the underlying deterministic observation path exists.
- Keep enforcement in the current CI style: cheap structural checks in `scripts/check/`, behavior/conformance checks in existing harness lanes.

### Success criteria

- Harness mode changes determinism/observation, not product semantics.
- Frontend product modules stop carrying harness-only shortcut branches.
- The harness no longer depends on hidden product-path exceptions to stay green.

### 19. Collapse onboarding-specific publish, recovery, and fallback code

Problem:
- Onboarding currently has extra sleeps, stale-state recovery, and synthetic fallback behavior in both the web and harness layers instead of using the same canonical snapshot lifecycle as the rest of the UI.

### Tasks to fix

- [x] Delete stale-onboarding publish recovery logic from the web harness bridge once publish ordering and quiescence are authoritative.
- [x] Remove synthetic onboarding snapshot fallbacks from harness backends once onboarding publishes through the canonical post-render path.
- [x] Delete onboarding-only sleep/retry glue that exists to mask publication-order problems.
- [x] Unify onboarding export/publication with the same semantic snapshot and readiness rules used by post-onboarding screens.

### Tasks to enforce go-forward

- [x] Add a CI-backed contract test that onboarding must publish through the same authoritative snapshot path and revision/quiescence model as other screens.
- [x] Add a structural check that new onboarding-specific harness fallbacks cannot be introduced in `aura-web` or `aura-harness` without failing CI.
- [x] Require any onboarding exception to be typed, inventory-backed, and explicitly temporary if one ever becomes unavoidable.

### Implementation guidance

- Focus on `crates/aura-web/src/harness_bridge.rs`, `crates/aura-web/src/main.rs`, and `crates/aura-harness/src/backend/local_pty.rs`.
- Remove stale-state repair only after post-render authoritative publication is in place for onboarding.
- Route fast structural checks through `scripts/check/` and keep any heavier onboarding conformance tests in the existing harness workflow family.

### Success criteria

- Onboarding no longer needs bespoke recovery or synthetic fallback logic.
- Web and harness layers treat onboarding like any other screen in the semantic publication pipeline.
- Onboarding-specific flake-reduction code is materially reduced.

### 20. Remove parity normalization glue and shared-concept duplication

Problem:
- The pipeline still carries parity normalization helpers, frontend-local shared-concept enums, and one-off exception glue that should disappear once the shared UX contract fully owns IDs, IA, naming, and semantics.

### Tasks to fix

- [x] Delete parity normalization helpers such as frontend-specific item-ID remapping once IDs come from one canonical shared source.
- [x] Collapse duplicated shared-concept enums and identifiers, such as settings/navigation concepts, onto the shared contract layer.
- [x] Remove hardcoded parity exception maps that only exist to paper over frontend information-architecture drift.
- [x] Delete compatibility aliases once the last consumer is migrated to canonical shared naming.

### Tasks to enforce go-forward

- [x] Add CI validation that parity-critical IDs and shared enum values may only be introduced in the shared contract layer.
- [x] Add a search/lint check that frontend-local remapping helpers for parity-critical identifiers fail CI once the cleanup lands.
- [x] Require any new parity exception to be typed metadata with an explicit expiry/removal plan rather than inline normalization logic.

### Implementation guidance

- Start with `crates/aura-app/src/ui_contract.rs`, `crates/aura-ui/src/model.rs`, `crates/aura-terminal/src/tui/types.rs`, and any parity-comparison helper still normalizing IDs after export.
- Prefer deleting exception glue rather than migrating it to a new location.
- Keep enforcement in the existing CI pattern by making shared-contract ownership a fast validator rule under `scripts/check/`.

### Success criteria

- Shared IDs, naming, and IA concepts have one owner.
- Frontend-local parity normalization glue disappears from the steady-state design.
- Parity comparison becomes simpler because the frontends are actually aligned.

### 21. Delete browser-driver self-healing code after observation and action are separated

Problem:
- The browser driver currently compensates for structural issues with stale-state repair, implicit retries, navigation recovery, and fallback interaction modes that should not survive the stricter architecture.

### Tasks to fix

- [x] Remove stale-state recovery from observation paths once observation becomes read-only and revisioned.
- [x] Delete fallback click/input/navigation paths that no longer belong in the canonical shared execution path.
- [x] Split the driver into smaller modules and remove glue code that only coordinated retries, recovery, and fallback mechanics inside one monolith.
- [x] Remove duplicated shape-normalization code once the frontend export contract is strict enough to consume directly.

### Tasks to enforce go-forward

- [x] Add CI checks that new browser-driver retries/recovery helpers cannot appear in observation modules.
- [x] Add conformance tests proving observation is passive and that failures surface diagnostically instead of being auto-repaired in the success path.
- [x] Keep any remaining explicit recovery isolated to one bounded module with tests and clear ownership.

### Implementation guidance

- Start from `crates/aura-harness/playwright-driver/playwright_driver.mjs` and keep the repo’s current CI conventions: structural checks in `scripts/check/`, heavier driver conformance in the existing harness workflow family.
- If the driver is later migrated to TypeScript, do the cleanup first or concurrently so the migration does not preserve the current monolithic fallback architecture.
- Treat every deleted retry/fallback path as a real deliverable, not incidental cleanup.

### Success criteria

- Browser-driver observation is passive and explicit.
- Success-path self-healing logic is materially reduced or eliminated.
- The driver is smaller, more modular, and easier to diagnose.

### 22. Collapse duplicated semantic choreography across TUI and browser backends

Problem:
- Shared semantic workflows are still duplicated across local PTY and Playwright backends, with each backend carrying its own waits, fallbacks, and workflow choreography.

### Tasks to fix

- [x] Extract shared semantic action sequencing into one canonical harness layer wherever backend differences are only in primitive UI interaction.
- [x] Delete backend-local copies of shared-flow choreography once the common semantic layer owns those flows.
- [x] Keep backend-specific code limited to renderer primitives and authoritative observation, not business-level workflow sequencing.
- [x] Remove compatibility helper methods that remain only because both backends evolved separate semantic stacks.

### Tasks to enforce go-forward

- [x] Add CI review/search checks that new shared-flow semantic procedures are not copied into multiple backend implementations without an explicit architectural exception.
- [x] Add tests that shared semantic flows execute through the common layer and only diverge at backend primitive boundaries.
- [x] Require any backend-specific shared-flow exception to be typed, justified, and inventory-visible.

### Implementation guidance

- Use `crates/aura-harness/src/backend/local_pty.rs`, `crates/aura-harness/src/backend/playwright_browser.rs`, and the executor/backend boundary as the consolidation points.
- Move common sequencing up, not more fallback logic down.
- Keep CI aligned with repo norms: lightweight duplication/pattern checks in `scripts/check/`, deeper cross-backend conformance in the existing harness CI lanes.

### Success criteria

- Shared semantic workflows are implemented once.
- Backend code shrinks toward primitives plus observation.
- Cross-backend divergence becomes explicit and rare.

### 23. Remove obsolete shell scripts and duplicated governance once typed validators are complete

Problem:
- Some cleanup work will leave shell wrappers, duplicated policy scripts, mirrored metadata, and docs fragments behind even after the typed validators become authoritative.

### Tasks to fix

- [x] Delete redundant policy logic from shell scripts once the equivalent typed validator is live and wired into CI.
- [x] Remove mirrored metadata files, copied rule tables, and stale governance fragments that become unnecessary after validator centralization.
- [x] Collapse surviving shell scripts into thin wrappers only where workflow stability still benefits from them.
- [x] Delete superseded comments and architecture notes that describe the pre-cleanup governance shape.

### Tasks to enforce go-forward

- [x] Add a maintenance rule in CI that new policy checks should extend the typed validator domain first, not create another bespoke shell script, unless there is a clear workflow-level reason.
- [x] Keep wrapper scripts small enough that divergence from the typed validator logic is easy to detect in review.
- [x] Add a docs check that references the authoritative validator entry point rather than duplicating policy prose in multiple places when possible.

### Implementation guidance

- Start with the overlapping harness/parity/coverage scripts in `scripts/check/`.
- Preserve current workflow and `just ci-*` entry points while removing duplicated logic underneath them.
- Keep docs/workflow updates aligned with the existing `docs.yml` and harness workflow structure instead of introducing a parallel CI taxonomy.

### Success criteria

- Governance logic is centralized and typed.
- Shell wrappers are thin and stable.
- Duplicated rule text and policy code decrease materially.

### 24. Prune migration-only tests, helpers, allowlists, and comments

Problem:
- After the architecture hardening work lands, the repo will still contain migration scaffolding unless cleanup explicitly removes old tests, helper APIs, allowlists, and explanatory comments for behaviors that no longer exist.

### Tasks to fix

- [x] Delete tests that only preserve legacy shared-scenario mechanics, old fallback observation semantics, or harness-only product behavior.
- [x] Remove migration-only helper types, no-op adapters, deprecated API surfaces, and transitional allowlists once callers are gone.
- [x] Delete comments and architecture notes that describe transitional fallback paths after those paths are removed.
- [x] Run an explicit post-milestone deletion pass after each major contract-hardening wave.

### Tasks to enforce go-forward

- [x] Add a cleanup checklist item to the relevant architecture/docs/agent guidance so each migration PR asks what compatibility code became unnecessary and removes it.
- [x] Add CI checks where practical to keep allowlists and exception metadata from growing without review or expiry.
- [x] Require every temporary compatibility surface to carry an owner or removal condition so it can be audited and deleted.

### Implementation guidance

- Treat deletion as part of the feature/remediation scope, not a later polish pass.
- Keep CI and docs updates aligned with existing repo conventions rather than creating a new cleanup-specific workflow.
- When removing allowlists or exceptions, clean both code and docs in the same change set so the repo does not retain stale narrative debt.

### Success criteria

- Migration scaffolding does not persist after the new path stabilizes.
- The repository reflects one coherent steady-state harness pipeline.
- Code, tests, docs, and agent guidance shrink along with the fallback surface.
