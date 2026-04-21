# Aura Journal (Layer 2)

## Purpose

Fact-based journal semantics using join-semilattice CRDTs for deterministic conflict-free state reduction across distributed replicas.

## Scope

| Belongs here | Does not belong here |
|-------------|---------------------|
| Fact model, journal operations, namespace scoping | Storage implementations (`StorageEffects`) |
| Deterministic state reduction from facts | Multi-party coordination (`aura-protocol`) |
| Commitment tree state machine and compaction | Runtime composition (`aura-agent`) |
| CRDT handlers (join/meet/operation-based) | Direct OS access (use effect traits) |
| Extensibility: `DomainFact`, `FactReducer`, `FactRegistry` | |

## Dependencies

| Direction | Crate | What |
|-----------|-------|------|
| consumes | `aura-core` | Effect traits, domain types, semilattice traits, tree primitives |
| produces | Fact types | `Fact`, `FactContent`, `RelationalFact`, `ProtocolRelationalFact` |
| produces | Journal operations | `Journal`, `FactJournal`, `JournalNamespace` |
| produces | Reduction engine | `reduce_authority()`, `reduce_context()` |
| produces | Commitment tree | `TreeState`, `reduce()`, `apply_verified()` |
| produces | CRDT handlers | `CvHandler`, `MvHandler`, `CmHandler`, `DeltaHandler` |
| produces | Extensibility | `DomainFact`, `FactReducer`, `FactRegistry` |
| produces | Application effect | `JournalHandler` implementing `JournalEffects` |

## Key Modules

- `fact.rs`: Fact model, journal operations, namespace scoping.
- `reduction.rs`: Deterministic state derivation from facts.
- `commitment_tree/`: Tree state machine, reduction, compaction.
- `crdt/`: Semilattice handlers (join/meet/operation-based).
- `algebra/`: Domain-specific CRDT types (`OpLog`, `AccountState`, `EpochLog`).
- `extensibility.rs`: `DomainFact` trait, `FactRegistry` for Layer 4/5 facts.
- `effects.rs`: `JournalHandler` application effect implementation.

## Invariants

- Monotonic growth: `Journal_{t+1} = Journal_t ⊔ δ` (facts append-only).
- Deterministic reduction: Same facts → identical state on all replicas.
- Immutability: Facts immutable; metadata updates monotonic.
- Namespace isolation: Authority and Context journals distinct.
- Content addressing: Facts identified by hash (CID).
- Nonce uniqueness per namespace (`InvariantNonceUnique`).

### InvariantCRDTConvergence

Identical fact sets must reduce to identical state across replicas, independent of arrival order.

Enforcement locus:
- `src/reduction.rs`: `reduce_authority()` and `reduce_context()` are deterministic reducers.
- `src/reduction.rs`: validation path enforces structural and attestation preconditions.
- `src/semilattice/`: join semantics remain commutative, associative, and idempotent.
- `src/fact.rs`: namespace scoping prevents cross-context pollution during reduction.

Failure mode:
- Replica divergence for the same fact set.
- Inconsistent authority or relational state reconstruction.
- Non-deterministic replay behavior in simulation or recovery.

Verification hooks:
- `cargo test -p aura-journal convergence`
- `cargo test -p aura-simulator crdt_convergence_scenario`
- property tests for deterministic replay and permutation stability
- `quint run --invariant=InvariantNonceUnique verification/quint/journal/core.qnt`

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) defines join-semilattice semantics and deterministic reduction.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines journal CRDT safety, deterministic reduction order, and `InvariantNonceUnique`.
- [Journal](../../docs/105_journal.md#61-amp-channel-epoch-transition-reduction) defines AMP transition reduction states and single-live-successor exposure.

### InvariantAmpTransitionSingleLiveSuccessor

For one AMP channel parent prestate, context reduction may expose at most one
live successor. Proposal-only facts remain `Observed`; exactly one valid
unsuppressed A2 certificate becomes `A2Live`; conflicting A2 certificates or
conflict evidence become `A2Conflict`; and A3 finalization is exposed only when
durable evidence is unambiguous.

Enforcement locus:
- `src/reduction.rs`: `AmpTransitionParentKey` groups transition facts by parent prestate.
- `src/reduction.rs`: `AmpTransitionReductionStatus` exposes `Observed`, `A2Live`, `A2Conflict`, `A3Finalized`, `A3Conflict`, `Aborted`, and `Superseded`.
- `src/reduction.rs`: `ChannelEpochState::pending_bump` is derived only from an `A2Live` transition, not from proposal ordering.
- `src/fact.rs`: AMP transition facts bind the canonical `transition_id`.

Failure mode:
- Reducer tie-breaks between conflicting certificates.
- Live message acceptance uses a proposal without A2 evidence.
- Recovery replay derives a different live successor from the same fact set.

Verification hooks:
- `cargo test -p hxrts-aura-journal amp_single_a2_certificate_exposes_one_live_successor`
- `cargo test -p hxrts-aura-journal amp_conflicting_a2_certificates_expose_no_live_successor`
- `cargo test -p hxrts-aura-journal amp_a3_finalization_advances_durable_epoch`

### InvariantNonceUnique

No two accepted facts may share the same nonce within the same journal namespace.

Enforcement locus:
- `src/fact.rs`: namespace + nonce identity model for fact uniqueness.
- `src/reduction.rs`: validation/reduction path rejects duplicate nonce facts per namespace.
- `src/crdt/`: merge semantics preserve uniqueness constraints under replay and anti-entropy.

Failure mode:
- Replay acceptance for duplicate facts.
- Divergent reducers caused by nonce collisions.
- Ambiguous event identity in evidence/audit pipelines.

Verification hooks:
- `cargo test -p aura-journal nonce`
- `quint run --invariant=InvariantNonceUnique verification/quint/journal/core.qnt`

Contract alignment:
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) defines `InvariantNonceUnique`.
- [Project Structure](../../docs/999_project_structure.md#invariant-traceability) indexes canonical naming.

### InvariantAuthorityTreeTopologyCommitmentCoherence

Authority tree topology and commitment caches must remain coherent after every mutation.

Enforcement locus:
- `src/commitment_tree/authority_state.rs`: deterministic topology materialization.
- `src/commitment_tree/authority_state.rs`: dirty-path propagation and bottom-up commitment recompute.
- `src/commitment_tree/authority_state.rs`: topology invariant assertions and validation wrappers.
- `src/commitment_tree/authority_state.rs`: Merkle proof path updates derived from materialized topology.

Failure mode:
- Parent and child pointer inconsistency.
- Root commitment mismatch for equivalent state.
- Invalid Merkle proofs or non-deterministic branch indexing.

Verification hooks:
- `cargo test -p aura-journal --test authority_tree_correctness`
- differential checks against full recomputation
- randomized mutation sequence checks with invariant assertions

Contract alignment:
- [Theoretical Model](../../docs/002_theoretical_model.md) requires deterministic state transitions.
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) requires safety under replay and anti-entropy.

