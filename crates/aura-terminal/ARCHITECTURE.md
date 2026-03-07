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
- Parse/validation failures for identifiers and thresholds are handled at UI/input boundaries.
- Internal command and modal contracts should prefer typed IDs/domain values over raw `String`.
- Operational responses for core flows should use structured variants instead of free-form strings.

### Detailed Specifications

### InvariantTerminalUiBoundary
Terminal interfaces must remain a presentation layer over aura-app and must not introduce runtime effect implementations.

Enforcement locus:
- src tui and command handlers map user intents to app workflows.
- User interface state changes are derived from reactive app signals.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just check-arch and just test-crate aura-terminal

Contract alignment:
- [Aura System Architecture](../../docs/001_system_architecture.md) defines interface layer boundaries.
- [Effect System and Runtime](../../docs/105_effect_system.md) defines signal and workflow integration.
## Boundaries
- Business logic lives in aura-app.
- Effect implementations live in aura-effects.
- Runtime composition lives in aura-agent.
