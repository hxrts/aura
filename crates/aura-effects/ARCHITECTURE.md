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

### Allowed Adapter Mechanics

The following stateful mechanics are currently allowed because they are
low-level adapter boundaries rather than product-semantic owners:

- `reactive/*`: signal graph subscriptions and task registry used to drive the
  reactive effect surface
- `query/handler.rs`: query-side caches, pending-consensus tracking, and
  subscription plumbing around the reactive/query effect boundary
- `encrypted_storage.rs`: local master-key cache and one-time initialization
  guard for the encrypted-storage adapter

These surfaces are allowed only as handler-local mechanics. They must not grow
product-semantic lifecycle, readiness ownership, or unsupervised business-flow
coordination.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| core handler modules (`crypto.rs`, `storage*.rs`, `transport/*.rs`, `time.rs`, `leakage.rs`) | `Pure` adapter layer | Stateless or low-level effect adapters only; transport timeout wrappers remain infrastructure-local, not product-semantic ownership. |
| `reactive/*` | allowed adapter-local mechanics | Signal graph subscriptions, registries, and task plumbing are permitted only as handler-local effect machinery. |
| `query/handler.rs` | allowed adapter-local mechanics | Query-side caches and pending-consensus tracking are effect-boundary mechanics, not product-semantic coordinators. |
| `encrypted_storage.rs` | allowed adapter-local mechanics | Local key cache and initialization guard are adapter-local only. |
| Actor-owned runtime state | none | Any product-semantic lifecycle, readiness, or long-lived owner task belongs in higher layers. |
| Observed-only surfaces | none | Observation belongs in higher layers; handlers implement effects only. |

### Capability-Gated Points

- upstream capability-gated effect entrypoints consumed through handler
  implementations
- no handler-local semantic lifecycle or readiness publication

### Verification Hooks

- `cargo check -p aura-effects`
- `just lint-arch-syntax`
- `just check-arch`
- `cargo test -p aura-effects -- --nocapture`

### Detailed Specifications

### InvariantStatelessHandlerBoundary
Infrastructure handlers remain stateless, single-party, and isolated from domain semantics.

Enforcement locus:
- src handler implementations map effect traits to operating system integration points.
- No domain crate dependencies are introduced in handler modules.
- `just lint-arch-syntax` owns the syntax-level checks for stateless handler
  boundaries, raw impure/runtime escape hatches, and direct crypto/time/random
  usage; `just check-arch` keeps the integration/governance checks.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- just check-arch and just test-crate aura-effects

Contract alignment:
- [Aura System Architecture](../../docs/001_system_architecture.md) defines handler placement.
- [Effect System and Runtime](../../docs/103_effect_system.md) defines stateless handler rules.
## Testing

### Strategy

Handler isolation and purity are the primary testing concerns. Each handler
must be stateless between calls and confined to infrastructure-only concerns.
Integration tests live in `tests/handlers/`; build-configuration guards live
at `tests/` top level.

### Running tests

```
cargo test -p aura-effects
cargo test -p aura-effects -- --nocapture   # with handler output
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Plaintext leaks to disk | `tests/handlers/encrypted_storage_roundtrip.rs`, `src/encrypted_storage.rs` (inline) | Covered |
| EncryptedStorage key separation fails | `src/encrypted_storage.rs` `test_different_keys_produce_different_ciphertext` | Covered |
| EncryptedStorage disabled mode broken | `src/encrypted_storage.rs` `test_disabled_encryption_passes_through_plaintext` | Covered |
| EncryptedStorage rejects tampered blob | `src/encrypted_storage.rs` `test_plaintext_read_rejected` | Covered |
| Guard interpreter misinterprets plan | `src/guard_interpreter.rs` (inline), `tests/handlers/guard_interpreter.rs` | Covered |
| Impure API used outside effect impl | `tests/handlers/impure_api_confinement.rs` | Covered |
| Handler retains state between calls | `src/transport/real.rs` (inline) | Covered |
| Feature guards misconfigured | `tests/feature_guards.rs` | Covered |
| Crypto FROST key gen/sign/verify incorrect | `src/crypto.rs` (inline, 14 tests) | Covered |
| Leakage budget accumulation wrong | `src/leakage.rs` (inline) | Covered |

## Boundaries
- Stateful caches belong in Layer 6 services.
- Multi-party coordination belongs in aura-protocol.
- Application-specific handlers belong in domain crates.
