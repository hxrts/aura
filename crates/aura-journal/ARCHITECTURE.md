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

## Boundaries
- No storage implementations (use `StorageEffects`).
- No multi-party coordination (use `aura-protocol`).
- No runtime composition (use `aura-agent`).
- No direct OS access (use effect traits).

## Fact Pattern Summary
- **Protocol facts** (`ProtocolRelationalFact`): Core protocol, stay in aura-journal.
- **Domain facts** (`Generic` + registry): Layer 4/5 crates, register with `FactRegistry`.
- **Layer 2 crates**: Use `aura_core::types::facts`, no aura-journal dependency.
