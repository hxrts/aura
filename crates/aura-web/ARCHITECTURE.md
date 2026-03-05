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

## Non-Goals

- No shared UI logic ownership.
- No effect trait or runtime handler ownership.
- No domain/protocol logic ownership.

## Invariants

- Browser-only APIs stay in this crate.
- Shared UI behavior remains in `aura-ui`.
- Harness bridge methods are deterministic and backwards-compatible.
