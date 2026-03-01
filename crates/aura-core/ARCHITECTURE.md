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

## Detailed Invariant Specifications

### `CONTEXT_ISOLATION`
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

## Boundaries
- No handler implementations (those live in aura-effects).
- No protocol logic (that lives in aura-protocol).
- No application-specific types (those live in domain crates).
