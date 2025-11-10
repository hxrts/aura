# 105 · Journal vs Ledger Architecture

**Purpose**: Canonical explanation of Journal vs Ledger separation in Aura's data layer.

**Key Insight**: Journal and Ledger are **complementary layers** with distinct responsibilities, not competing or interchangeable concepts.

## Architecture Overview

Aura's data layer separates concerns into two distinct but complementary components:

- **Journal** ([`crates/aura-journal/`](../crates/aura-journal/)) - High-level CRDT state management for threshold identity
- **Ledger** ([`crates/aura-protocol/src/effects/ledger.rs`](../crates/aura-protocol/src/effects/ledger.rs)) - Low-level effect interface providing primitive operations

**Relationship**: Journal **uses** Ledger effects to implement high-level operations. Ledger **supports** Journal through primitive capabilities.

## Journal Layer (High-Level CRDT State)

The Journal implements the formal `Journal { facts: Fact, caps: Cap }` semilattice model for distributed threshold identity state management.

### Responsibilities
- **Threshold Identity Management**: Ratchet tree operations, device membership, guardian relationships
- **CRDT State**: Implements join/meet semilattice operations for conflict-free replication  
- **Intent Pool**: OR-set of pending operations for high availability
- **Tree Operations**: Stores threshold-signed `AttestedOp` records for all identity mutations
- **Policy Enforcement**: Manages threshold policies and capability refinement

### Key Files
- **Core types**: [`crates/aura-core/src/journal.rs`](../crates/aura-core/src/journal.rs)
- **CRDT implementation**: [`crates/aura-journal/src/semilattice/journal_map.rs`](../crates/aura-journal/src/semilattice/journal_map.rs) 
- **Ratchet tree**: [`crates/aura-journal/src/ratchet_tree/`](../crates/aura-journal/src/ratchet_tree/)
- **Tree reduction**: [`crates/aura-journal/src/ratchet_tree/reduction.rs`](../crates/aura-journal/src/ratchet_tree/reduction.rs)

### Architecture Pattern
```rust
pub struct Journal {
    pub facts: Fact,    // Join-semilattice: knowledge accumulation
    pub caps: Cap,      // Meet-semilattice: capability refinement  
}

// Journal uses Ledger effects for primitive operations
async fn append_tree_op(op: AttestedOp, ledger: &impl LedgerEffects) -> Result<()> {
    // Verify signatures using ledger crypto primitives
    ledger.verify_threshold_signature(&op.agg_sig).await?;
    // Journal manages the high-level CRDT merge logic
    self.ops.insert(op.op.parent_epoch, op);
    Ok(())
}
```

## Ledger Layer (Low-Level Effect Interface)

The Ledger provides primitive operations that Journal and other high-level components depend on.

### Responsibilities  
- **Device Management**: Device authorization, metadata, activity tracking
- **Crypto Primitives**: Blake3 hashing, secret generation, signature verification
- **Graph Utilities**: Cycle detection, shortest paths, topological sorting  
- **Event Sourcing**: Append events, epoch management, basic persistence
- **Infrastructure Support**: UUID creation, random number generation

### Key Implementation
- **Effect interface**: [`crates/aura-protocol/src/effects/ledger.rs`](../crates/aura-protocol/src/effects/ledger.rs)
- **Handler implementations**: Various concrete handlers in [`crates/aura-protocol/src/handlers/`](../crates/aura-protocol/src/handlers/)

### Architecture Pattern
```rust
#[async_trait]
pub trait LedgerEffects {
    // Event sourcing primitives
    async fn append_event(&self, event: Vec<u8>) -> Result<(), LedgerError>;
    async fn current_epoch(&self) -> Result<u64, LedgerError>;
    
    // Crypto primitives 
    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32];
    async fn verify_signature(&self, sig: &[u8], data: &[u8]) -> bool;
    
    // Device management
    async fn authorize_device(&self, device_id: DeviceId) -> Result<bool, LedgerError>;
    async fn get_device_metadata(&self, device_id: DeviceId) -> Option<DeviceMetadata>;
    
    // Graph utilities
    async fn shortest_path(&self, from: NodeId, to: NodeId) -> Option<Vec<NodeId>>;
}
```

## Integration Pattern

The Journal-Ledger integration follows a clean dependency injection pattern:

```rust
// Journal depends on Ledger effects but doesn't implement them
pub struct JournalMap;

impl JournalMap {
    pub async fn append_tree_op(
        &mut self, 
        op: TreeOpRecord,
        ledger: &impl LedgerEffects,  // Dependency injection
    ) -> Result<(), JournalError> {
        // Use ledger for primitive operations
        let epoch = ledger.current_epoch().await?;
        
        // Journal handles high-level CRDT logic
        let mut new_state = Self::bottom();
        new_state.ops.insert(epoch, op);
        *self = self.join(&new_state);
        
        Ok(())
    }
}
```

