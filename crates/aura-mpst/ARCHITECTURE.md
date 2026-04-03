# Aura MPST (Layer 2)

## Purpose

Aura-owned boundary library for choreographic protocol specifications and multi-party session types. Provides semantic abstractions over Telltale's public language/runtime/type surfaces plus Aura-specific extensions.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Session type runtime abstractions (`LocalSessionType`) | Multi-party coordination logic (only types and runtime abstractions) |
| Journal coupling types (`JournalCoupling`, `JournalAnnotation`) | Protocol implementations (belong in feature crates, Layer 5) |
| Guard chain integration traits | Macro parsing (belongs in `aura-macros`) |
| Choreography error types (`MpstError`) | |
| Aura-owned upstream boundary in `src/upstream.rs` | Ad hoc direct upstream Telltale imports in downstream Aura crates |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Inbound | `aura-core` | Domain types, effect traits |
| Inbound | Session type definitions | Choreography protocols |
| Inbound | `aura-macros` | Aura-specific annotations |

## Key Modules

- `src/projection.rs`: Global-to-local protocol projection rules.
- `src/protocol.rs`: Protocol specification semantics.
- `src/types.rs`: Core session type definitions.
- `src/guards.rs`: Guard chain integration traits.
- `src/runtime.rs`: Session endpoints and continuation types.
- `src/annotation_lowering.rs`: Lower compiled Telltale annotation records into Aura-owned effects and canonical capability metadata.
- `src/upstream.rs`: Single Aura-owned boundary for upstream Telltale surfaces.

## Upstream Boundary Contract

Allowed to cross `src/upstream.rs`:

- upstream protocol/type surfaces required for choreography parsing and projection
- runtime handler/identifier traits required by Aura-owned protocol abstractions
- theory/coherence surfaces required for compile-time validation and test helpers
- serialized protocol metadata types consumed by Aura-owned manifest generation

Must not cross `src/upstream.rs`:

- Aura ownership policy
- Aura guard-chain admission decisions
- Aura workflow semantics
- Aura runtime service lifecycle and task ownership
- ad hoc direct imports of upstream crate names from unrelated Aura crates

## Invariants

- Depends only on aura-core.
- No handler implementations or composition.
- Extensions handled externally via aura-macros.
- Provides the same `tell!` macro interface over Telltale's public surface.
- `src/upstream.rs` is the sanctioned boundary for naming upstream Telltale crates from Aura-owned code.
- `src/upstream.rs` remains intentionally narrow: language/types/theory plus the
  minimal runtime trait and identifier surface Aura-owned protocol abstractions
  require.
- Choreography capability parsing is fail-closed and admits only canonical
  namespaced `CapabilityName` values.

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

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-mpst` is primarily `Pure`. It defines protocol/specification structure rather than `ActorOwned` runtime ownership. Session/delegation transfer semantics consumed by generated protocols remain explicit and `MoveOwned` in higher layers. `Observed` tooling may inspect generated artifacts but not author protocol truth.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `src/projection.rs`, `src/protocol.rs`, `src/types.rs`, `src/guards.rs` | `Pure` | Protocol/specification semantics and projection rules only. |
| `src/runtime.rs`, endpoint/session descriptors, continuation types | `MoveOwned` | Session endpoints and protocol continuations are value-level handoff surfaces consumed by higher layers. |
| `src/annotation_lowering.rs` | `Pure` | Lower compiled Telltale annotation records into Aura-owned effects. |
| Actor-owned runtime state | none | Live protocol execution ownership belongs in higher layers using these types. |
| Observed-only surfaces | none | Tooling can inspect generated artifacts but does not own protocol truth. |

### Capability-Gated Points

- Typed protocol annotations and guard metadata consumed by higher-layer capability-gated mutation/publication paths

## Testing

### Strategy

aura-mpst defines session types and choreographic extensions. If annotations are lost or reordered, the guard chain executes in the wrong sequence — capability checks may happen after journal commits.

### Commands

```
cargo test -p aura-mpst --test protocols  # annotation and extension contracts
cargo test -p aura-mpst --lib             # inline unit tests
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Guard capability annotation lost | `tests/protocols/annotation_extraction.rs` | covered |
| Legacy or unnamespaced guard capability admitted | `src/composition.rs` inline | covered |
| Leak annotation lost | `tests/protocols/annotation_extraction.rs` | covered |
| Multiple annotations reordered | `tests/protocols/annotation_extraction.rs` | covered |
| Extension types can't be composed | `tests/protocols/extension_types.rs` | covered |
| Composite extension ordering wrong | `tests/protocols/extension_types.rs` | covered |
| Extension field values silently lost | `tests/protocols/extension_types.rs` | covered |
| Core types unavailable via re-exports | `tests/protocols/extension_types.rs` | covered |

## References

- [MPST and Choreography](../../docs/110_mpst_and_choreography.md)
- [Choreography Guide](../../docs/803_choreography_guide.md)
- [Theoretical Model](../../docs/002_theoretical_model.md)
