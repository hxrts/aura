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
- `AuraEffectSystem` with subsystems: `CryptoSubsystem`, `TransportSubsystem`, `JournalSubsystem`.
- Services: `SessionServiceApi`, `AuthServiceApi`, `RecoveryServiceApi`, `SyncManagerState`.
- `RuntimeSystem`, `LifecycleManager`, `ReceiptManager`, `FlowBudgetManager`.

## Key Modules
- `core/`: Public API (AgentBuilder, AuraAgent, AuthorityContext).
- `builder/`: Platform-specific preset builders (CLI, iOS, Android, Web).
- `runtime/`: Internal runtime, subsystems, services, choreography adapters.
- `handlers/`: Service API implementations (auth, session, recovery, etc.).
- `reactive/`: Signal-based notification and scheduling.

## Invariants
- Must NOT create new effect implementations (delegate to aura-effects).
- Must NOT implement multi-party coordination (delegate to aura-protocol).
- Must NOT be imported by Layers 1-5 (prevents circular dependencies).
- Authority-first design: all operations scoped to specific authorities.
- Lazy composition: effects assembled on-demand.
- Mode-aware execution: production, testing, and simulation use same API.

## Concurrency
- `parking_lot` locks for brief sync operations (RNG, stats, inbox).
- `tokio::sync` locks for operations spanning `.await` points.
- `std::sync` locks where poison detection is required.
- See subsystem modules for lock ordering rules.

### Detailed Specifications

### InvariantRuntimeCompositionBoundary
Runtime composition must assemble existing effect handlers without introducing new effect implementations or protocol logic.

Enforcement locus:
- src/runtime composes handlers and services through registry and builder types.
- src/builder constrains runtime modes and dependency wiring.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just check-arch and just test-crate aura-agent

Contract alignment:
- [Aura System Architecture](../../docs/001_system_architecture.md) defines layer boundaries.
- [Effect System and Runtime](../../docs/105_effect_system_and_runtime.md) defines composition constraints.
## Boundaries
- Stateless handlers live in aura-effects.
- Protocol logic lives in aura-protocol.
- Application core lives in aura-app.

