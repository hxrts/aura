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

- Public FROST package construction accepts typed public commitments only.
  Bound-message signing checks both native and DTO transcripts against the
  independently admitted message, public package, threshold and participant
  inventory before the audited library signs with one local key package.
  `PublicFrostSigningError` preserves native and codec causes. Durable nonce
  ownership and quorum admission belong to the runtime; these primitives do
  not establish either. The `crypto::public_frost::tests` regressions verify a
  genuine 2-of-3 signature and reject substituted intents and inventories.
  Aggregation validates the complete selected inventory and exact share count;
  it never ignores surplus shares or erases native aggregation causes.

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

### Secure record lifetime protection and physical scope

Immutable secure publication seals the exact original record, including a legacy original, and rejects generic replacement, deletion, and key generation at that location. Provider-authenticated protection metadata is published in the same complete encrypted record. Atomic initial mutable publication is separate: required clock/profile checkpoints remain mutable under their own domain owner. Cooperating fallback writer views share the selected physical profile's record decision gate; platform keyring writers share the actual original service namespace owner; browser writes authenticate the original and compare exact ciphertext inside one strict IndexedDB transaction. Interrupted publication may have succeeded; an immutable retry acknowledges unchanged original bytes before reporting success.

The native fallback and browser adapters read historical v1 encrypted records and seal original plaintext without using caller replacement bytes. Platform keyring records retain original service/key addresses. Provider-private wrapping-key/inventory addresses are unreachable through normal location mapping; a missing original key with prior inventory fails closed. A historical raw keyring value colliding with the reserved encrypted format is rejected rather than reinterpreted or repaired. Generic retirement cannot delete protected records; no unrestricted retirement escape is supplied.

Ordinary filesystem IO, enumeration, statistics, and clear operations exclude the existing infrastructure secure-provider subtree before traversal. This prevents filename collisions and ordinary clear from bypassing secure-record protection. Existing physical data is not relocated. Real native child-process recreation and historical codec fixtures exercise the selected fallback provider. Browser multi-context/process-death and actual OS credential-provider loss/legacy-collision fixtures remain required validation for the platform paths; syntax validation alone establishes none of those runtime guarantees.

Individual participant proof signing is an explicit crypto effect. Production
entropy failures retain the native source; deterministic simulation uses its
owned seeded source. The handler validates the actual share/verifier match and
uses the FROST ciphersuite Schnorr API without custom nonce construction or
Ed25519 seed reinterpretation. This single-party proof is distinct from group
threshold signing, which continues to require the actual FROST quorum path.

Required secure retrieval preserves logical record absence as the concrete
`SecureStorageRecordMissing` cause within the storage category. Observational
`secure_exists` still reports absence as `false`; native provider failures
retain their original sources. Logical absence never manufactures an OS error
and never grants authority to select replacement signing material.

Selected backing-record corruption fixtures are available only under explicit
native test support. They mutate actual encrypted bytes under the retained
provider record owner and preserve acknowledgment failures. Production mutation
and immutable-record protections remain required; fixture corruption supplies
no plaintext access or retirement capability.

Unified `verify_signature` decodes the exact mode-specific public package.
Threshold mode extracts its native group point before the low-level
`frost_verify` primitive; a serialized package is not a raw verifying point.
The genuine public-only 2-of-3 primitive regression also exercises this unified
verification path and rejects message substitution.

The selected descriptor provider retains an authenticated original root birth,
readiness seal and bounded birth inventory. Interrupted pre-live allocation may
complete its retained original pending record; missing once-live checkpoint or
leaf fails closed. Negative first-decision tombstones require atomic publication
and directory ACK. Clearing current plaintext does not prove physical or backup
erasure while the persistent wrapping root remains. Permanent legacy immutable
records cannot acquire allocation retirement semantics.


### Original selected-provider migration

An exclusive selected filesystem provider authenticates canonical legacy records
within 4096 entries, 8 MiB per record and 32 MiB cumulative encrypted bytes.
Exceeding these bounds fails without truncation or profile mutation. Authentication
uses its retained original wrapping key before anchoring an
allocation lifetime root. Original permanent envelopes and their protection
bits are unchanged. Separate protected immutable birth and completed-handoff
anchors bind the root. Mandatory authenticated lifecycle state precedes birth,
advances to Handed after every handoff ACK and never reconstructs from absence.
Live use checks their exact acknowledged encrypted bytes
as well as original ledger birth, marker, readiness and handoff seals. Missing
live evidence fails closed; it does not trigger migration or reinitialization.
This local custody proof grants no peer or identity trust.

Native keyring and browser providers require their own transactional lifetime
backend; absence is a typed unavailable result. Unknown staging artifacts and
corrupt legacy records fail closed. Provider ACK/tombstones do not prove physical
or backup erasure or detection of a complete filesystem backup rollback.

