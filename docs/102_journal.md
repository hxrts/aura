# Journal

This document describes the journal architecture and state reduction system in Aura. It explains how journals implement CRDT semantics, how facts are structured, and how reduction produces deterministic state for account authorities and relational contexts. It describes integration with the effect system and defines the invariants that ensure correctness. See [Maintenance](111_maintenance.md) for the end-to-end snapshot and garbage collection pipeline.

## Hybrid Journal Model (Facts + Capabilities)

Aura’s journal state is a composite of:
- **Fact Journal**: the canonical, namespaced CRDT of immutable facts.
- **Capability Frontier**: the current capability lattice for the namespace.
- **JournalState**: a composite view used by effects/runtime to carry both together.

The fact journal is stored and merged as a semilattice. Capabilities are refined via meet. The runtime always treats these as orthogonal dimensions of state.

## 1. Journal Namespaces

Aura maintains a separate journal namespace for each authority and each [relational context](103_relational_contexts.md). A journal namespace stores all facts relevant to the entity it represents. A namespace is identified by an `AuthorityId` (see [Authority and Identity](100_authority_and_identity.md)) or a `ContextId` and no namespace shares state with another. Identifier definitions appear in [Identifiers and Boundaries](105_identifiers_and_boundaries.md).

A journal namespace evolves through fact insertion. Facts accumulate monotonically. No fact is removed except through garbage collection rules that preserve logical meaning.

```rust
pub struct Journal {
    pub namespace: JournalNamespace,
    pub facts: BTreeSet<Fact>,
}

pub enum JournalNamespace {
    Authority(AuthorityId),
    Context(ContextId),
}
```

This type defines a journal as a namespaced set of facts. The namespace identifies whether this journal tracks an authority's commitment tree or a relational context. The journal is a join semilattice under set union where merging two journals produces a new journal containing all facts from both inputs. Journals with different namespaces cannot be merged.

## 2. Fact Model

Facts represent immutable events or operations that contribute to the state of a namespace. Facts have ordering tokens, timestamps, and content. Facts do not contain device identifiers used for correctness.

```rust
pub struct Fact {
    pub order: OrderTime,
    pub timestamp: TimeStamp,
    pub content: FactContent,
}

pub enum FactContent {
    AttestedOp(AttestedOp),
    Relational(RelationalFact),
    Snapshot(SnapshotFact),
    RendezvousReceipt {
        envelope_id: [u8; 32],
        authority_id: AuthorityId,
        timestamp: TimeStamp,
        signature: Vec<u8>,
    },
}
```

The `order` field provides an opaque, privacy-preserving total order for deterministic fact ordering in the BTreeSet. The `timestamp` field provides semantic time information for application logic. Facts implement `Ord` via the `OrderTime` field. Do not use `TimeStamp` for cross-domain indexing or total ordering; use `OrderTime` or consensus/session sequencing.

This model supports account operations, relational context operations, snapshots, and rendezvous receipts. Each fact is self contained. Facts are validated before insertion into a namespace.

## 2.2 Protocol-Level vs Domain-Level Relational Facts

`RelationalFact` has only two variants:

- `Protocol(ProtocolRelationalFact)`: Protocol-level facts that must live in `aura-journal` because reduction semantics depend on them.
- `Generic { .. }`: Extensibility hook for domain facts (`DomainFact` + `FactReducer`).

**Criteria for ProtocolRelationalFact** (all must hold):

1. **Reduction-coupled**: the fact directly affects core reduction invariants in `reduce_context()` (not just a view).
2. **Cross-domain**: the fact’s semantics are shared across multiple protocols or layers.
3. **Non-derivable**: the state cannot be reconstructed purely via `FactReducer` + `RelationalFact::Generic`.

If a fact does **not** meet all three criteria, it must be implemented as a domain fact and stored via `RelationalFact::Generic`.

**Enforcement**:

- All protocol facts are defined in `crates/aura-journal/src/protocol_facts.rs`.
- Any new protocol fact requires a doc update in this section and a matching reduction rule.

## 2.1 Domain Fact Contract (Checklist + Lint)

Domain facts are the extensibility mechanism for Layer 2 crates. Every domain fact must follow this contract to ensure cross-replica determinism and schema stability:

- **Type ID**: define a `*_FACT_TYPE_ID` constant (unique, registered in `crates/aura-agent/src/fact_types.rs`).
- **Schema version**: specify a schema version (via `#[domain_fact(schema_version = N)]` or `*_FACT_SCHEMA_VERSION`).
- **Canonical encoding**: use `#[derive(DomainFact)]` or explicit `encode_domain_fact` / `VersionedMessage` helpers.
- **Context derivation**: declare `context` / `context_fn` for `DomainFact` or implement a stable `context_id()` derivation.
- **Reducer registration**: provide a `FactReducer` and register it in the central registry (`crates/aura-agent/src/fact_registry.rs`).

