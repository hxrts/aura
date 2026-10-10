# Aura Core (Layer 1)

AMP lifecycle failures retain concrete effect causes through the native runtime boundary. Canonical checkpoint absence has a private producer in the AMP journal reader. Scoped duplicate diagnostics require an exact requested entity and an independent successful canonical read before reconciliation; diagnostic wording and error records alone cannot suppress mutation failures. `AmpChannelError` carries source-bearing `AuraError` values and no longer promises equality; compare typed variants or stable categories. Foreign diagnostics explicitly discard native causes only at the presentation adapter.

## Purpose

`invitation` owns the canonical invitation wire/cache schema and setup binding.
These serializable values are observations, not issuance, admission, original
policy approval or journal-commit evidence. Feature capability policy stays in
`aura-invitation`; lower-layer admission must retain its independent owner.
Device enrollment requires the original setup binding and invited authority in
the canonical schema; omitted or null fields fail decoding.

The AMP error interface includes `RejoinRequiresMembershipEvidence` with exact context,
channel and participant diagnostics. This typed refusal identifies an
unversioned membership mutation failure; it supplies no successor capability.

Single source of truth for domain types and effect trait definitions. Provides foundational algebraic types with zero dependencies on other Aura crates.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Effect trait definitions (infrastructure + application) | Handler implementations (`aura-effects`) |
| Domain types, algebraic types, crypto utilities | Protocol logic (`aura-protocol`) |
| Ownership vocabulary (`actor_owned`, `move_owned`, `capability_gated`) | Application-specific types (domain crates) |
| Tree types, time system, query types, message types | Runtime state or business logic |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| consumes | External libraries only | No internal Aura dependencies |
| produces | Effect traits (infrastructure) | Crypto, Network, Storage, Time, Random |
| produces | Effect traits (application) | Journal, Authorization, FlowBudget, Leakage |
| produces | Domain types | `AuthorityId`, `ContextId`, `SessionId`, `FlowBudget` |
| produces | Algebraic types | `Cap` (meet-semilattice), `Fact` (join-semilattice), `Journal` |
| produces | Crypto utilities | Key derivation, FROST types, merkle trees |
| produces | Tree types | `TreeOp`, `AttestedOp`, `Policy`, `LeafNode`, commitment functions |
| produces | Time system | Physical/Logical/Order/Range clocks with `TimeStamp` variants |
| produces | Query types | `Query` trait, Datalog types |
| produces | Message types | `WireEnvelope`, versioning, validation |
| produces | Ownership vocabulary | `OperationContext`, `TerminalPublisher`, owner tokens, handoff records |

## Invariants

- Portable conformance counts and transport-error capacities use fixed-width
  integers. Runtime collection sizes are converted at their producer boundaries;
  artifact conversion failures retain their original source.

- `time::causal` (`CausalTag`, `CausalClock`, `CausalMetadata`) is wire data for
  order-independent fact families; the reduction rules live in `aura-journal`.
- `FrostPublicCommitment` is public protocol data, not signing authority.
  The public-package effect accepts these entries rather than serialized
  secret nonce bundles. Bound-message signing requires independently supplied
  expected message, public package and threshold; runtime owners establish
  their provenance and enforce one-use nonce custody.

- Zero internal dependencies (foundation constraint).
- `AuraError` preserves each concrete process-local cause as its immediate
  standard `Error::source`, including after cloning. Source traversal exposes
  the original error rather than its shared storage wrapper. Serialized errors
  retain their category and message but omit process-local source objects.
- `crypto::tree_signing::validate_retained_threshold_key_package` checks a native
  FROST package against the exact signer index, authenticated threshold policy,
  complete public participant inventory, group key and derived verifying share.
  It validates one local share without a quorum. Native public packages do not
  encode the threshold; conversion defaults cannot establish policy evidence.
- Effect trait definitions only (no implementations).
- `ReactiveEffects::ensure_registered` requires atomic per-signal check-and-insert without resetting live values; registration under an existing ID with a different type is an error. Subscription to an unregistered signal fails explicitly.
- Semilattice laws: monotonic growth (facts), monotonic restriction (capabilities).
- Context isolation prevents cross-context information flow.
- Secret-bearing wrappers such as `PrivateKeyBytes` are the canonical Layer 1
  carrier for raw private-key material; explicit export context is required
  before those bytes may leave the wrapper.