The private allocation lifetime record has an explicitly justified serde codec
for AEAD persistence only (docs/100_crypto.md). Plaintext buffers use Zeroizing;
the decoded record erases its secret field on drop. No Debug/Clone or public
export is provided. Required secret-wrapper and exception-metadata gates cover
this codec alongside physical provider ciphertext/retirement regressions.

Encrypted storage retains the actual selected crypto KDF/encryption/decryption
source in `StorageError::BackendFailure`; operation context does not replace
that cause with formatted text. An invalid configured KDF key length is rejected
before copying into the fixed key buffer. Ordinary provider replacement does
not bypass unified at-rest encryption or selected secure key custody.

Protected lifetime JSON is counted without a plaintext buffer and then encoded
into a single exactly sized zeroizing allocation through a non-growing slice
writer. Both passes enforce the authenticated record's ciphertext budget,
including its magic, nonce and AEAD tag. Partial codec failures zeroize the
allocated plaintext and preserve their native serialization cause.

Private profile staging binds its versioned name to the exact target and
acknowledges the staged directory entry before publication. Initialization
recovery bounds and authenticates original ciphertext, validates its protected
origin and root/index bindings, then finishes only a missing initial target or
identical publication. A differing acknowledged target, ambiguous stage, old
anonymous stage or missing once-live evidence fails closed. Recovery never
discards such evidence to create an apparently empty profile.

Initial lifetime recovery checks a descriptor-relative provider-wide stage
inventory with streaming depth-first retained descriptors. Memory is bounded by
the eight candidate initialization targets and the native secure-location
layout (namespace/key/optional-subkey), with two descendant directory levels.
Ordinary leaves have no new total inventory cap; a maximum 4096 allocation ledger
plus provider metadata and arbitrary ordinary records remain scannable. It permits only the
exact initialization target families for later authenticated validation and
requires no residual stage before root handoff. Recovery-specific publication
proves equality on an existing target before removing its original stage; it
never uses the general create-conflict cleanup path. Mutable successor recovery
remains separate and differing retained checkpoints still fail closed.

The native full inventory scan has linear IO latency. An original builder
execution-window integration remains a separate obligation; this synchronous
provider API does not claim an injected deadline or timeout proof.

Private initialization reads distinguish canonical target presence from an
observed stage and tolerate two links only for an exact original target/stage
inode pair. The original physical selected provider and protected root binding
stay held until target ACK and stage removal ACK. This does not relax generic
secure-record alias checks or recover absent once-live records from a stage.

### Initial mutable publication ownership

`VerifiedOriginalInitializationSuccessor` is a private nonserializable witness
minted by two declared proof issuers from the actual selected provider, retained
publication and complete original phase proof. Private cutover consumers preserve
bounded authenticated journals and exact predecessor/successor inode custody
before atomic exchange. Displaced evidence and journals are archived without
replacement or deletion. Recovery and archived-history validation retain original
birth and reject allocated/pending state. Only Apple/Linux atomic exchange is
supported; unavailable native operations return original errors with no rename
fallback. No dependency change is needed: rustix1.1.4 already supplies exchange and
no-replace operations.

Actual native tests kill creator subprocesses at journal stage/ACK, each custody
publication, exchange, ACK and archive, and inject both source and current-target
substitution between verification and cutover. Additional tests cover consecutive
initial index transitions, authenticated historical corruption, unknown metadata,
foreign owner and an exposed positive first decision. Required source/discovery/
execution inventory includes these cases. Live mutable checkpoint recovery and
original builder IO-window/drain remain open; synchronous native IO is not
preemptible merely because callers perform checkpoints.

Archived initialization transactions require all three retained inode/ciphertext custody records on every Ready reopen. The native alias reader is observational: authenticated journal validation supplies the authority and exact expected identities. Unreferenced custody metadata cannot satisfy that validation.

Required archived-custody validation also rejects ciphertext corruption at each of the three retained paths, preserving the original birth and current root checkpoint bytes without replacement.

Required physical reads propagate native clock failures instead of returning
zero. Absolute deadline waits use the actual provider clock and fixed endpoint,
recheck after timer wakes, and report rollback. Browser timers own their callback
and cancel registration on Drop; thread-confined JS custody is checked by
`SendWrapper`. This bookkeeping is local to the timer future, not a runtime
service or detached task. Native required endpoint tests join the lifecycle gate.

Original lifetime inventory admission and bounded listing use the shared
`MAX_PROFILE_ALLOCATION_COUNT` count bound, preserving the existing 4096 limit.
The bound cannot substitute for provider-authenticated inventory custody.
