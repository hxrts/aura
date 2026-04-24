# Aura Effects (Layer 3)

## Purpose

Production-grade stateless effect handlers implementing infrastructure effect traits. Delegates to OS services for crypto, storage, networking, and time.

## Scope

| Belongs here | Does not belong here |
|--------------|----------------------|
| Infrastructure handlers: `RealCryptoHandler`, `RealTransportHandler`, `FilesystemStorageHandler` | Stateful caches (Layer 6 services) |
| Time providers: `PhysicalTimeHandler`, `LogicalClockHandler`, `OrderClockHandler` | Multi-party coordination (aura-protocol) |
| Encrypted storage: `EncryptedStorage` wrapper with transparent encryption | Application-specific handlers (domain crates) |
| Query handler: `QueryHandler` for Datalog-style queries | Domain semantics or business logic |
| Leakage handler: `ProductionLeakageHandler` | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| Down | `aura-core` | Effect trait definitions |
| External | crypto, networking, filesystem libraries | OS integration |

## Invariants

- Handlers must be stateless (no shared mutable state).
- Handlers must be single-party (each handler independent).
- Handlers must be context-free (no assumptions about caller context).
- No dependencies on domain crates or aura-protocol.
- `EncryptedStorage` production construction is encrypted-only; plaintext
  passthrough remains available only through the explicit
  `EncryptedStorageConfig::testing_plaintext()` test/simulation constructor.

### InvariantStatelessHandlerBoundary

Infrastructure handlers remain stateless, single-party, and isolated from domain semantics.

Enforcement locus:
- src handler implementations map effect traits to operating system integration points.
- No domain crate dependencies are introduced in handler modules.
- `just lint-arch-syntax` owns the syntax-level checks for stateless handler boundaries, raw impure/runtime escape hatches, and direct crypto/time/random usage; `just check-arch` keeps the integration/governance checks.

Failure mode:
- Behavior diverges from the crate contract and produces non-reproducible outcomes.
- Cross-layer assumptions drift and break composition safety.

Verification hooks:
- `just check-arch` and `just test-crate aura-effects`

Contract alignment:
- [Aura System Architecture](../../docs/001_system_architecture.md) defines handler placement.
- [Effect System and Runtime](../../docs/103_effect_system.md) defines stateless handler rules.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-effects` is primarily a stateless adapter layer, not an `ActorOwned` semantic owner. Handlers implement effects only; semantic lifecycle, readiness, and `MoveOwned` authority transfer are defined in higher layers. See [Ownership Model §9](../../docs/122_ownership_model.md) for reactive contract details.

### Allowed Adapter Mechanics

The following stateful mechanics are currently allowed because they are low-level adapter boundaries rather than product-semantic owners:

- `reactive/*`: signal graph subscriptions and task registry used to drive the reactive effect surface
- `query/handler.rs`: query-side caches, pending-consensus tracking, and subscription plumbing around the reactive/query effect boundary
- `encrypted_storage.rs`: local master-key cache and one-time initialization guard for the encrypted-storage adapter

These surfaces are allowed only as handler-local mechanics. They must not grow product-semantic lifecycle, readiness ownership, or unsupervised business-flow coordination.

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

- Upstream capability-gated effect entrypoints consumed through handler implementations.
- No handler-local semantic lifecycle or readiness publication.

### Transport Failure Handling

- Transport connect retries stay bounded and handler-local. `aura-effects::transport`
  may retry transient DNS / TCP / handshake failures with exponential backoff, but it
  must not grow session ownership, peer registries, or multi-party coordination.
- Retryable failures are limited to transient network conditions such as DNS timeout,
  temporary address-resolution failure, connection refusal/reset, and handshake I/O
  timeout. Protocol/URL errors and non-I/O handshake failures remain terminal.
- Hostname resolution for WebSocket endpoints must stay inside an explicit async timeout
  boundary; synchronous DNS lookups outside the timeout budget are not allowed.

## Testing

### Strategy

Handler isolation and purity are the primary testing concerns. Each handler must be stateless between calls and confined to infrastructure-only concerns. Integration tests live in `tests/handlers/`; build-configuration guards live at `tests/` top level.

### Commands

```
cargo test -p aura-effects
cargo test -p aura-effects -- --nocapture   # with handler output
just lint-arch-syntax
just check-arch
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Plaintext leaks to disk | `tests/handlers/encrypted_storage_roundtrip.rs`, `src/encrypted_storage.rs` (inline) | Covered |
| EncryptedStorage key separation fails | `src/encrypted_storage.rs` `test_different_keys_produce_different_ciphertext` | Covered |
| EncryptedStorage explicit test-only plaintext path broken | `src/encrypted_storage.rs` `test_disabled_encryption_passes_through_plaintext` | Covered |
| EncryptedStorage rejects tampered blob | `src/encrypted_storage.rs` `test_plaintext_read_rejected` | Covered |
| Guard interpreter misinterprets plan | `src/guard_interpreter.rs` (inline), `tests/handlers/guard_interpreter.rs` | Covered |
| Impure API used outside effect impl | `tests/handlers/impure_api_confinement.rs` | Covered |
| Handler retains state between calls | `src/transport/real.rs` (inline) | Covered |
| Feature guards misconfigured | `tests/feature_guards.rs` | Covered |
| Crypto FROST key gen/sign/verify incorrect | `src/crypto.rs` (inline, 14 tests) | Covered |
| Leakage budget accumulation wrong | `src/leakage.rs` (inline) | Covered |
| Query reads bypass capability checks or implicit public allowlists | `src/query/handler.rs` (inline) | Covered |

## References

- [Aura System Architecture](../../docs/001_system_architecture.md)
- [Effect System and Runtime](../../docs/103_effect_system.md)
- [Ownership Model](../../docs/122_ownership_model.md)