Lint: run `scripts/check-domain-fact-contract.sh` to validate these requirements.

## 3. Semilattice Structure

Journals use a join semilattice. The semilattice uses set union as the join operator with partial order defined by subset inclusion. The journal never removes facts during merge. Every merge operation increases or preserves the fact set.

The join semilattice ensures convergence across replicas. Any two replicas that exchange facts eventually converge to identical fact sets. All replicas reduce the same fact set to the same state.

```rust
impl JoinSemilattice for Journal {
    fn join(&self, other: &Self) -> Self {
        assert_eq!(self.namespace, other.namespace);
        let mut merged_facts = self.facts.clone();
        merged_facts.extend(other.facts.clone());
        Self {
            namespace: self.namespace.clone(),
            facts: merged_facts,
        }
    }
}
```

This merge function demonstrates set union across two fact sets. The namespace assertion ensures only compatible journals merge. The result is monotonic and convergent.

## 4. Account Journal Reduction

Account journals store attested operations for commitment tree updates. Reduction computes a `TreeStateSummary` from the fact set. Reduction applies only valid operations and resolves conflicts deterministically.

```rust
pub struct TreeStateSummary {
    epoch: Epoch,
    commitment: Hash32,
    threshold: u16,
    device_count: u32,
}
```

The `TreeStateSummary` is a lightweight public view that hides internal device structure. For the full internal representation with branches, leaves, and topology, see `TreeState` in `aura-journal::commitment_tree`.

Reduction follows these steps:

1. Identify all `AttestedOp` facts.
2. Group operations by their referenced parent state (epoch + commitment).
3. For concurrent operations (same parent), select winner using max hash tie-breaking: `H(op)` comparison.
4. Apply winners in topological order respecting parent dependencies.
5. Recompute commitments bottom-up after each operation.

The max hash conflict resolution ensures determinism:

```rust
fn resolve_conflict(ops: &[AttestedOp]) -> &AttestedOp {
    // Sort by hash of operation, take maximum
    ops.iter().max_by_key(|op| hash_op(op)).unwrap()
}
```

The result is a single `TreeStateSummary` for the account:

```rust
pub fn reduce_authority(journal: &Journal) -> Result<AuthorityState, ReductionError> {
    // Extract AttestedOp facts, apply in deterministic order
    // Returns AuthorityState { tree_state, facts }
}
```

## 5. RelationalContext Journal Reduction

Relational contexts store relational facts. These facts reference authority commitments. Reduction produces a `RelationalState` that captures the current relationship between authorities.

```rust
pub struct RelationalState {
    pub bindings: Vec<RelationalBinding>,
    pub flow_budgets: BTreeMap<(AuthorityId, AuthorityId, u64), u64>,
    pub leakage_budget: LeakageBudget,
    pub channel_epochs: BTreeMap<ChannelId, ChannelEpochState>,
}

pub struct RelationalBinding {
    pub binding_type: RelationalBindingType,
    pub context_id: ContextId,
    pub data: Vec<u8>,
}
```

This structure represents the reduced relational state. It contains relational bindings, flow budget tracking between authorities, leakage budget totals for privacy accounting, and AMP channel epoch state for message ratcheting.

Reduction processes the following protocol fact types wrapped in `Protocol(...)`:

1. `GuardianBinding` maps to `RelationalBinding` for guardian relationships
2. `RecoveryGrant` creates recovery permission bindings between authorities
3. `Consensus` stores generic bindings with consensus metadata
4. `AmpChannelCheckpoint` anchors channel state snapshots for AMP messaging
5. `AmpProposedChannelEpochBump` and `AmpCommittedChannelEpochBump` track channel epoch transitions
6. `AmpChannelPolicy` defines channel-specific policy overrides for skip windows
7. `DkgTranscriptCommit` stores consensus-finalized DKG transcripts
8. `ConvergenceCert` records soft-safe convergence certificates
9. `ReversionFact` tracks explicit reversion events
10. `RotateFact` marks lifecycle rotation or upgrade events

Domain-specific facts use `Generic { context_id, binding_type, binding_data }` and are reduced by registered `FactReducer` implementations

```rust
pub fn reduce_context(journal: &Journal) -> Result<RelationalState, ReductionError> {
    // Process relational facts, build bindings and channel state
}
```