- `StoragePath` is the canonical segment-aware storage scope primitive;
  wildcard coverage is limited to a single terminal `*` segment and matching
  must use `StoragePath::covers` rather than raw string prefix checks.
- Peer-originated, signed, content-addressed, and journal-fact DAG-CBOR bytes
  must decode through strict `util::serialization::from_slice`; the
  non-canonical-tolerant `from_slice_trusted` path is reserved for trusted
  internal bytes only.

### InvariantContextIsolation

Information must not flow across relational context boundaries without explicit authorization.

Enforcement locus:
- `aura-core/src/types/identifiers.rs`: `ContextId` defines opaque context scope.
- `aura-journal/src/fact.rs`: `JournalNamespace::Context(ContextId)` isolates fact storage.
- `aura-journal/src/reduction.rs`: `reduce_context()` reduces one context at a time.
- `aura-rendezvous/src/new_channel.rs`: secure channels bind to a single `ContextId`.

Failure mode:
- Cross-context visibility of facts or metadata.
- Capability scope confusion across unrelated relationships.
- Replay of facts or messages into the wrong context namespace.

Verification hooks:
- `cargo test -p aura-core context_isolation`
- `cargo test -p aura-journal namespace_separation`
- `cargo test -p aura-rendezvous channel`

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines context-scoped semantics.
- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md) defines context privacy boundaries.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines `InvariantContextIsolation`.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-core` is primarily `Pure`. It defines the canonical ownership vocabulary (`actor_owned::*`, `move_owned::*`, `capability_gated::*`) consumed by higher layers. It must not own `ActorOwned` runtime state. Downstream `Observed` layers consume these contracts but must not mutate or republish semantic truth.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `src/ownership.rs` | `MoveOwned` + capability-gated vocabulary | `OperationContext`, `TerminalPublisher`, publication wrappers, owner tokens, handoff records, `actor_owned`/`move_owned`/`capability_gated` module layout. |
| `src/time/timeout.rs` | `MoveOwned` | Typed timeout budgets, attempt budgets, retry/backoff policy. `OperationTimeoutBudget` is the workflow-facing wrapper. |
| `src/service.rs` | `Pure` | Canonical family/object vocabulary including `Establish`, `Move`, shared `Hold` custody types, selector capabilities, and typed reply-block contracts. |
| `src/effects/` | `Pure` | Effect traits and trait-level helper surfaces only. |
| `src/domain/`, `src/types/`, `src/query.rs`, `src/messages/`, `src/tree/`, `src/crypto/` | `Pure` | Value-level domain/state/query/message/crypto contracts. |

### Capability-Gated Points

- operation-context issuance in `src/ownership.rs`
- progress / terminal / readiness publication wrappers in `src/ownership.rs`
- actor-ingress mutation wrappers in `src/ownership.rs`
- ownership token issuance requests in `src/ownership.rs`

## Testing

### Strategy

aura-core is the foundation for a threshold-cryptographic P2P identity system. If its invariants break, every crate above it is silently unsound. Testing priorities follow the blast radius of a failure:

1. **Cryptographic commitment correctness** — highest-consequence bugs
2. **Algebraic laws** — semilattice violations cause CRDT divergence
3. **Ownership boundaries** — compile-fail tests enforce private constructors, sealed traits
4. **Serialization determinism** — pinned test vectors lock byte-level encoding
5. **Identifier and key derivation stability** — pinned vectors prevent drift
6. **Time system ordering** — all four clock domains need law coverage

### Commands

```
cargo test -p aura-core                    # all tests
cargo test -p aura-core --test laws        # algebraic laws only
cargo test -p aura-core --test contracts   # API contracts only
cargo test -p aura-core --test compile_fail # ownership boundaries only
cargo test -p aura-core --lib              # inline unit tests only
```

### Coverage matrix

| What breaks if wrong | Test location | Method | Status |
|---------------------|--------------|--------|--------|
| Branch/leaf commitment determinism | `src/tree/commitment.rs` | inline pinned | covered |
| Binding message includes group pubkey | `src/tree/verification.rs` | inline | covered |
| FROST sign → aggregate → verify | `src/crypto/tree_signing.rs` | inline | covered |
| Commitment changes when any input changes | `src/tree/commitment.rs` | inline differential | covered |
| Signature replay across groups blocked | `src/tree/verification.rs` | inline | covered |
| JoinSemilattice — u64, Vec, BTreeMap | `tests/laws/semilattice_join.rs` | example | covered |
| MeetSemilattice — u64, BTreeSet | `tests/laws/semilattice_meet.rs` | proptest | covered |
| FlowBudget CRDT — join, merge, convergence | `tests/laws/flow_budget_crdt.rs` | proptest | covered |
| Policy meet-semilattice | `tests/laws/tree_policy_meet.rs` | proptest | covered |
| Time ordering across clock domains | `tests/laws/time_ordering.rs` | proptest | covered |
| JoinSemilattice — Fact, FactValue | `tests/laws/semilattice_join.rs` | example | covered |
| MeetSemilattice — Cap | `tests/laws/semilattice_meet.rs` | example + Biscuit | covered |
| FlowBudget epoch rotation monotonicity | `tests/laws/flow_budget_crdt.rs` | example | covered |
| TerminalPublisher: not clonable, no double publish | `tests/boundaries/` | compile-fail | covered |
| OperationContext: private constructor | `tests/boundaries/` | compile-fail | covered |
| OwnerToken: stale after handoff | `tests/boundaries/` | compile-fail | covered |
| Sealed owner traits: external impl blocked | `tests/boundaries/` | compile-fail | covered |
| Capability-gated publication (3 variants) | `tests/boundaries/` | compile-fail | covered |
| Raw query bypass unavailable on `QueryEffects` | `tests/boundaries/query_effects_raw_query_private.rs` | compile-fail | covered |
| WireEnvelope, FactEnvelope roundtrip | `tests/contracts/serialization_roundtrip.rs` | roundtrip | covered |
| DAG-CBOR canonical encoding (byte-exact) | `tests/contracts/serialization_roundtrip.rs` | hash stability | covered |
| Wire and fact decode reject non-canonical DAG-CBOR | `src/envelope.rs`, `src/types/facts.rs`, `src/util/serialization.rs` | inline strict-decode regression | covered |
| AuthorityId, DeviceId, SessionId uniqueness | `tests/contracts/identifier_uniqueness.rs` | pinned vectors | covered |
| DKD derivation determinism | `tests/contracts/dkd_determinism.rs` | determinism | covered |
| Content addressing (Hash32, ContentId) | `tests/contracts/content_addressing.rs` | roundtrip | covered |
| Context isolation (opaque, unlinkable IDs) | `tests/contracts/identifier_uniqueness.rs` | uniqueness | covered |
| FlowBudget charge-before-send | `src/types/flow.rs` | inline | covered |
| StoragePath wildcard coverage stays segment-aware | `src/types/scope.rs` | inline | covered |
| Consistency metadata at 10k scale | `tests/contracts/consistency_scaling.rs` | `#[ignore]` | covered |

