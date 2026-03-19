# Aura Core (Layer 1) - Architecture and Invariants

## Purpose
Single source of truth for domain types and effect trait definitions. Provides
foundational algebraic types with zero dependencies on other Aura crates.

## Inputs
- External libraries only (no internal Aura dependencies).

## Outputs
- Effect trait definitions (infrastructure: Crypto, Network, Storage, Time, Random).
- Effect trait definitions (application: Journal, Authorization, FlowBudget, Leakage).
- Domain types: `AuthorityId`, `ContextId`, `SessionId`, `FlowBudget`.
- Algebraic types: `Cap` (meet-semilattice), `Fact` (join-semilattice), `Journal`.
- Cryptographic utilities: key derivation, FROST types, merkle trees.
- Tree types: `TreeOp`, `AttestedOp`, `Policy`, `LeafNode`, commitment functions.
- Time system: Physical/Logical/Order/Range clocks with `TimeStamp` variants.
- Query types: `Query` trait, Datalog types for unified query execution.
- Message types: `WireEnvelope`, versioning, validation.
- Ceremony types: Category C operation lifecycle.

## Invariants
- Zero internal dependencies (foundation constraint).
- Effect trait definitions only (no implementations).
- Semilattice laws: monotonic growth (facts), monotonic restriction (capabilities).
- Context isolation prevents cross-context information flow.

## Ownership Model

- `aura-core` is primarily `Pure`.
- It defines the canonical ownership split through:
  - `actor_owned::*` for long-lived task/mailbox ownership
  - `move_owned::*` for consumed workflow, handoff, and terminal-publication
    surfaces
  - `capability_gated::*` for minting and publication authority
- It defines shared `MoveOwned` vocabulary for higher layers such as opaque
  handles, owner tokens, transfer records, `OperationContext`, consumed
  `TerminalPublisher`, `OwnerEpoch`, and `PublicationSequence`.
- It defines capability-gated authority wrappers for progress publication,
  terminal publication, readiness publication, actor-ingress mutation, and
  operation-context issuance.
- It must not own `ActorOwned` runtime state.
- Capability-gated boundaries should be expressible in core types and traits,
  not bypassed by helper conventions in higher layers.
- Downstream `Observed` layers consume these contracts but must not mutate or
  republish semantic truth through them.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `src/ownership.rs` | `MoveOwned` + capability-gated + actor-owned vocabulary | Canonical `OperationContext`, consumed `TerminalPublisher`, exact progress/terminal publication wrappers, owner tokens, handoff records, owned spawner/shutdown wrappers, and explicit `actor_owned` / `move_owned` / `capability_gated` module layout. |
| `src/time/timeout.rs` | `MoveOwned` | Typed timeout budgets, attempt budgets, and retry/backoff policy. These are consumed local-owner policy objects, not distributed semantic clocks. `OperationTimeoutBudget` is the workflow-facing wrapper. |
| `src/effects/` | `Pure` | Effect traits and trait-level helper surfaces only. No long-lived owner state or runtime mutation. |
| `src/domain/`, `src/types/`, `src/query.rs`, `src/messages/`, `src/tree/`, `src/crypto/` | `Pure` | Value-level domain/state/query/message/crypto contracts. |
| Actor-owned runtime state | none | `aura-core` must not grow long-lived async owner state. |
| Observed-only surfaces | none | Observation belongs in higher layers. |

### Capability-Gated Points

- operation-context issuance in `src/ownership.rs`
- progress publication wrappers in `src/ownership.rs`
- terminal publication wrappers in `src/ownership.rs`
- readiness publication wrappers in `src/ownership.rs`
- actor-ingress mutation wrappers in `src/ownership.rs`
- ownership token issuance requests in `src/ownership.rs`

### Verification Hooks

- `cargo check -p aura-core`
- `cargo test -p aura-core --lib ownership_ -- --nocapture`
- `cargo test -p aura-core --test compile_fail -- --nocapture`

### Detailed Specifications

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

## Testing

### Strategy

aura-core is the foundation for a threshold-cryptographic P2P identity system.
If its invariants break, every crate above it is silently unsound. Testing
priorities follow the blast radius of a failure, not the amount of code:

1. **Cryptographic commitment correctness** (`tests/laws/`, inline). A wrong
   commitment hash means a forged tree operation passes verification. A wrong
   binding message means signatures can be replayed across groups. These are
   the highest-consequence bugs in the system and the hardest to detect in
   production.

2. **Algebraic laws** (`tests/laws/`). If a semilattice law is violated, CRDTs
   diverge across peers and state never converges. Property tests with proptest
   verify associativity, commutativity, idempotence, and monotonicity. Every
   `JoinSemilattice` and `MeetSemilattice` implementation must have law
   coverage — not just the primitives but the domain types built on them.

3. **Ownership boundaries** (`tests/boundaries/`). If a boundary can be
   bypassed, the ownership model is advisory. Compile-fail tests with trybuild
   verify private constructors, consumed publishers, sealed traits, and
   capability gates. Every new private constructor or sealed trait in
   `ownership.rs` needs a corresponding boundary test.

