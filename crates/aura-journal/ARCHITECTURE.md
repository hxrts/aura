# Aura Journal (Layer 2) - Architecture and Invariants

## Purpose
Define fact-based journal semantics using join-semilattice CRDTs for deterministic
conflict-free state reduction across distributed replicas.

## Inputs
- `aura-core`: Effect traits, domain types, semilattice traits, tree primitives.

## Outputs
- Fact types: `Fact`, `FactContent`, `RelationalFact`, `ProtocolRelationalFact`.
- Journal operations: `Journal`, `FactJournal`, `JournalNamespace`.
- Reduction engine: `reduce_authority()`, `reduce_context()`.
- Commitment tree: `TreeState`, `reduce()`, `apply_verified()`.
- CRDT handlers: `CvHandler`, `MvHandler`, `CmHandler`, `DeltaHandler`.
- Extensibility: `DomainFact`, `FactReducer`, `FactRegistry`.
- Application effect: `JournalHandler` implementing `JournalEffects`.

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

### Detailed Specifications

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
- [Project Structure](../../docs/999_project_structure.md#invariant-traceability) indexes canonical naming used by tests and proofs.

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

## Boundaries
- No storage implementations (use `StorageEffects`).
- No multi-party coordination (use `aura-protocol`).
- No runtime composition (use `aura-agent`).
- No direct OS access (use effect traits).

## Fact Pattern Summary
- **Protocol facts** (`ProtocolRelationalFact`): Core protocol, stay in aura-journal.
- **Domain facts** (`Generic` + registry): Layer 4/5 crates, register with `FactRegistry`.
- **Layer 2 crates**: Use `aura_core::types::facts`, no aura-journal dependency.