## References

- [System Architecture](../../docs/001_system_architecture.md) — 8-layer structure, effect system
- [Theoretical Model](../../docs/002_theoretical_model.md) — semilattice semantics, context isolation
- [Privacy and Information Flow Contract](../../docs/003_information_flow_contract.md) — context privacy
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) — `InvariantContextIsolation`
- [Effect System](../../docs/103_effect_system.md) — effect trait design and handler rules
- [Ownership Model](../../docs/122_ownership_model.md) — ownership taxonomy, reactive contract
- [Testing Guide](../../docs/804_testing_guide.md) — ownership testing requirements

## Native timeout failure provenance

Required clock and sleep failures retain their original causes through `TimeoutBudgetError`, `TimeoutRunError`, and `RetryRunError`. A failed clock read cannot create deadline evidence. Serialized clock errors omit native sources. Source-bearing timeout errors no longer implement equality; policy uses exhaustive variants and stable codes. Converting deadline or attempt exhaustion to `AuraError` uses a source-bearing internal wrapper rather than the former source-less terminal string; the typed cause defines the outcome.

### Profile lifetime ownership and immutable secure publication

`ProfileStorageEffects` defines nonblocking storage-profile ownership through a non-Clone adapter resource lease. Ownership errors retain native sources; unsupported backends fail explicitly. The lease is infrastructure ownership and does not establish enrollment trust. `SecureStorageEffects::secure_store_immutable` distinguishes Created from AlreadyExists, never treating existence as equality or authorization. Created requires complete encrypted publication and durable acknowledgement; post-publication failures may leave a complete record requiring owned recovery. Default implementations report typed unsupported atomicity.

