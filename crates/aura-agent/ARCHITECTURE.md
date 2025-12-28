# Aura Agent (Layer 6) - Architecture and Invariants

## Purpose
Production runtime composition and effect system assembly for authority-based
identity management. Owns effect registry, builder infrastructure, context
management, and choreography adapters.

## Inputs
- All lower layers (Layers 1-5): core types, effect traits, domain crates, protocols.
- Authority identifiers (`AuthorityId`) and context (`ContextId`, `SessionId`).
- Effect implementations from aura-effects.
- Protocol coordination from aura-protocol.

## Outputs
- `AgentBuilder`, `AuraAgent`, `EffectContext`, `EffectRegistry`.
- `AuraEffectSystem`, `EffectSystemBuilder`, `EffectExecutor`.
- Services: `SessionService`, `AuthService`, `RecoveryService`, `SyncManagerState`.
- `RuntimeSystem`, `LifecycleManager`, `ReceiptManager`, `FlowBudgetManager`.
- `ReactiveScheduler` for signal-based notification.

## Invariants
- Must NOT create new effect implementations (delegate to aura-effects).
- Must NOT implement multi-party coordination (delegate to aura-protocol).
- Must NOT be imported by Layers 1-5 (prevents circular dependencies).
- Authority-first design: all operations scoped to specific authorities.
- Lazy composition: effects assembled on-demand.
- Mode-aware execution: production, testing, and simulation use same API.

## Boundaries
- Stateless handlers live in aura-effects.
- Protocol logic lives in aura-protocol.
- Application core lives in aura-app.