Reduction verifies that relational facts reference valid authority commitments and applies them in dependency order.

## 6. Snapshots and Garbage Collection

Snapshots summarize all prior facts. A snapshot fact contains a state hash, the list of superseded facts, and a sequence number. A snapshot establishes a high water mark. Facts older than the snapshot can be pruned.

```rust
pub struct SnapshotFact {
    pub state_hash: Hash32,
    pub superseded_facts: Vec<OrderTime>,
    pub sequence: u64,
}
```

Garbage collection removes pruned facts while preserving logical meaning. Pruning does not change the result of reduction. The GC algorithm uses safety margins to prevent premature pruning:

- **Default skip window**: 1024 generations
- **Safety margin**: `skip_window / 2`
- **Pruning boundary**: `max_generation - (2 * skip_window) - safety_margin`

Helper functions determine what can be pruned:

```rust
pub fn compute_checkpoint_pruning_boundary(max_gen: u64, skip_window: Option<u64>) -> u64;
pub fn can_prune_checkpoint(checkpoint: &AmpCheckpoint, boundary: u64) -> bool;
pub fn can_prune_proposed_bump(bump: &ProposedBump, committed_gen: u64) -> bool;
```

## 7. Journal Effects Integration

The effect system provides journal operations through `JournalEffects`. This trait handles persistence, merging, and flow budget tracking:

```rust
#[async_trait]
pub trait JournalEffects: Send + Sync {
    async fn merge_facts(&self, target: &Journal, delta: &Journal) -> Result<Journal, AuraError>;
    async fn refine_caps(&self, target: &Journal, refinement: &Journal) -> Result<Journal, AuraError>;
    async fn get_journal(&self) -> Result<Journal, AuraError>;
    async fn persist_journal(&self, journal: &Journal) -> Result<(), AuraError>;
    async fn get_flow_budget(&self, context: &ContextId, peer: &AuthorityId) -> Result<FlowBudget, AuraError>;
    async fn update_flow_budget(&self, context: &ContextId, peer: &AuthorityId, budget: &FlowBudget) -> Result<FlowBudget, AuraError>;
    async fn charge_flow_budget(&self, context: &ContextId, peer: &AuthorityId, cost: u32) -> Result<FlowBudget, AuraError>;
}
```

The effect layer writes facts to persistent storage. Replica synchronization loads facts through effect handlers into journal memory. The effect layer guarantees durability but does not affect CRDT merge semantics.

## 8. AttestedOp Structure

AttestedOp exists in two layers with different levels of detail:

**Layer 1 (aura-core)** - Full operation metadata:

```rust
pub struct AttestedOp {
    pub op: TreeOp,
    pub agg_sig: Vec<u8>,
    pub signer_count: u16,
}

pub struct TreeOp {
    pub parent_epoch: Epoch,
    pub parent_commitment: TreeHash32,
    pub op: TreeOpKind,
    pub version: u16,
}
```

**Layer 2 (aura-journal)** - Flattened for journal storage:

```rust
pub struct AttestedOp {
    pub tree_op: TreeOpKind,
    pub parent_commitment: Hash32,
    pub new_commitment: Hash32,
    pub witness_threshold: u16,
    pub signature: Vec<u8>,
}
```

The aura-core version includes epoch and version for full verification. The aura-journal version includes computed commitments for efficient reduction.

## 9. Invariants

The journal and reduction architecture satisfy several invariants:

- **Convergence**: All replicas reach the same state when they have the same facts
- **Idempotence**: Repeated merges or reductions do not change state
- **Determinism**: Reduction produces identical output for identical input across all replicas
- **No HashMap iteration**: Uses BTreeMap for deterministic ordering
- **No system time**: Uses OrderTime tokens for ordering
- **No floating point**: All arithmetic is exact

These invariants guarantee correct distributed behavior. They also support offline operation with eventual consistency. They form the foundation for Aura's account and relational context state machines.

## 10. Fact Validation Pipeline

Every fact inserted into a journal must be validated before merge. The following steps outline the required checks and the effect traits responsible for each fact type:

### 10.1 AttestedOp Facts

**Checks**
- Verify the threshold signature (`agg_sig`) using the two-phase verification model from `aura-core::tree::verification`:
  - `verify_attested_op()`: Cryptographic signature check against `BranchSigningKey` stored in TreeState
  - `check_attested_op()`: Full verification plus state consistency (epoch, parent commitment)
- Ensure the referenced parent state exists locally; otherwise request missing facts.
- Confirm the operation is well-formed (e.g., `AddLeaf` indexes a valid parent node).

