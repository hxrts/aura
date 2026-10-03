# Aura Effects (Layer 3)

## Purpose

Production-grade stateless effect handlers implementing infrastructure effect traits. Delegates to OS services for crypto, storage, networking, and time.

Encrypted storage initializes a new master key only for an empty ordinary record
namespace. A populated profile requires its original secure key, including when
another first-use wrapper has just published it. Missing or corrupt key material
fails without replacing that key or rewriting existing ciphertext. Required
record inventory and secure retrieval failures preserve their native causes.

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
- Reactive registration uses atomic check-and-insert per signal ID. Repeated `ensure_registered` calls retain the current value; a mismatched value type fails explicitly. An attached subscription establishes its graph receiver before returning a stream to the hook owner.
- The reactive graph assigns a local source revision to every successful publication. `read_snapshot` returns the value and revision under one lock; `update_signal` applies a synchronous delta atomically and leaves value, revision, and subscribers unchanged on rejection; `compare_and_emit` rejects a replacement derived from a stale revision. Product-level projection ownership remains in `aura-app`, which uses these primitives for app and runtime writers.
- The `test-support` feature exposes a platform task spawner for lifecycle tests that must acknowledge a running listener. Production runtime ownership continues through its own task registry.
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
| native profile lifetime and directory resources (`profile_storage.rs`, private `profile_directory.rs`, secure wrapper) | `MoveOwned` infrastructure resources | Actual lock/directory descriptors and provider continuity only; no business lifecycle or actor state. |
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
cargo test -p hxrts-aura-effects
cargo test -p hxrts-aura-effects -- --nocapture   # with handler output
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
| WASM secure storage routes secrets through plaintext browser storage | `src/secure.rs` `wasm_secure_storage_does_not_delegate_to_plain_storage`; web implementation uses WebCrypto AES-GCM with a non-extractable wrapping `CryptoKey` persisted in IndexedDB and encrypted records in a separate IndexedDB store | Covered |
| Guard interpreter misinterprets plan | `src/guard_interpreter.rs` (inline), `tests/handlers/guard_interpreter.rs` | Covered |
| Impure API used outside effect impl | `tests/handlers/impure_api_confinement.rs` | Covered |
| Handler retains state between calls | `src/transport/real.rs` (inline) | Covered |
| Feature guards misconfigured | `tests/feature_guards.rs` | Covered |
| Crypto FROST key gen/sign/verify incorrect | `src/crypto.rs` (inline, 14 tests) | Covered |
| Leakage budget accumulation wrong | `src/leakage.rs` (inline) | Covered |
| Query reads bypass capability checks or implicit public allowlists | `src/query/handler.rs` (inline) | Covered |
| Concurrent projection deltas lose updates or stale replacement overwrites a newer value | `src/reactive/graph.rs`, `src/reactive/handler.rs` (inline) | Covered |

## References

- [Aura System Architecture](../../docs/001_system_architecture.md)
- [Effect System and Runtime](../../docs/103_effect_system.md)
- [Ownership Model](../../docs/122_ownership_model.md)

### Profile ownership adapter and immutable publication

`profile_storage` owns only OS resource lifetime mechanics. The Unix adapter uses a private stable flock inode held by one non-Clone descriptor; the lock file is never removed. This is cooperative local-filesystem exclusivity, not adversarial path protection or a product transaction. Native fallback secure immutable publication encrypts and syncs staging bytes, publishes with an atomic hard link that cannot replace an existing record, and syncs namespace ancestors. Crashes leave absent or complete encrypted records; orphan staging files do not imply admission. Browser, non-Unix and platform keyring unsupported capabilities fail through typed errors until actual backend transactions exist. Real process contention/crash tests and encrypted publication failpoint tests are in `tests/profile_storage_process.rs`, `tests/secure_immutable_publication.rs`, and `secure.rs`.

### Production owner construction and browser backend scope

Production assembly accepts the concrete adapter-produced `OwnedProfileLease`, whose fields remain private. A custom core trait guard does not establish cross-process ownership; compile-fail docs guard this distinction. Every exposed secure backend clone and its encrypted-storage writers retain the actual Arc owner. Backend acquisition precedes wrapping-key and signing-state construction. A backend clone can intentionally extend the lease lifetime after runtime shutdown; another process remains Busy until all writers release.

