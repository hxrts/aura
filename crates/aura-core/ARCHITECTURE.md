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
- Algebraic types: `Cap` (meet-semilattice), `Fact` (join-semilattice).
- Cryptographic utilities: key derivation, FROST types, merkle trees.
- Time system: Physical/Logical/Order/Range clocks.
- Guard types: `GuardSnapshot`, `EffectCommand`, `GuardOutcome`.

## Invariants
- Zero internal dependencies (foundation constraint).
- Effect trait definitions only (no implementations).
- Semilattice laws: monotonic growth (facts), monotonic restriction (capabilities).
- Context isolation prevents cross-context information flow.

## Boundaries
- No handler implementations (those live in aura-effects).
- No protocol logic (that lives in aura-protocol).
- No application-specific types (those live in domain crates).
