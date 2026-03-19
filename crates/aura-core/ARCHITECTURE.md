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

## Boundaries
- No handler implementations (those live in aura-effects).
- No protocol logic (that lives in aura-protocol).
- No application-specific types (those live in domain crates).