The public profile effect trait is an infrastructure adapter interface, not an unforgeable trust witness. Production runtime construction accepts only the selected audited adapter's concrete private lease type. Browser errors explicitly convert foreign JS objects into typed operation/name/message diagnostics; those values never authorize domain outcomes. Deadline exhaustion and historical plaintext profile ambiguity are distinct typed failures.

### Required deadline observations

Timeout budgets retain their original deadline and share a latched physical-clock high-water observation across clones and child budgets. Rollback after progress is a typed required-clock failure, even when the new observation remains above the start. Timer exhaustion and rollback survive validated serialization; legacy records lacking observation state fail closed. Nonblocking observation contention is a separate ownership fault. Snapshot and expiration updates acquire the same short-lived guards atomically and never hold guards across await.

### Required observation acknowledgment

The checkpointed timeout executor waits for the owner acknowledgment after each observation, including rollback and timer exhaustion, before polling the operation or returning its result. Cancellation before acknowledgment cannot continue the operation. Historical snapshot validation checks frozen bounds and sticky failure state without treating recorded time as a fresh clock observation; that pure check grants no authority.

### Native identity query failures

Device leaf metadata decoding preserves the canonical codec source inside `AuraError::Serialization`; an empty legacy metadata value remains explicit absence, while malformed nonempty metadata is a required-read failure.

## Shared window arithmetic

`types::window` owns sealed physical-millisecond and receipt-generation coordinates with validated half-open intervals. This pure contract proves bounds and domain separation, not admission or checkpoint provenance. Physical timeout ownership keeps its fixed deadline, sticky observations and required durable acknowledgment; flow allowance ownership keeps epoch progression and previous-window receipt policy. No common interval method renews a budget.

Required timeout checkpoints preserve storage and codec causes in `TimeoutBudgetError::CheckpointFailure`. This failure does not represent clock unavailability or elapsed time. Native and semantic classification follows its retained cause; unclassified checkpoint faults remain internal. Serialized diagnostics omit native error sources and grant no checkpoint authority.

### Canonical codec failure provenance

Required canonical serialization retains the original concrete ciborium failure through the standard error source chain. Display diagnostics keep their existing prefixes; pure canonical-format validation failures have no invented codec source. Canonical bytes and strict decoding rules are unchanged. Native semantic classification remains SerializationFailure, including when a codec cause is carried through storage or checkpoint context. Serialized presentation diagnostics do not establish native failure provenance.

### VM send custody contract

`VmBridgeEffects` exposes an observational pending-send snapshot and exclusive
`VmBridgeSendLease` acquisition. The move-owned lease acknowledges only an
in-flight frame; unknown delivery, concurrent ownership and invalid transitions
are distinct typed failures. No destructive queue-drain escape is exposed.

### Asynchronous timeout observation ordering

Timeout clock owners share a nonblocking observation lease across clones and
children. Required physical reads, high-water updates, and checkpoint
acknowledgments execute in that owner order. Pure snapshot guards remain short
and do not cross awaits. The async lease is released before the owned operation
or timer is awaited and on cancellation; it never renews a deadline or grants
admission authority. Deterministic interleaved-read tests distinguish genuine
rollback from delayed earlier queries.

### Required task outcome contract

`TaskSpawner` and `OwnedTaskSpawner` distinguish unit background work from required cancellable work returning `Result<(), AuraError>`. Required admission returns a native error immediately when the adapter cannot supervise it or the runtime owner has closed admission. Unsupported adapters cannot acknowledge successful required admission. The owned facade rejects unit futures for required work; its compile-fail guard enforces that distinction.

### Individual participant key proofs

The pure participant proof primitive establishes possession of one exact
participant key. It does not establish device membership, ceremony admission,
threshold agreement, or an authority signature. FROST shares retain their exact
canonical scalar encoding and use the ciphersuite standard Schnorr signing and
verification APIs. Runtime owners must independently bind the verifier to the
current participant inventory before accepting the proof.

Required secure retrieval preserves logical record absence as the concrete
`SecureStorageRecordMissing` cause within the storage category. Observational
`secure_exists` still reports absence as `false`; native provider failures
retain their original sources. Logical absence never manufactures an OS error
and never grants authority to select replacement signing material.

