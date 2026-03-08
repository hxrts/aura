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

Verification hooks:
- Playwright driver self-test
- browser harness contract tests
