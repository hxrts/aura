# Aura Web Architecture

## Purpose

`aura-web` is the Layer 7 web shell for Aura.
It remains thin and delegates shared UI state, routing, and snapshot rendering to
`aura-ui`.

## Responsibilities

- Bootstrap Aura runtime for browser/WASM environments.
- Mount the shared `aura-ui` Dioxus root.
- Provide browser-specific adapters (clipboard, JS harness bridge).
- Expose `window.__AURA_HARNESS__` for Playwright-driven automation.
- Publish semantic `UiSnapshot` and `RenderHeartbeat` data in harness mode for
  browser-side observation.
- Expose a typed semantic command bridge for shared-flow execution in harness
  mode.

## Non-Goals

- No shared UI logic ownership.
- No effect trait or runtime handler ownership.
- No domain/protocol logic ownership.

## Invariants

- Browser-only APIs stay in this crate.
- Shared UI behavior remains in `aura-ui`.
- Harness bridge methods are deterministic and backwards-compatible.
- Harness publication is semantic-first: pushed shared-contract state and render
  heartbeat are authoritative; DOM inspection is secondary diagnostics only.
- `src/main.rs` owns harness instrumentation installation for browser startup;
  `src/harness_bridge.rs` owns the explicit bridge surface, with passive
  observation kept separate from action/recovery behavior.
- Browser/DOM fallback paths are diagnostic-only and must not become
  parity-critical success-path observation.
- Browser harness observation must be side-effect free; retries and recovery are
  explicit behaviors, not part of reading state.
- Harness mode may change instrumentation and render stability, but not
  business-flow semantics.
- Shared browser-flow execution must go through the semantic command bridge and
  real app workflows rather than selector-driving as the primary substrate.
- DOM clicks and selector helpers are frontend-conformance-only and must not be
  the shared semantic execution path.
- Published semantic state must support stale-state detection through shared
  revision/sequence and render-convergence semantics.
- Onboarding uses the same semantic snapshot/publication path as every other
  screen.
- The browser shell is an `Observed` plus bridge crate for shared semantic
  flows. It may submit commands and expose projections, but it must not own
  terminal semantic lifecycle truth for parity-critical operations.

## Ownership Model

For shared semantic flows, `aura-web` should use:

- `Observed`
  - browser-side projection publication
  - render-convergence publication
  - compatibility/version metadata for the harness bridge
- narrow `ActorOwned` bridge ownership only where necessary
  - browser bridge installation and command ingress may be long-lived mutable
    async browser surfaces

It must not use:

- browser-local semantic lifecycle ownership
- selector/DOM-driven readiness authorship
- browser-only owner fields that shadow authoritative operation ownership

The correct split is:

- browser bridge code is allowed to own bridge mechanics
- authoritative semantic lifecycle remains in shared workflow/runtime
  coordinators
- DOM helpers stay conformance-only and downstream of semantic ownership

### Ownership Inventory

| Path | Category | Authoritative owner | May mutate | Observe only |
|------|----------|---------------------|------------|--------------|
| Browser harness bridge installation and command ingress | `ActorOwned` | `aura-web::harness_bridge` bridge loop | bridge ingress/installation code | browser render layer, harness |
| Browser semantic lifecycle rendering | `Observed` | authoritative semantic facts from `aura-app` | browser presentation state only | harness, user-visible rendering |
| Render-convergence and projection publication | `Observed` | browser projection/export path | bridge/publication code only | Playwright/harness |
| Web onboarding/bootstrap command helpers for shared flows | `Observed` shell over upstream `MoveOwned`/`ActorOwned` coordination | shared workflow/runtime coordinators | browser-local UI state only; never terminal truth | harness, DOM/render readers |

### InvariantBrowserHarnessBridgePublishesSemanticState
The browser shell exports structured semantic UI state and render convergence
signals for harness observation.

Enforcement locus:
- `src/harness_bridge.rs` publishes `UiSnapshot` and `RenderHeartbeat`.
- `src/main.rs` wires harness-mode startup and publication hooks.

Failure mode:
- Browser harness runs must infer state from DOM text or ad hoc JS scraping.
- Post-action hangs cannot be attributed to semantic state vs render
  convergence.
- State reads silently repair stale state or hide observation side effects.

### InvariantBrowserSharedFlowExecutionUsesSemanticBridge
The browser shell accepts shared semantic commands through an explicit bridge and
executes them through real app workflows.

Enforcement locus:
- `src/harness_bridge.rs` owns typed command submission and unsupported-command
  failures.
- selector-click helpers remain outside the shared semantic path.

Failure mode:
- shared browser scenarios debug DOM/selector timing instead of production
  workflows.
- browser and TUI shared lanes drift because they do not share one execution
  contract.

Verification hooks:
- browser harness contract tests
- Playwright semantic bridge tests

Verification hooks:
- Playwright driver self-test
- browser harness contract tests

Compatibility policy:
- the harness bridge request/response surface carries explicit compatibility
  metadata so callers can detect versioned behavior changes
- additive fields and additive non-breaking methods are allowed when old callers
  continue to observe the same behavior
- breaking request/response or observation-shape changes must update explicit
  compatibility metadata and tests
