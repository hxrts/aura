# Aura Journal (Layer 2) - Architecture and Invariants

## Purpose
Define fact-based journal semantics using join semilattices, enabling deterministic
conflict-free state reduction across distributed replicas.

## Inputs
- aura-core (domain types, effect traits, algebraic types).

## Outputs
- Fact types and join-semilattice operations.
- Journal API: `Journal`, `CommittedFact`, `JournalFact`.
- Reduction engine: `reduce_authority`, `reduce_context`, `TreeState`.
- Fact registry with extensibility infrastructure.
- CRDT types: `AccountState`, `EpochLog`, `GuardianRegistry`.

## Invariants
- Monotonic growth: `Fₜ₊₁ = Fₜ ⊔ δ`.
- Identical fact sets produce identical states (deterministic reduction).
- Facts are immutable and attested.
- Authority-scoped and context-scoped journals.

## Boundaries
- No storage implementations (use JournalEffects).
- No protocol coordination (use aura-protocol).
- Reduction is pure; I/O happens via effect traits.
