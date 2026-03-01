# Aura MPST (Layer 2) - Architecture and Invariants

## Purpose
Runtime library for choreographic protocol specifications and multi-party session
types. Provides semantic abstractions integrating with Telltale for protocol-level
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
- Re-exports of Telltale choreography/runtime functionality.

## Invariants
- Depends only on aura-core.
- No handler implementations or composition.
- Extensions handled externally via aura-macros.
- Provides the same `choreography!` macro interface over Telltale.

### Detailed Specifications

### InvariantMpstProjectionSafety
Global-to-local projection must preserve communication safety and guard annotation ordering.

Enforcement locus:
- src projection and runtime types encode role-local protocol progression.
- Session transitions preserve send and receive duality constraints.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just test-crate aura-mpst

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines MPST safety and duality.
- [MPST and Choreography](../../docs/108_mpst_and_choreography.md) defines runtime projection rules.
## Boundaries
- No multi-party coordination logic (only types and runtime abstractions).
- Protocol implementations belong in feature crates (Layer 5).
- Macro parsing belongs in aura-macros.