## Why This Separation Matters

### Clear Responsibilities
- **Journal**: "What is the current threshold identity state?"
- **Ledger**: "How do I perform primitive operations to support that state?"

### Testability
- **Journal**: Can be tested with mock Ledger implementations
- **Ledger**: Can be tested independently of Journal semantics

### Reusability  
- **Ledger effects**: Used by Journal, storage systems, transport layer, etc.
- **Journal**: Focused solely on threshold identity CRDT semantics

### Evolution
- **Ledger interface**: Stable primitive operations
- **Journal implementation**: Can evolve CRDT algorithms without changing primitives

## Foundation Architecture

### Core Types

The journal implements the formal model `Journal { facts: Fact, caps: Cap }` with strict semilattice properties:

```rust
pub struct Journal {
    pub facts: Fact,    // Join-semilattice: knowledge accumulation
    pub caps: Cap,      // Meet-semilattice: capability refinement
}

pub struct Fact {
    data: BTreeMap<String, JournalValue>,
}

pub struct Cap {
    permissions: BTreeSet<String>,
}
```

### Semilattice Operations

**Join Operation (Facts)**: Knowledge accumulates monotonically.
```rust
impl JoinSemilattice for Fact {
    fn join(&self, other: &Self) -> Self {
        // Merges facts from multiple replicas
        // Conflict resolution via deterministic rules
    }
}
```

**Meet Operation (Capabilities)**: Authority only contracts.
```rust
impl MeetSemilattice for Cap {
    fn meet(&self, other: &Self) -> Self {
        // Intersects capability sets
        // Result is subset of both inputs
    }
}
```

### CRDT Properties

The implementation enforces mathematical laws through property tests:

- **Associative**: `(a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)`
- **Commutative**: `a ⊔ b = b ⊔ a`
- **Idempotent**: `a ⊔ a = a`
- **Monotonic**: Facts grow, capabilities shrink

## Implementation Layers

### Layer 1: Foundation CRDT

**Location**: `aura-core/src/journal.rs`

Complete implementation of the theoretical model. Provides pure CRDT operations without domain-specific logic. All semilattice laws verified through comprehensive property tests.

### Layer 2: Journal Map

**Location**: `aura-journal/src/semilattice/journal_map.rs`

State-based CRDT that extends foundation types with namespace support:

```rust
pub struct JournalMap {
    ops: BTreeMap<u64, TreeOp>,      // epoch -> operation
    intent: BTreeMap<String, Intent>, // intent_id -> intent
}
```

Conflict resolution uses deterministic commitment hash ordering. Intent management follows observed-remove semantics through OR-set implementation.

### Layer 3: Effect Integration

**Location**: `aura-journal/src/ledger/mod.rs`

Algebraic effect pattern for journal operations:

```rust
pub enum LedgerEffect {
    ApplyOperation { op: Operation, actor_id: ActorId },
    MergeRemoteState { remote_state: AccountState },
    QueryHistory { query: HistoryQuery },
}
```

Effect handlers provide async operation processing with proper error propagation and operation logging.

### Layer 4: Graph Structure

**Location**: `aura-journal/src/journal.rs`

Graph-based identity management through KeyJournal:

```rust
pub struct KeyJournal {
    pub nodes: BTreeMap<NodeId, KeyNode>,
    pub edges: BTreeMap<EdgeId, KeyEdge>,
    pub policies: BTreeMap<NodeId, Policy>,
}
```

Supports threshold policies (All, Any, Threshold) with validation. Commitment computation uses content-addressed hashing.

## Reconciliation & Divergence Handling

Although the journal is a CRDT, malicious or faulty replicas can inject invalid operations or diverge temporarily. Aura resolves divergences through:

1. **Evidence Recording**: All attested operations include their `parent_epoch`, `parent_commitment`, and `FROST` signature. Devices reject any op whose parent does not exist locally and emit a `DivergenceEvidence { op_cid, reason }` fact. This provides a forensic trail.
2. **Deterministic Tie-Break**: When multiple children target the same parent, reduction keeps only the operation with the highest `H(op)` hash (as described in `123`). Others remain in the log but are marked superseded, preventing fork persistence.
3. **Poison Remediation**: If a replica ingests poisoned state, it can run `JournalRepair`, which replays operations from a trusted snapshot plus the divergence evidence. This is a local operation using only CRDT facts; no external coordinator is required.
4. **Flow Control**: Ledger effects expose `append_event` only through capability guards. Suspicious replicas can have their append capability revoked by publishing a `CapabilityRevocation` fact signed by the guardian policy.

Together, these mechanisms ensure that even if replicas disagree temporarily, they converge on a single canonical state and provide auditability for any malicious behavior.

## Current Implementation Status

### Complete Components

