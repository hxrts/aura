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

## Non-Goals

- No browser API usage (`web_sys`, `wasm_bindgen`, `js_sys`, etc.).
- No desktop/mobile shell integration code.
- No runtime/effect handler implementation ownership.

## Invariants

- Shared core remains platform agnostic; shell crates own platform interop.
- Snapshot output remains deterministic for equivalent state and key streams.
- Keyboard routing is centralized and side-effect order is deterministic.
