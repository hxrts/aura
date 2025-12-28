# Aura Terminal (Layer 7) - Architecture and Invariants

## Purpose
Terminal-based CLI and TUI interfaces for account management, authentication,
recovery, and diagnostics. Uses AppCore as unified backend while remaining
platform-agnostic.

## Inputs
- aura-app (pure application core: `AppCore`, `Intent`, `ViewState`).
- aura-agent (runtime layer: `AuraAgent`, `EffectContext`, services).
- aura-core (types only: errors, identifiers, execution modes).

## Outputs
- CLI handlers and command implementations.
- TUI screens, components, and layouts.
- Terminal-specific rendering and input handling.
- Human-friendly error messages and visualization.

## Invariants
- Must NOT create effect implementations or handlers.
- Must NOT be imported by Layer 1-6 crates.
- Uses dependency inversion: imports from both aura-app and aura-agent.
- Terminal-specific rendering must stay in this layer.

## Boundaries
- Business logic lives in aura-app.
- Effect implementations live in aura-effects.
- Runtime composition lives in aura-agent.