See [Tree Operation Verification](101_accounts_and_commitment_tree.md#41-tree-operation-verification) for details on the verify/check model and binding message security.

**Responsible Effects**
- `CryptoEffects` for FROST signature verification via `verify_attested_op()`.
- `JournalEffects` for parent lookup, state consistency via `check_attested_op()`, and conflict detection.
- `StorageEffects` to persist the fact once validated.

### 10.2 Relational Facts

**Checks**
- Validate that each authority commitment referenced in the fact matches the current reduced state (`AuthorityState::root_commitment`).
- Verify Aura Consensus proofs if present (guardian bindings, recovery grants).
- Enforce application-specific invariants (e.g., no duplicate guardian bindings).

**Responsible Effects**
- `AuthorizationEffects` / `RelationalEffects` for context membership checks.
- `CryptoEffects` for consensus proof verification.
- `JournalEffects` for context-specific merge.

### 10.3 FlowBudget Facts

**Checks**
- Ensure `spent` deltas are non-negative and reference the active epoch for the `(ContextId, peer)` pair.
- Reject facts that would decrease the recorded `spent` (monotone requirement).
- Validate receipt signatures associated with the charge (see `108_transport_and_information_flow.md`).

**Responsible Effects**
- `FlowBudgetEffects` (or FlowGuard) produce the fact and enforce monotonicity before inserting.
- `JournalEffects` gate insertion to prevent stale epochs from updating headroom.

### 10.4 Snapshot Facts

**Checks**
- Confirm the snapshot `state_hash` matches the hash of all facts below the snapshot.
- Ensure no newer snapshot already exists for the namespace (check `sequence` number).
- Verify that pruning according to the snapshot does not remove facts still referenced by receipts or pending consensus operations.

**Responsible Effects**
- `JournalEffects` compute and validate snapshot digests.
- `StorageEffects` persist the snapshot atomically with pruning metadata.

By clearly separating validation responsibilities, runtime authors know which effect handlers must participate before a fact mutation is committed. This structure keeps fact semantics consistent across authorities and contexts.

## 11. Consistency Metadata Schema

Facts carry consistency metadata for tracking agreement level, propagation status, and acknowledgments. See [Consistency Metadata](121_consistency_metadata.md) for the full type definitions.

### 11.1 Fact Schema Fields

```rust
pub struct Fact {
    pub order: OrderTime,
    pub timestamp: TimeStamp,
    pub content: FactContent,

    // Consistency metadata (added via serde defaults for backwards compat)
    #[serde(default)]
    pub agreement: Agreement,      // Provisional, SoftSafe, or Finalized
    #[serde(default)]
    pub propagation: Propagation,  // Local, Syncing, Complete, or Failed
    #[serde(default)]
    pub ack_tracked: bool,         // Whether this fact requests acknowledgments
}
```

- `agreement`: Tracks finalization level (A1/A2/A3 taxonomy)
- `propagation`: Tracks anti-entropy sync status
- `ack_tracked`: Opt-in flag for per-peer acknowledgment tracking

### 11.2 Ack Storage Table

For facts with `ack_tracked = true`, acknowledgments are stored in a separate table:

```
┌─────────────────────────────────────────────────────────────────────────┐
│ Ack Table                                                                │
├─────────────┬─────────────────────────┬─────────────────────────────────┤
│ fact_id     │ peer_id                 │ acked_at                        │
├─────────────┼─────────────────────────┼─────────────────────────────────┤
│ msg-001     │ alice_authority_id      │ 2024-01-15T10:30:00Z            │
│ msg-001     │ bob_authority_id        │ 2024-01-15T10:30:05Z            │
└─────────────┴─────────────────────────┴─────────────────────────────────┘
```

### 11.3 Journal API for Consistency

```rust
impl Journal {
    // Record an acknowledgment from a peer
    pub fn record_ack(&mut self, fact_id: &str, peer: AuthorityId, acked_at: PhysicalTime);

    // Get all acks for a fact
    pub fn get_acks(&self, fact_id: &str) -> Option<&Acknowledgment>;

    // Garbage collect ack tracking based on policy
    pub fn gc_ack_tracking(&mut self, policies: &PolicyRegistry);
}
```

## See Also

- [Database Architecture](113_database.md) - Query system for reading journal facts via Datalog
- [Consistency Metadata](121_consistency_metadata.md) - Agreement, propagation, and acknowledgment tracking
- [State Reduction](120_state_reduction.md) - Reduction pipeline details
- [Maintenance](111_maintenance.md) - Snapshot and garbage collection pipeline
- [Relational Contexts](103_relational_contexts.md) - Context journal structure
