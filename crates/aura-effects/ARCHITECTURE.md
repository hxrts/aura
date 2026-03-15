# Aura Effects (Layer 3) - Architecture and Invariants

## Purpose
Production-grade stateless effect handlers implementing infrastructure effect traits.
Delegates to OS services for crypto, storage, networking, and time.

## Inputs
- aura-core (effect trait definitions).
- External libraries (crypto, networking, filesystem).

## Outputs
- Infrastructure handlers: `RealCryptoHandler`, `RealTransportHandler`, `FilesystemStorageHandler`.
- Time providers: `PhysicalTimeHandler`, `LogicalClockHandler`, `OrderClockHandler`.
- Encrypted storage: `EncryptedStorage` wrapper with transparent encryption.
- Query handler: `QueryHandler` for Datalog-style queries.
- Leakage handler: `ProductionLeakageHandler`.

## Invariants
- Handlers must be stateless (no shared mutable state).
- Handlers must be single-party (each handler independent).
- Handlers must be context-free (no assumptions about caller context).
- No dependencies on domain crates or aura-protocol.

## Ownership Model

- `aura-effects` is primarily a stateless adapter layer, not an `ActorOwned`
  semantic owner.
- It should not grow long-lived mutable async ownership beyond narrow low-level
  adapter mechanics.
- `MoveOwned` authority transfer is not defined here; higher layers own those
  contracts.
- Capability-gated semantic mutation and publication remain upstream.
- `Observed` and runtime layers consume handler behavior; handlers must not
  silently redefine semantic ownership.

### Detailed Specifications

### InvariantStatelessHandlerBoundary
Infrastructure handlers remain stateless, single-party, and isolated from domain semantics.

Enforcement locus:
- src handler implementations map effect traits to operating system integration points.
- No domain crate dependencies are introduced in handler modules.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just check-arch and just test-crate aura-effects

Contract alignment:
- [Aura System Architecture](../../docs/001_system_architecture.md) defines handler placement.
- [Effect System and Runtime](../../docs/103_effect_system.md) defines stateless handler rules.
## Boundaries
- Stateful caches belong in Layer 6 services.
- Multi-party coordination belongs in aura-protocol.
- Application-specific handlers belong in domain crates.
