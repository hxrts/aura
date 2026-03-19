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

## Ownership Model

- `aura-mpst` is primarily `Pure`.
- It defines protocol/specification structure rather than `ActorOwned` runtime
  ownership.
- Session/delegation transfer semantics consumed by generated protocols should
  remain explicit and `MoveOwned` in higher layers.
- Capability or authority requirements should be carried in typed protocol
  artifacts, not hidden in runtime-local conventions.
- `Observed` tooling may inspect generated artifacts but not author protocol
  truth.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `src/projection.rs`, `src/protocol.rs`, `src/types.rs`, `src/guards.rs` | `Pure` | Protocol/specification semantics and projection rules only. |
| `src/runtime.rs`, endpoint/session descriptors, continuation types | `MoveOwned` | Session endpoints and protocol continuations are value-level handoff surfaces consumed by higher layers. |
| `src/ast_extraction.rs` | `Pure` | Annotation parsing and typed choreography metadata extraction. |
| Actor-owned runtime state | none | Live protocol execution ownership belongs in higher layers using these types. |
| Observed-only surfaces | none | Tooling can inspect generated artifacts but does not own protocol truth. |

### Capability-Gated Points

- typed protocol annotations and guard metadata consumed by higher-layer
  capability-gated mutation/publication paths

### Verification Hooks

- `cargo check -p aura-mpst`
- `cargo test -p aura-mpst -- --nocapture`

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
- [MPST and Choreography](../../docs/110_mpst_and_choreography.md) defines runtime projection rules.
## Testing

### Strategy

aura-mpst defines session types and choreographic extensions. If annotations
are lost or reordered, the guard chain executes in the wrong sequence —
capability checks may happen after journal commits.

### Running tests

```
cargo test -p aura-mpst --test protocols  # annotation and extension contracts
cargo test -p aura-mpst --lib             # inline unit tests
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Guard capability annotation lost | `tests/protocols/annotation_extraction.rs` | covered |
| Leak annotation lost | `tests/protocols/annotation_extraction.rs` | covered |
| Multiple annotations reordered | `tests/protocols/annotation_extraction.rs` | covered |
| Extension registry creation fails | `tests/protocols/extension_types.rs` | covered |
| Extension types can't be composed | `tests/protocols/extension_types.rs` | covered |
| Composite extension ordering wrong | `tests/protocols/extension_types.rs` | covered |
| Extension field values silently lost | `tests/protocols/extension_types.rs` | covered |
| Core types unavailable via re-exports | `tests/protocols/extension_types.rs` | covered |

## Boundaries
- No multi-party coordination logic (only types and runtime abstractions).
- Protocol implementations belong in feature crates (Layer 5).
- Macro parsing belongs in aura-macros.
