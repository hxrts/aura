# Aura Web (Layer 7)

## Purpose

Browser/WASM shell for Aura. Remains thin and delegates shared UI state, routing, and snapshot rendering to `aura-ui`.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Bootstrap Aura runtime for browser/WASM environments | Shared UI logic ownership (lives in aura-ui) |
| Mount the shared `aura-ui` Dioxus root | Effect trait or runtime handler ownership |
| Browser-specific adapters (clipboard, JS harness bridge) | Domain/protocol logic ownership |
| `window.__AURA_HARNESS__` for Playwright-driven automation | Parity-critical semantic lifecycle ownership |
| Semantic `UiSnapshot` and `RenderHeartbeat` publication in harness mode | |
| Typed semantic command bridge for shared-flow execution | |

## Dependencies

| Direction | Crate | What is consumed / produced |
|-----------|-------|-----------------------------|
| Consumes | `aura-ui` | Shared Dioxus root, UI state, routing, snapshot rendering |
| Consumes | `aura-app` | `AppCore`, `ui_contract`, workflow types |
| Consumes | `aura-core` | Types, identifiers |
| Produces | — | Browser shell, harness bridge surface, semantic projections |

## Invariants

- Browser-only APIs stay in this crate.
- Shared UI behavior remains in `aura-ui`.
- Browser task ownership reuses the shared frontend task-owner implementation
  from `aura-ui` rather than keeping a forked cancellation/spawner stack.
- Harness bridge methods are deterministic and backwards-compatible.
- Bridge responses that claim to return a channel binding must originate from authoritative selected-channel context materialization; selected ids without context are not a binding.
- Browser bootstrap handoff stays explicit: runtime identity is staged through
  the dedicated `stage_runtime_identity` bridge entrypoint rather than through
  ambient storage or a generic bootstrap trigger.
- Harness publication is semantic-first: pushed shared-contract state and render heartbeat are authoritative; DOM inspection is secondary diagnostics only.
- Browser/DOM fallback paths are diagnostic-only and must not become parity-critical success-path observation.
- Harness mode may change instrumentation and render stability, but not business-flow semantics.
- Shared browser-flow execution must go through the semantic command bridge and real app workflows; DOM clicks and selector helpers are frontend-conformance-only.
- Playwright-to-page semantic submission uses the page-owned semantic queue
  (`window.__AURA_DRIVER_SEMANTIC_ENQUEUE__`) so the driver does not own
  browser semantic lifecycle or runtime/bootstrap state.
- Long-lived browser maintenance tasks must surface terminal pause/failure
  through observed UI state or equivalent structured browser signals rather than
  relying on console logging alone.
- The browser shell is an `Observed` plus bridge crate for shared semantic flows. It may submit commands and expose projections, but it must not own terminal semantic lifecycle truth for parity-critical operations.

### InvariantBrowserHarnessBridgePublishesSemanticState

The browser shell exports structured observed semantic UI projections and render convergence signals for harness observation.

Enforcement locus:
- `src/harness_bridge.rs` publishes `UiSnapshot` and `RenderHeartbeat`.
- `src/main.rs` wires harness-mode startup and publication hooks.

Failure mode:
- Browser harness runs must infer state from DOM text or ad hoc JS scraping.
- Post-action hangs cannot be attributed to semantic state vs render convergence.
- State reads silently repair stale state or hide observation side effects.

Verification hooks:
- Browser harness contract tests
- Playwright semantic bridge tests

### InvariantBrowserSharedFlowExecutionUsesSemanticBridge

The browser shell accepts shared semantic commands through an explicit bridge and executes them through real app workflows.

Enforcement locus:
- `src/harness_bridge.rs` owns typed command submission and unsupported-command failures.
- Selector-click helpers remain outside the shared semantic path.

Failure mode:
- Shared browser scenarios debug DOM/selector timing instead of production workflows.
- Browser and TUI shared lanes drift because they do not share one execution contract.

Verification hooks:
- Browser harness contract tests
- Playwright semantic bridge tests

## Ownership Model

Reference: [docs/122_ownership_model.md](../../docs/122_ownership_model.md)

For shared semantic flows, `aura-web` uses `Observed` for browser-side projection publication, render-convergence publication, and compatibility/version metadata. Narrow `ActorOwned` bridge ownership is permitted for browser bridge installation and command ingress (long-lived mutable async browser surfaces). The browser shell must not own terminal semantic lifecycle truth; browser code must not allocate local semantic owners for parity-critical operations or keep browser-local `OperationState::Submitting` state after handoff to shared workflow ownership per docs/122 section 16.

### Inventory

| Path | Category | Authoritative owner | May mutate | Observe only |
|------|----------|---------------------|------------|--------------|
| Browser harness bridge installation and command ingress | `ActorOwned` | `aura-web::harness_bridge` bridge loop | bridge ingress/installation code | browser render layer, harness |
| Browser semantic lifecycle rendering | `Observed` | authoritative semantic facts from `aura-app` | browser presentation state only | harness, user-visible rendering |
| Render-convergence and projection publication | `Observed` | browser projection/export path | bridge/publication code only | Playwright/harness |
| Web onboarding/bootstrap command helpers for shared flows | `Observed` shell over upstream `MoveOwned`/`ActorOwned` coordination | shared workflow/runtime coordinators | browser-local UI state only; never terminal truth | harness, DOM/render readers |
| Shared browser task-owner cancellation/spawn mechanics | `ActorOwned` helper reused from `aura-ui::task_owner` | shared frontend task-owner implementation | browser shell wiring only | harness, render layer |

### Capability-Gated Points

- Harness bridge command ingress and compatibility-gated semantic command execution in `src/harness_bridge.rs`.
- Browser-side semantic projection and render-heartbeat publication in `src/harness_bridge.rs`.
- Browser clipboard and bootstrap adapters that may trigger upstream workflows, but may not author terminal semantic lifecycle locally.

### Verification Hooks

- `cargo check -p aura-web`
- Browser harness contract tests
- Playwright semantic bridge tests
- `just ci-observed-layer-boundaries`
- `just ci-frontend-handoff-boundary`
- `just ci-actor-lifecycle`

### Compatibility Policy

- The harness bridge request/response surface carries explicit compatibility metadata so callers can detect versioned behavior changes.
- Explicit bridge entrypoints currently include semantic command submission,
  observed snapshot/render publication, and runtime identity staging for
  owned browser rebootstrap during create-account style flows.
- Additive fields and additive non-breaking methods are allowed when old callers continue to observe the same behavior.
- Breaking request/response or observation-shape changes must update explicit compatibility metadata and tests.

## Testing

### Strategy

Browser harness bridge correctness, semantic projection publication, and shared-flow execution parity are the primary concerns.

### Commands

```
cargo check -p aura-web
```

### Coverage matrix

| What breaks if wrong | Invariant | Test location | Status |
|---------------------|-----------|--------------|--------|
| Harness bridge publishes wrong state | BrowserHarnessBridgePublishesSemanticState | Browser harness contract tests | Covered |
| Shared flow uses selector instead of bridge | BrowserSharedFlowExecutionUsesSemanticBridge | Playwright semantic bridge tests | Covered |
| Render convergence signals wrong | BrowserHarnessBridgePublishesSemanticState | Playwright driver self-test | Covered |

## References

- [Aura System Architecture](../../docs/001_system_architecture.md)
- [Ownership Model](../../docs/122_ownership_model.md)
- [Testing Guide](../../docs/804_testing_guide.md)
- [Project Structure](../../docs/999_project_structure.md)