**CRDT Foundation**: Mathematical properties correctly implemented. Join and meet operations follow semilattice laws. Property tests verify correctness.

**Journal Map**: Functional CRDT with epoch-based operation storage. Proper conflict resolution through commitment hash ordering.

**Basic Graph Operations**: Node and edge creation with policy validation. Graph traversal for reachability analysis.

**Effect System Integration**: Proper algebraic effect pattern with async handlers. Operation logging and idempotency checking.

### Incomplete Components

**Ratchet Tree Integration**: Tree operations defined but application logic is placeholder. Reduction from operation log to tree state not implemented.

**Cryptographic Backend**: Ed25519/Blake3 backend structure exists but not integrated with operations. Threshold signature verification uses placeholder functions.

**Distributed Protocols**: Choreographic coordination utilities are stub implementations. Synchronization protocols not implemented.

### Architecture Decisions

**Two-Phase Synchronization**: Operations stored in intent pool before commitment. Allows coordination without blocking local operations.

**Namespace Separation**: Ops and intents stored separately. Prevents operation interference during coordination phases.

**Deterministic Conflict Resolution**: Uses content-addressed commitment hashes. Ensures all replicas converge to same state.

## Usage Patterns

### Basic Journal Operations

```rust
let mut journal = Journal::new();

// Add facts (accumulative)
let new_facts = Fact::from_kv("device_count", "3");
journal.facts = journal.facts.join(&new_facts);

// Refine capabilities (restrictive)  
let policy = Cap::from_permissions(&["read", "write"]);
journal.caps = journal.caps.meet(&policy);
```

### Effect System Integration

```rust
// Define effect for journal operation
let effect = LedgerEffect::ApplyOperation {
    op: tree_op,
    actor_id: device_id,
};

// Handler processes effect asynchronously
let result = handler.handle_effect(effect).await?;
```

### Graph Operations

```rust
let mut key_journal = KeyJournal::new();

// Add device node
let device_node = KeyNode::device(device_id, public_key);
key_journal.add_node(device_node)?;

// Set threshold policy
let policy = Policy::Threshold { m: 2, n: 3 };
key_journal.set_policy(node_id, policy)?;
```

## Design Constraints

### Monotonicity Requirements

Facts must only grow through join operations. No deletion or rollback operations permitted. Tombstones handle removal semantics while preserving monotonicity.

Capabilities must only shrink through meet operations. Authority cannot be escalated without explicit delegation through proper channels.

### Conflict Resolution

All conflicts resolved deterministically without coordinator. Commitment hash ordering ensures consistent resolution across all replicas.

Multiple concurrent operations handled through intent pool coordination. Prevents race conditions during distributed operation commitment.

### Privacy Preservation

Journal content is context-scoped. Cross-context information flow requires explicit bridge protocols with capability checks.

Operation metadata bounded by leakage budgets. Transport layer enforces privacy constraints through effect system guards.

## Integration Points

### Effect System

Journal operations integrate with unified effect system through `LedgerEffects` trait. Handlers compose with middleware for retry, metrics, and tracing.

### CRDT Synchronization  

The journal implements four core synchronization programs for distributed state management:

**Anti-Entropy Synchronization**: Operation log equalization across replicas using OR-set semantics. Exchange digests and transfer missing operations to achieve convergent operation logs.

**Operation Broadcast**: Immediate dissemination of newly committed operations. Reduces propagation latency after threshold ceremonies complete through eager push protocols.

**Distributed Coordination**: Multi-party coordination for operation attestation through threshold ceremonies. Sessions coordinate partial signature collection and produce single attested operations.

**Snapshot Coordination**: Distributed garbage collection through threshold approval. Implements join-preserving retractions that reduce history while preserving future merge semantics.

### Identity Management

Graph structure tracks device and guardian relationships. Threshold policies enforce M-of-N authorization for sensitive operations.

## Implementation Roadmap

### Current Capabilities

The journal system provides working CRDT operations with proper mathematical properties. Basic graph structure supports device management with threshold policies.

### Near-term Development

Ratchet tree integration requires completing operation application logic. Tree state reduction from operation log needs implementation.

Cryptographic backend integration for threshold signature verification. Share generation and commitment verification through FROST protocol.

Synchronization program implementation for distributed coordination. Anti-entropy protocols, operation broadcast, and snapshot coordination require choreographic protocol completion.

### Future Extensions

Advanced choreographic protocols for multi-party coordination. Session-typed communication patterns with formal verification through MPST algebra.

Enhanced privacy features including differential privacy budgets and unlinkability guarantees. Context isolation with capability-based bridge protocols.

## Cross-References

The journal system integrates with authentication architecture documented in `docs/101_auth_authz.md`. Effect system patterns follow `docs/002_system_architecture.md`. Privacy contracts align with `docs/001_theoretical_foundations.md`.
