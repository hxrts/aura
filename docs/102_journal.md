# Journal

This document describes the journal architecture and state reduction system in Aura. It explains how journals implement CRDT semantics, how facts are structured, and how reduction produces deterministic state for account authorities and relational contexts. It describes integration with the ledger layer and defines the invariants that ensure correctness. See [Maintenance](111_maintenance.md) for the end-to-end snapshot and garbage collection pipeline.

## 1. Journal Namespaces

Aura maintains a separate journal namespace for each authority and each relational context. A journal namespace stores all facts relevant to the entity it represents. A namespace is identified by an `AuthorityId` or a `ContextId`. No namespace shares state with another. Identifier definitions appear in [Identifiers and Boundaries](105_identifiers_and_boundaries.md).

A journal namespace evolves through fact insertion. Facts accumulate monotonically. No fact is removed except through garbage collection rules that preserve logical meaning.

```rust
pub struct Journal {
    pub facts: BTreeSet<Fact>,
}
```

This type defines a journal as a set of facts. The journal is a join semilattice under set union. Merging two journals produces a new journal containing all facts from both inputs.

## 2. Fact Model

Facts represent immutable events or operations that contribute to the state of a namespace. Facts have identifiers and content. Facts do not contain device identifiers or timestamps used for correctness.

```rust
pub struct Fact {
    pub fact_id: FactId,
    pub content: FactContent,
}

pub enum FactContent {
    AttestedOp(AttestedOp),
    Relational(RelationalFact),
    Snapshot(SnapshotFact),
}
```

This model supports account operations, relational context operations, and snapshots. Each fact is self contained. Facts are validated before insertion into a namespace.

## 3. Semilattice Structure

Journals use a join semilattice. The semilattice uses set union as the join operator. The partial order is defined by subset inclusion. The journal never removes facts during merge. Every merge operation increases or preserves the fact set.

The join semilattice ensures convergence across replicas. Any two replicas that exchange facts eventually converge to identical fact sets. All replicas reduce the same fact set to the same state.

```rust
impl Journal {
    pub fn merge(&self, other: &Journal) -> Journal {
        Journal { facts: self.facts.union(&other.facts).cloned().collect() }
    }
}
```

This merge function demonstrates set union across two fact sets. The result is monotonic and convergent.

## 4. Account Journal Reduction

Account journals store attested operations for ratchet tree updates. Reduction computes a `TreeState` from the fact set. Reduction applies only valid operations and resolves conflicts deterministically.

Reduction follows these steps.

1. Identify all `AttestedOp` facts.
2. Group operations by their referenced parent state.
3. Select a winning operation for each parent using a deterministic ordering.
4. Apply winners in parent order.
5. Compute commitments after each operation.

The result is a single `TreeState` for the account.

```rust
pub fn reduce_account(facts: &BTreeSet<Fact>) -> TreeState {
    // Reduction logic applies attested operations deterministically
    TreeState::default()
}
```

This function signature illustrates how reduction produces deterministic state. Implementations must follow defined rules for conflict resolution and validation.

## 5. RelationalContext Journal Reduction

Relational contexts store relational facts. These facts reference authority commitments. Reduction produces a `RelationalState` that captures the current relationship between authorities.

Reduction applies relational facts in the order defined by their dependencies. Aura Consensus may produce commit facts that include thresholds and signatures. Reduction verifies that relational facts reference current authority states.

```rust
pub fn reduce_context(facts: &BTreeSet<Fact>) -> RelationalState {
    // Reduction logic processes relational facts
    RelationalState::default()
}
```

This function signature shows how a relational state is derived. Implementations must ensure that relational facts reference valid authority commitments.

## 6. Snapshots and Garbage Collection

Snapshots summarize all prior facts. A snapshot fact contains a digest of the fact set at the time of creation. A snapshot establishes a high water mark. Facts older than the snapshot can be pruned.

Garbage collection removes pruned facts while preserving logical meaning. Pruning does not change the result of reduction. Pruning reduces storage requirements.

```rust
pub struct SnapshotFact {
    pub digest: Hash32,
}
```

This structure defines a snapshot. The digest represents a summary of the fact set below the snapshot.

## 7. Ledger Integration

The ledger layer stores facts durably. The ledger exposes minimal operations such as append and read. The ledger does not define logical meaning. The journal and reduction logic provide semantics.

The ledger writes facts to persistent storage. Replica synchronization loads facts from the ledger into journal memory. The ledger guarantees durability but does not affect CRDT merge semantics.

```rust
#[async_trait]
pub trait LedgerEffects {
    async fn append_fact(&self, fact: Fact) -> Result<FactId>;
    async fn read_facts(&self, namespace: &str) -> Result<Vec<Fact>>;
}
```

This trait defines minimal storage operations. The journal uses these operations to persist and retrieve facts.

## 8. Invariants

The journal and reduction architecture satisfy several invariants. Convergence ensures all replicas reach the same state when they have the same facts. Idempotence ensures that repeated merges or reductions do not change state. Determinism ensures that reduction produces the same output for all replicas.

These invariants guarantee correct distributed behavior. They also support offline operation with eventual consistency. They form the foundation for Aura's account and relational context state machines.

## 9. Fact Validation Pipeline

Every fact inserted into a journal must be validated before merge. The following steps outline the required checks and the effect traits responsible for each fact type:

### 9.1 AttestedOp Facts

**Checks**
- Verify the threshold signature (`agg_sig`) against the parent commitment and epoch.
- Ensure the referenced parent state exists locally; otherwise request missing facts.
- Confirm the operation is well-formed (e.g., `AddLeaf` indexes a valid parent node).

**Responsible Effects**
- `CryptoEffects` for signature verification.
- `JournalEffects` for parent lookup and conflict detection.
- `LedgerEffects` to persist the fact once validated.

### 9.2 Relational Facts

**Checks**
- Validate that each authority commitment referenced in the fact matches the current reduced state (`AuthorityState::root_commitment`).
- Verify Aura Consensus proofs if present (guardian bindings, recovery grants).
- Enforce application-specific invariants (e.g., no duplicate guardian bindings).

**Responsible Effects**
- `AuthorizationEffects` / `RelationalEffects` for context membership checks.
- `CryptoEffects` for consensus proof verification.
- `JournalEffects` for context-specific merge.

### 9.3 FlowBudget Facts

**Checks**
- Ensure `spent` deltas are non-negative and reference the active epoch for the `(ContextId, peer)` pair.
- Reject facts that would decrease the recorded `spent` (monotone requirement).
- Validate receipt signatures associated with the charge (see `108_transport_and_information_flow.md`).

**Responsible Effects**
- `FlowBudgetEffects` (or FlowGuard) produce the fact and enforce monotonicity before inserting.
- `JournalEffects` gate insertion to prevent stale epochs from updating headroom.

### 9.4 Snapshot Facts

**Checks**
- Confirm the snapshot digest matches the hash of all facts below the snapshot.
- Ensure no newer snapshot already exists for the namespace.
- Verify that pruning according to the snapshot does not remove facts still referenced by receipts or pending consensus operations.

**Responsible Effects**
- `JournalEffects` compute and validate snapshot digests.
- `LedgerEffects` persist the snapshot atomically with pruning metadata.

By clearly separating validation responsibilities, runtime authors know which effect handlers must participate before a fact mutation is committed. This structure keeps fact semantics consistent across authorities and contexts.
