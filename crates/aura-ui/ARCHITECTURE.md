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

Failure mode:
- Harness execution requires per-screen or per-frontend special cases.
- Shared scenarios regress back to raw mechanics.

Verification hooks:
- `just ci-shared-flow-policy`