## Ownership Model

> Taxonomy: [Ownership Model](../../docs/122_ownership_model.md)

`aura-journal` is primarily `Pure`. Fact reduction and journal-domain semantics are deterministic. It does not own `ActorOwned` runtime state.

### Ownership Inventory

| Surface | Category | Notes |
|---------|----------|-------|
| `src/fact.rs`, `src/reduction.rs`, `src/extensibility.rs` | `Pure` | Canonical fact encoding, reduction, and registry semantics. |
| `src/algebra/`, `src/crdt/`, `src/commitment_tree/` | `Pure` | Deterministic algebra/CRDT/tree-state transitions. |
| `src/effect_api/` | `MoveOwned` | Typed journal append/intent/capability records and effect-facing handoff surfaces. |

### Capability-Gated Points

- journal append capability/effect surfaces in `src/effect_api/`
- fact acceptance and interpretation gates in typed journal/domain APIs

## Testing

### Strategy

aura-journal is the CRDT convergence layer — if reduction diverges, every replica disagrees on state irrecoverably. Testing priorities:

1. **Reducer determinism**: same facts → same state regardless of order
2. **CRDT convergence laws**: join associativity, commutativity, idempotence
3. **Nonce uniqueness**: replay prevention within namespaces
4. **Tree state machine integrity**: incremental updates match full recompute
5. **Fact encoding stability**: serialization doesn't drift between releases

### Commands

```
cargo test -p aura-journal --test convergence  # CRDT convergence + determinism
cargo test -p aura-journal --test contracts    # tree integrity + encoding stability
cargo test -p aura-journal --lib               # inline unit tests
```

### Coverage matrix

| What breaks if wrong | Test location | Status |
|---------------------|--------------|--------|
| Context reducer non-deterministic | `tests/convergence/journal_join_laws.rs` | covered (proptest) |
| Authority reducer non-deterministic | `tests/convergence/journal_join_laws.rs` | covered |
| Tree reduction non-deterministic | `tests/convergence/tree_reduction_determinism.rs` | covered (proptest) |
| Journal join not associative/commutative | `tests/convergence/journal_join_laws.rs` | covered (proptest) |
| Journal join not idempotent | `tests/convergence/journal_join_laws.rs` | covered (proptest) |
| Adding fact removes existing facts | `tests/convergence/journal_join_laws.rs` | covered |
| Convergence certificates not emitted | `tests/convergence/convergence_cert.rs` | covered |
| Tree topology incoherent after mutation | `tests/contracts/authority_tree_integrity.rs` | covered (proptest) |
| Incremental update diverges from recompute | `tests/contracts/authority_tree_integrity.rs` | covered (proptest) |
| Merkle proofs invalid after mutation | `tests/contracts/authority_tree_integrity.rs` | covered |
| Fact encoding changes between releases | `tests/contracts/fact_encoding_stability.rs` | covered |
| Fact encoding bytes drift silently | `tests/contracts/fact_encoding_stability.rs` | covered |
| Fact deduplication via BTreeSet identity | `src/fact.rs` inline | covered |
| Wrong namespace type accepted by reducer | `src/reduction.rs` inline | covered |
| AMP epoch reduction order-dependent | `src/reduction.rs` inline | covered |
| Recovery AMP reconstruction fails | `tests/contracts/recovery_amp_reconstruction.rs` | covered |

## References

- [Theoretical Model](../../docs/002_theoretical_model.md) — join-semilattice semantics, deterministic reduction
- [Distributed Systems Contract](../../docs/004_distributed_systems_contract.md) — CRDT safety, `InvariantNonceUnique`
- [Journal](../../docs/105_journal.md) — full journal specification
- [Project Structure](../../docs/999_project_structure.md) — fact pattern selection, invariant traceability
- [Ownership Model](../../docs/122_ownership_model.md) — ownership taxonomy
- [Testing Guide](../../docs/804_testing_guide.md) — test patterns and ownership testing requirements
