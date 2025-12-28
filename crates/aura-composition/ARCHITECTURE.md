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

## Boundaries
- Depends only on aura-core and aura-effects.
- No domain crates or higher layers.
- Runtime lifecycle management belongs in aura-agent.