Browser acquisition uses actual origin-scoped Web Locks and an adapter-owned release acknowledgement. The release signal remains owned by the token; a narrow adapter promise observes broker completion/rejection. This is infrastructure resource lifetime, not a product task or lifecycle publisher. Single-thread wasm is supported; atomics/thread-enabled wasm remains explicitly unsupported until cross-worker release ownership is implemented. IndexedDB immutable publication uses one strict-durability readwrite transaction; wrapping-key first creation also rechecks and adds inside one transaction. Historical plaintext secure localStorage namespaces are rejected before crypto initialization and are not automatically migrated/deleted.

Native platform keyring entries share namespaces across filesystem profiles. Owned Unix production construction additionally retains one service-wide descriptor lease under a fixed OS-user-bound namespace. The original selected profile lease caches that exact service lease across sanctioned runtime reassembly; unrelated profiles cannot simultaneously write the shared service. Raw platform adapters reject IO before this private custody is attached. The existing keyring service and key addresses remain unchanged. Configured platform records are not silently replaced by fresh filesystem state. Native filesystem fallback remains subject to its existing explicit test/harness construction policy; this incremental owner patch does not declare that fallback a platform credential store. Atomic multi-record profile handoff and live two-tab/process restart scenarios remain separate required integration gates.

### Filesystem replacement durability

`StorageCoreEffects::store` publishes complete sibling staging files using an
exclusive private create, file sync, atomic rename and directory sync through the
profile's containing directory. It never deletes the old destination to repair
a failed rename. Errors preserve the actual native I/O source. A failure after
rename reports an uncertain durable outcome: callers reread and validate the
canonical record before deciding whether to retry or resume. Abandoned staging
files are not canonical state. Process-death and publication-boundary fixtures
exercise original-or-complete replacement behavior; they do not simulate power
loss or claim durability on filesystems that reject directory synchronization.
This infrastructure contract does not authorize an enrollment profile handoff
or turn serialized workflow observations into trusted completion evidence.

### Physical profile descriptor ownership

The native Unix profile owner retains both the lifetime lock inode and the actual profile directory descriptor. Attached ordinary storage clones retain that descriptor. The explicit filesystem secure provider receives the concrete owner before accessing its wrapping key and retains the exact secure directory descriptor and original key. A working-directory change or replacement of the selected pathname cannot redirect a live writer.

Descendant file reads, directory creation, staging, replacement, immutable hard-link publication, and deletion use descriptor-relative OS operations with symlink rejection. Publication acknowledgment syncs the held directory chain. Failed or interrupted acknowledgment remains an uncertain outcome requiring canonical reread; no deletion-repair or key replacement is permitted. Directory descriptors are infrastructure resources, not mutable runtime business state.

Actual child-process cwd-change and profile-path replacement tests cover ordinary storage, secure storage, retained clones, and original key continuity. These local publication guarantees do not constitute a multi-record selected-profile transaction or enrollment WAL. Unsupported native targets retain their explicit unsupported semantics; Unix keyring provider custody covers the original shared service through its separately retained namespace lease.

### Original encryption key continuity

Independent encrypted-storage wrappers sharing one concrete owned profile admit their master key through immutable secure publication. A losing first-use publisher reads the original key. Persisted malformed keys cause a typed failure retaining their original bytes; they are never deleted or regenerated. Platform providers without immutable secure publication fail explicitly. The profile lease and wrapping provider must remain shared across bootstrap and runtime construction; key continuity alone does not establish a complete frontend handoff transaction.

### Required-task test supervision

The test-support `CountingTestTaskSpawner` retains the first native required-task failure in asynchronous bounded state. This fixture exercises app hook health without discarding `Result` outcomes. Production supervision remains the runtime task registry; the unit `TestTaskSpawner` does not acknowledge required admission.

The keyring namespace owner uses the actual OS user identity and root-owned sticky temporary directory, never caller HOME/TMPDIR/XDG selection. Private directory and descriptor ownership checks precede provider IO. Native source errors retain original keyring causes. Real child-process contention and process-death tests use private fixture service namespaces and do not read or mutate production keyring records. Keyring write acknowledgment follows the platform credential API; it does not claim filesystem fsync semantics. Lifetime immutable-record overwrite/delete protection is a separate required integration gate.
