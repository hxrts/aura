# Aura Composition (Layer 3)

## Purpose

Assemble individual effect handlers into cohesive effect systems. Provides registry, builder, and lifecycle infrastructure for composing stateless handlers.

## Scope

| Belongs here | Does not belong here |
|--------------|----------------------|
| `EffectRegistry`: type-indexed storage of handler instances | Handler implementations (aura-effects) |
| `CompositeHandler`, `CompositeHandlerBuilder`: unified handler composition | Multi-party coordination (aura-protocol) |
| `ViewDeltaReducer`, `ViewDeltaRegistry`, `ViewDelta`: view reduction infrastructure | Domain crates or higher layers |
| Adapter patterns for handler registration and delegation | Runtime lifecycle management (aura-agent) |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Down | `aura-core` | Effect trait definitions |
| Down | `aura-effects` | Individual effect handler implementations |

## Invariants

- Does NOT implement handlers (implementations live in aura-effects).
- Does NOT do multi-party coordination (that belongs in aura-protocol).
- Effect registry is type-indexed for compile-time safety.
- Handler composition is stateless.

### InvariantCompositionTypeSafeRegistry

Handler composition must remain type-safe and free of protocol semantics.

Enforcement locus:
- src registry and composition helpers assemble handlers by trait contract.
- No coordination logic is introduced in composition modules.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- `just check-arch` and `just test-crate aura-composition`

Contract alignment:
- [Aura System Architecture](../../docs/001_system_architecture.md) defines layer placement.
- [Effect System and Runtime](../../docs/103_effect_system.md) defines handler assembly rules.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-composition` is primarily `Pure` assembly and wiring. It coordinates construction and configuration but is not the `ActorOwned` owner of parity-critical runtime state. Capability gating belongs in the assembled contracts and owner modules. See [Ownership Model §9](../../docs/122_ownership_model.md) for reactive contract details.

### Allowed Assembly Mechanics

The `Arc<dyn ...>` adapter surfaces in `src/adapters/*` are allowed because they are shared references to already-owned handlers, not ownership of mutable runtime state. Composition may hold and clone handler references for type-safe assembly, but it must not introduce background tasks, internal mutable registries with semantic meaning, or lifecycle ownership of the assembled system.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `src/registry.rs`, `src/builder.rs`, `src/composite.rs` | `Pure` assembly | Type-safe handler registry and composition wiring only. |
| `src/adapters/*` | allowed assembly mechanics | Shared `Arc<dyn ...>` references to already-owned handlers; not semantic owner state. |
| `src/view_delta.rs` and related reduction infrastructure | `Pure` | View reduction and typed assembly-time adaptation only. |
| Actor-owned runtime state | none | Lifecycle ownership of the assembled system belongs in higher layers. |
| Observed-only surfaces | none | Composition may be inspected, but not treated as a semantic owner. |

### Capability-Gated Points

- None local; capability gating belongs in the assembled contracts and owner modules that consume composition output.

## Testing

### Strategy

All tests are inline — appropriate for a composition utility crate whose tests exercise type-safe registry wiring and builder patterns. No integration test surface is needed.

### Commands

```
cargo test -p aura-composition
just check-arch
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Type-safe registry lookup fails at runtime | `src/registry.rs` (inline) | Covered |
| Duplicate registration silently replaces handler | `src/registry.rs` `test_duplicate_registration_replaces_handler` | Covered |
| Supported operations don't match registry | `src/adapters/mod.rs` (inline) | Covered |
| View delta compaction loses deltas | `src/view_delta.rs` (inline) | Covered |
| Reducer dispatch to wrong handler | `src/view_delta.rs` (inline) | Covered |
| HandlerContext fresh operation/session ids and deterministic test constructor split | `src/registry.rs` (inline) | Covered |

## References

- [Aura System Architecture](../../docs/001_system_architecture.md)
- [Effect System and Runtime](../../docs/103_effect_system.md)
- [Ownership Model](../../docs/122_ownership_model.md)