`TimeoutBudget` supports pure fixed-deadline attenuation without changing the
original start or observing a new clock value. The child shares the parent's
clock owner and retains separate sticky exhaustion. Restored attenuation checks
exact bounds and acknowledged parent/child high-water and rollback history;
it does not grant storage provenance, signing or runtime admission. Those remain
with the domain owner. Timeout law tests cover nonrenewal, independent child
exhaustion, restored shared rollback and inconsistent paired snapshots.

### Retained threshold backend policy

Retained FROST package validation uses the unchanged authenticated threshold and ordered participant count. Zero or oversized thresholds are invalid domain policy. A threshold-one policy is domain-valid but unsupported by the selected FROST backend, and has a distinct typed capability failure. Validation never changes the policy or manufactures a solo package. Deterministic dependency tests verify the backend's actual `InvalidMinSigners` error.

`crypto::signature_input` provides bounded pure parsing of public signature
inputs with concrete native decoder sources. It does not verify a signature or
mint signing, pinning, quorum or runtime ownership evidence.

Ownership compile-fail tests use the shared host-only descriptor lock from
`toolkit/test-support/process_lock.rs`, through the unpublished host-only aura-build-support test dependency.
They require Cargo and fail on bounded acquisition errors. No other Aura crate
is imported to provide foundational test locking.

Allocation lifetime references are routing observations. Provider recovery and
negative-decision custody are opaque, nonserializable owners; retirement dispatches
through the original retained backend, with no caller-selected receiver.

Shared `Arc<T>` crypto dispatch forwards every extended operation to the same
provider. Optional defaults do not substitute for a selected implementation.
Network adapter failures retain the original provider/guard cause rather than
reducing it to display text.

### Required fact envelope decoding

`try_decode_envelope` validates an existing envelope's expected type, explicit
schema range, payload bound and declared encoding. `try_decode_fact` delegates
to this same pure validator after strict envelope decoding. Declared JSON
failures retain their concrete `serde_json::Error` source in `FactError::Json`;
DAG-CBOR failures retain the canonical serialization source. Neither entry
point establishes commitment or authorizes materialization.

A timeout owner drops the losing operation future before reacquiring its shared
observation lease for final expiry/checkpoint acknowledgment. This prevents an
operation's cancelled clock query from retaining the gate needed by its own
timeout owner. Required source coverage exercises the actual delayed query,
deadline wake, cancellation drop and unchanged sticky original interval.

The physical deadline effect accepts a typed existing endpoint and cannot confer
domain completion. `acknowledge_with_timeout_budget` bounds the original final
observation lease, selected clock reads before and after the checkpoint, and
checkpoint against that endpoint. The final read rejects expiry or rollback
caused within the checkpoint's completion poll; publication is synchronous under
the guard after success. This local validity check grants no durable domain ACK
and does not claim its final high-water observation was persisted. Endpoint priority and
owned loser Drop prevent late checkpoint success and retained stalled reads.
Relative-only providers fail explicitly. `TimeError::source` dereferences native
provider causes rather than exposing an Arc container; serialization retains
only diagnostics. Required source/discovery/execution covers these boundaries.

`MAX_PROFILE_ALLOCATION_COUNT` is the explicit count bound for original protected
profile inventory. Its 4096 value bounds arithmetic/data shape, not ownership or
admission authority. The native style/unit gate enforces its count suffix.

Plain timeout execution and final observation acknowledgment share one bounded
observation primitive. The original selected-provider deadline bounds observation
gate acquisition, the initial clock read, operation execution, and the success
clock read. Deadline arbitration runs first and retains the provider's actual
`PhysicalTime` witness; losing owned work is dropped before expiration is
recorded. Plain observations grant neither domain completion nor durable checkpoint
authority. The checkpointed enrollment executor retains its distinct required
durability contract; a plain observation cannot replace its acknowledgment.

Initial publication uses `acknowledge_initial_publication_with_timeout_budget`:
the actual owner-provided publication and readback precede the first physical
clock read. One original deadline bounds gate acquisition, publication, and
subsequent selected-provider validation. The helper returns the caller's actual
acknowledgment only while that window remains valid; it constructs no domain or
durability proof. Publication failures retain their concrete operation error,
and cancellation drops the losing work and any unexposed acknowledgment.

`TransportEffects::wait_receive_ready` is required for every provider. Its
register-before-own-queue-check contract prevents lost arrivals and preserves
selected-provider custody. `ReceiveReadinessUnsupported` is an explicit refusal,
not a default adapter. See docs/111 "Receive readiness".
