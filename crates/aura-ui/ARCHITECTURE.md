# Aura UI Architecture

## Purpose

`aura-ui` is the Layer 7 shared Dioxus UI core for Aura.
It provides platform-agnostic UI state, deterministic key routing, and canonical
text snapshot rendering used by harness automation.

## Responsibilities

- Host shared Dioxus component tree and UI state model.
- Provide deterministic keyboard routing for harness-driven scenarios.
- Render canonical snapshot text compatible with harness introspection.
- Expose platform-neutral harness bridge primitives and clipboard adapter boundary.
- Materialize the shared semantic UI contract from `aura-app` into typed screen,
  modal, operation, toast, list, and runtime-event state.

## Non-Goals

- No browser API usage (`web_sys`, `wasm_bindgen`, `js_sys`, etc.).
- No desktop/mobile shell integration code.
- No runtime/effect handler implementation ownership.

## Invariants

- Shared core remains platform agnostic; shell crates own platform interop.
- Snapshot output remains deterministic for equivalent state and key streams.
- Keyboard routing is centralized and side-effect order is deterministic.
- Shared state is keyed by semantic ids and typed operation/runtime-event
  snapshots rather than frontend-local row indexes or renderer-only state.
- Shared screen and modal structure remains stable enough for semantic harness
  execution and render-convergence checks.
- Parity-critical IDs, focus semantics, and action shapes are consumed from
  `aura-app::ui_contract`; they are not locally reinvented here.
- Published semantic state must support stale-state detection through shared
  revision/sequence and render-convergence semantics.
- Onboarding must publish through the same semantic snapshot path as every
  other screen.
- `aura-ui` is an `Observed` shared UI core for parity-critical semantic flows.
  It may render lifecycle and submit frontend-local UI transitions, but terminal
  semantic truth stays in authoritative workflow/runtime ownership upstream.

## Ownership Model

For shared semantic flows, `aura-ui` should use:

- `Observed`
  - semantic snapshot rendering
  - operation/runtime-event presentation state
  - keyboard/focus/modal state
- narrow `ActorOwned` shell ownership only at the frontend boundary
  - shell crates may own command ingress/event-loop mechanics around `aura-ui`

It must not use:

- shared-UI-local terminal lifecycle authorship for parity-critical operations
- callback-owned readiness synthesis
- browser-only or TUI-only owner fields shadowing authoritative operation state

### Ownership Inventory

| Path | Category | Authoritative owner | May mutate | Observe only |
|------|----------|---------------------|------------|--------------|
| Semantic snapshot shaping and projection export | `Observed` | authoritative facts and shared UI state reducers | `model.rs` presentation state only | harness, web shell, users |
| Keyboard/focus/modal state | `Observed` | shared UI model | `keyboard.rs`, `model.rs` | harness, shells |
| Parity-critical operation rendering | `Observed` | authoritative semantic facts from `aura-app` | `model.rs` projection state only | harness, shells |
| Shared-flow completion helpers | `Observed` | upstream workflow/runtime coordinators | auxiliary UI facts/toasts/modal dismissal only | harness, shells |

### InvariantUiSnapshotReflectsSemanticState
`aura-ui` publishes semantic state that matches the shared contract rather than
frontend-local incidental structure.

Enforcement locus:
- `model.rs` owns typed selection, operation, toast, and runtime-event state.
- `semantic_snapshot()` exports the canonical `UiSnapshot`.

Failure mode:
- Harness assertions depend on renderer text or row order instead of semantic
  ids.
- Browser and TUI drift in observable state despite sharing the same flows.

Verification hooks:
- `cargo test -p aura-ui semantic_snapshot_includes_runtime_events`
- `cargo test -p aura-ui restarting_operation_generates_new_operation_instance_id`

### InvariantSharedFlowShapesAreUniform
Shared screens and modals expose consistent semantic structure for frontends
and the harness.

Enforcement locus:
- shared modal/button/field ids are driven from the `aura-app` contract.
- keyboard and click flows resolve through shared control ids and typed modal
  state.

Uniform shared-flow shape means:
- canonical ids for screens, modals, lists, controls, and operations
- shared focus and selection semantics
- stable list/entity shape keyed by semantic ids rather than renderer order
- shared operation and runtime-event shape at the `UiSnapshot` boundary

Failure mode:
- Harness execution requires per-screen or per-frontend special cases.
- Shared scenarios regress back to raw mechanics.

Verification hooks:
- `just ci-shared-flow-policy`
