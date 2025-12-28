# Aura MPST (Layer 2) - Architecture and Invariants

## Purpose
Runtime library for choreographic protocol specifications and multi-party session
types. Provides semantic abstractions integrating with rumpsteak-aura for protocol-level
guards and Aura-specific extensions.

## Inputs
- Session type definitions from choreography protocols.
- Guard chain integration requirements (capability, journal coupling, leakage).
- Aura-specific annotations parsed by aura-macros.

## Outputs
- Session type runtime abstractions (`LocalSessionType`).
- Journal coupling types (`JournalCoupling`, `JournalAnnotation`).
- Guard chain integration traits.
- Choreography error types (`MpstError`).
- Re-exports of full rumpsteak-aura functionality.

## Invariants
- Depends only on aura-core.
- No handler implementations or composition.
- Extensions handled externally via aura-macros.
- Provides same `choreography!` macro interface as rumpsteak-aura.

## Boundaries
- No multi-party coordination logic (only types and runtime abstractions).
- Protocol implementations belong in feature crates (Layer 5).
- Macro parsing belongs in aura-macros.
