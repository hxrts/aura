# Harness UX Determinism Design Note

This note defines the determinism rules for parity-critical shared UX flows.
It supplements [Testing Guide](804_testing_guide.md).
It is the design reference for the shared harness contract, observation rules, and browser freshness model.

## 1. Scope

This note applies to parity-critical shared flows that run through `aura-harness`.
It covers the TUI, the web frontend, and the shared contract in `aura-app::ui_contract`.
It does not replace feature-specific product behavior docs.

## 2. Shared contract boundary

Shared scenarios are actor-based and frontend-neutral.
They define semantic roles and semantic intents.
Frontend and runtime binding belongs to config, lane, and matrix files.

The shared contract must define action requests, action handles, transition facts, terminal facts, revision metadata, and canonical trace events.
Shared scenarios may not encode frontend identity, selectors, modal choreography, or row-index addressing.

## 3. Real UI path requirement

The harness must exercise the real user-visible flow.
Typed semantic intents do not authorize direct workflow shortcuts for parity-critical validation.
Backend adapters may translate a semantic intent into frontend-specific clicks, keys, fields, and modal flow.

The executor owns semantic sequencing.
Backend adapters own renderer mechanics only.
This keeps parity-critical flows on the real UI path while preserving one shared contract.

## 4. Authoritative observation model

`UiSnapshot` is the authoritative semantic observation surface.
It must be post-render and revisioned.
Parity-critical waits must resolve against typed readiness, typed runtime facts, or quiescence.

Observation endpoints must be side-effect free.
Recovery, retries, and DOM or text fallbacks are diagnostic paths only.
They must not become success-path observation behavior.

## 5. Revisions and stale-state rules

Every parity-critical semantic transition must advance monotonic revision metadata.
Browser post-action observation must require a strictly newer semantic snapshot than the pre-action baseline.
Render heartbeat divergence must be treated as stale state, not as a reason to guess.

Browser cache invalidation is owned by one lifecycle-aware module.
Session start, authority switch, device import, storage reset, and navigation recovery are declared lifecycle boundaries.
Flow-specific cache reset logic is not allowed.

## 6. Canonical traces

Every shared scenario run must record canonical action, transition, and terminal events.
Trace conformance is part of parity validation.
A shared flow is not deterministic unless repeated runs with the same seed produce the same semantic trace shape.

## 7. Recovery ownership

Fallback behavior must be explicit.
Each parity-critical recovery path must be registered in typed metadata with an owner module and a stable code.
Inline fallback behavior outside the approved recovery owner modules is not allowed.

## 8. Harness mode constraints

Harness mode may add instrumentation, deterministic effect routing, and render-stability hooks.
Harness mode may not change product business semantics.
If a harness-only branch changes the meaning of a parity-critical flow, that branch is a bug.
