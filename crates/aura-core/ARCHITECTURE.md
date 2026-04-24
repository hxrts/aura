# Aura Core (Layer 1)

## Purpose

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

- Zero internal dependencies (foundation constraint).
- Effect trait definitions only (no implementations).
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