4. **Serialization determinism** (`tests/contracts/`). Peers must agree on
   byte-level encoding for threshold signatures (FROST requires identical
   binding messages) and content addressing (fact hashes must be reproducible).
   Roundtrip tests are necessary but not sufficient — pinned test vectors that
   lock specific bytes for specific inputs are the real contract.

5. **Identifier and key derivation stability** (`tests/contracts/`). If
   derivation changes, all existing journals, keys, and channel bindings break.
   Pinned vectors (known input → known output) prevent silent drift.

6. **Time system ordering** (`tests/laws/`). If time ordering is wrong, causal
   consistency and privacy-preserving ordering both fail. All four clock
   domains need ordering law coverage.

### Test placement rule

If the test would still be meaningful after a complete rewrite of the module
internals, it belongs in `tests/`. If it tests a specific function's return
value for a specific input, it belongs inline.

### Coverage matrix

| What breaks if wrong | Test location | Method | Status |
|---------------------|--------------|--------|--------|
| **Cryptographic commitments** | | | |
| Branch/leaf commitment determinism | `src/tree/commitment.rs` | inline pinned | covered |
| Binding message includes group pubkey | `src/tree/verification.rs` | inline | covered |
| FROST sign → aggregate → verify | `src/crypto/tree_signing.rs` | inline | covered |
| Commitment changes when any input changes | `src/tree/commitment.rs` | inline differential | covered |
| Signature replay across groups blocked | `src/tree/verification.rs` | inline | covered |
| **Algebraic laws** | | | |
| JoinSemilattice — u64, Vec, BTreeMap | `tests/laws/semilattice_join.rs` | example | covered |
| MeetSemilattice — u64, BTreeSet | `tests/laws/semilattice_meet.rs` | proptest | covered |
| FlowBudget CRDT — join, merge, convergence | `tests/laws/flow_budget_crdt.rs` | proptest | covered |
| Policy meet-semilattice | `tests/laws/tree_policy_meet.rs` | proptest | covered |
| Time ordering across clock domains | `tests/laws/time_ordering.rs` | proptest | minimal (2 props) |
| JoinSemilattice — Fact, FactValue | `tests/laws/semilattice_join.rs` | example | covered |
| MeetSemilattice — Cap | `tests/laws/semilattice_meet.rs` | example + Biscuit | covered |
| FlowBudget epoch rotation monotonicity | `tests/laws/flow_budget_crdt.rs` | example | covered |
| **Ownership boundaries** | | | |
| TerminalPublisher: not clonable, no double publish | `tests/boundaries/` | compile-fail | covered |
| OperationContext: private constructor | `tests/boundaries/` | compile-fail | covered |
| OwnerToken: stale after handoff | `tests/boundaries/` | compile-fail | covered |
| Sealed owner traits: external impl blocked | `tests/boundaries/` | compile-fail | covered |
| Capability-gated publication (3 variants) | `tests/boundaries/` | compile-fail | covered |
| **Serialization determinism** | | | |
| WireEnvelope, FactEnvelope roundtrip | `tests/contracts/serialization_roundtrip.rs` | roundtrip | covered |
| FlowCost, FlowNonce, ReceiptSig roundtrip | `tests/contracts/serialization_roundtrip.rs` | roundtrip | covered |
| OwnershipCategory roundtrip | `tests/contracts/serialization_roundtrip.rs` | roundtrip | covered |
| TimeStamp variant roundtrip | `tests/contracts/serialization_roundtrip.rs` | roundtrip | covered |
| DAG-CBOR canonical encoding (byte-exact) | `tests/contracts/serialization_roundtrip.rs` | pinned length | partial |
| **Identifier and key derivation** | | | |
| AuthorityId, DeviceId, SessionId uniqueness | `tests/contracts/identifier_uniqueness.rs` | example + determ. | covered |
| DKD derivation determinism | `tests/contracts/dkd_determinism.rs` | determinism | covered |
| Content addressing (Hash32, ContentId) | `tests/contracts/content_addressing.rs` | roundtrip | covered |
| Pinned string format + entropy derivation | `tests/contracts/identifier_uniqueness.rs` | pinned vectors | covered |
| **Domain invariants** | | | |
| Context isolation (no cross-context fact leakage) | — | — | **missing** — needs L1 test |
| FlowBudget charge-before-send | `src/types/flow.rs` | inline | covered |
| Epoch monotonicity (no regression) | `tests/laws/flow_budget_crdt.rs` | example | covered |
| **Scaling** | | | |
| Consistency metadata at 10k scale | `tests/contracts/consistency_scaling.rs` | `#[ignore]` | covered |

### Running tests

```
cargo test -p aura-core                    # all tests
cargo test -p aura-core --test laws        # algebraic laws only
cargo test -p aura-core --test contracts   # API contracts only
cargo test -p aura-core --test compile_fail # ownership boundaries only
cargo test -p aura-core --lib              # inline unit tests only
```

## Boundaries
- No handler implementations (those live in aura-effects).
- No protocol logic (that lives in aura-protocol).
- No application-specific types (those live in domain crates).
