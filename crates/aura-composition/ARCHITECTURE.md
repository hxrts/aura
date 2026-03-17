# Aura Composition (Layer 3) - Architecture and Invariants

## Purpose
Assemble individual effect handlers into cohesive effect systems. Provides registry,
builder, and lifecycle infrastructure for composing stateless handlers.

## Inputs
- Individual effect handler implementations from aura-effects.
- Handler definitions implementing `RegistrableHandler` trait.
- Configuration and lifecycle signals.

## Outputs
- `EffectRegistry`: Type-indexed storage of handler instances.
- `CompositeHandler`, `CompositeHandlerBuilder`: Unified handler composition.
- `ViewDeltaReducer`, `ViewDeltaRegistry`, `ViewDelta`: View reduction infrastructure.
- Adapter patterns for handler registration and delegation.

## Invariants
- Does NOT implement handlers (implementations live in aura-effects).
- Does NOT do multi-party coordination (that belongs in aura-protocol).
- Effect registry is type-indexed for compile-time safety.
- Handler composition is stateless.

## Ownership Model

- `aura-composition` is primarily `Pure` assembly and wiring.
- It may coordinate construction and configuration, but it is not the
  `ActorOwned` owner of parity-critical runtime state.
- `MoveOwned` transfer semantics should be surfaced in higher-layer contracts,
  not hidden in composition utilities.
- Capability gating belongs in the assembled contract and owner modules rather
  than in composition-local shortcuts.
- `Observed` tooling may inspect assembled systems, but composition should not
  author semantic lifecycle.

### Allowed Assembly Mechanics

The `Arc<dyn ...>` adapter surfaces in `src/adapters/*` are allowed because
they are shared references to already-owned handlers, not ownership of mutable
runtime state. Composition may hold and clone handler references for type-safe
assembly, but it must not introduce background tasks, internal mutable
registries with semantic meaning, or lifecycle ownership of the assembled
system.

### Detailed Specifications

### InvariantCompositionTypeSafeRegistry
Handler composition must remain type-safe and free of protocol semantics.

Enforcement locus:
- src registry and composition helpers assemble handlers by trait contract.
- No coordination logic is introduced in composition modules.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just check-arch and just test-crate aura-composition

Contract alignment:
- [Aura System Architecture](../../docs/001_system_architecture.md) defines layer placement.
- [Effect System and Runtime](../../docs/103_effect_system.md) defines handler assembly rules.
## Boundaries
- Depends only on aura-core and aura-effects.
- No domain crates or higher layers.
- Runtime lifecycle management belongs in aura-agent.
