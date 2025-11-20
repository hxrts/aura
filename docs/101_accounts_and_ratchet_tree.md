# Accounts and Ratchet Tree

This document specifies the internal state machine of an account authority. It defines the ratchet tree structure, available operations, reduction model, epoch semantics, and security properties. It describes when Aura Consensus is required and how the account state interacts with deterministic key derivation.

## 1. Account State Machine

An account authority maintains its internal state through a ratchet tree and an account journal. The ratchet tree defines device membership and threshold policies. The journal stores facts that represent signed tree operations. The reduction function reconstructs the canonical tree state from the accumulated fact set.

An account authority exposes a single public key. This public key is derived from the ratchet tree root. The authority never exposes device structure. The account state changes only when an attested operation appears in the journal.

```rust
pub struct TreeState {
    pub epoch: Epoch,
    pub root_commitment: Hash32,
}
```

This structure represents the reduced state of an account. The `epoch` and `root_commitment` are derived from the fact set. External parties reference only these values.

## 2. Ratchet Tree Structure

A ratchet tree contains branch nodes and leaf nodes. A leaf node represents a device inside the account. A branch node represents a subpolicy. Each node has an index and a commitment. The root node defines the account-level threshold policy.

```rust
pub enum NodeKind {
    Leaf(LeafNode),
    Branch(BranchNode),
}
```

This type defines leaf and branch variants. Leaf nodes store device information required for threshold signing. Branch nodes store policy data. Each node contributes to the total commitment.

The ratchet tree is ordered by node index. Children of a branch are ordered consistently. The ordering appears in the commitment calculation. This ensures identical structure across replicas.

## 3. Policies

A branch node contains a threshold policy. A policy describes the number of required signatures for authorization. Aura defines three policy forms.

```rust
pub enum Policy {
    Any,
    Threshold { m: u16, n: u16 },
    All,
}
```

This enum expresses the allowed policies. The `Any` policy accepts one signature from any device under that branch. The `Threshold` policy requires `m` signatures out of `n` devices. The `All` policy requires all devices under the branch. Policies form a meet semilattice. The meet selects the stricter of two policies.

## 4. Tree Operations

Tree operations modify the ratchet tree. Each operation references a parent epoch and parent commitment. Each operation is signed through threshold signing.

```rust
pub enum TreeOpKind {
    AddLeaf { leaf: LeafNode, under: NodeIndex },
    RemoveLeaf { leaf: LeafId, reason: u8 },
    ChangePolicy { node: NodeIndex, new_policy: Policy },
    RotateEpoch { affected: Vec<NodeIndex> },
}
```

These four operations modify device membership, branch policy, or epoch. The `AddLeaf` operation inserts a new leaf. The `RemoveLeaf` operation removes an existing leaf. The `ChangePolicy` operation updates the policy of a branch. The `RotateEpoch` operation increments the epoch for a set of nodes. Epoch rotation invalidates derived context keys.

Each operation appears in the journal as an attested operation.

```rust
pub struct AttestedOp {
    pub op: TreeOp,
    pub agg_sig: Vec<u8>,
}
```

The `agg_sig` field stores the threshold signature produced by devices. The signature validates under the parent root commitment. Devices refuse to sign if the local tree state does not match.

## 5. Semilattice Model

The account journal is a join semilattice. It stores `AttestedOp` facts. All replicas merge fact sets using set union. The ratchet tree state is recovered using deterministic reduction.

Reduction applies the following rules.

1. Identify operations that reference the same parent state.
2. Select a single winner using a deterministic ordering:
   * Sort contenders by `(parent_commitment, op_hash)` where `op_hash = H(op_bytes || agg_sig)`.
   * If hashes match, break ties with the lexicographic ordering of `(agg_sig, op_bytes)`.
3. Discard superseded operations.
4. Apply winners in parent order.

This process yields a single tree state for any given fact set. All replicas with the same facts compute the same result.

## 6. Conflict Resolution

Conflicts arise when multiple operations reference the same parent epoch and commitment. The reduction algorithm resolves these conflicts using a total order on operations. The order is based on a stable hash. The winning operation applies. The losing operations are ignored for state calculation.

Conflict resolution ensures convergence. It also ensures that replicas remain consistent under concurrent updates.

### 6.1 Pseudocode

```rust
fn reduce_account(facts: &[AttestedOp]) -> TreeState {
    // Group ops by parent commitment + epoch
    let mut buckets: BTreeMap<ParentState, Vec<&AttestedOp>> = BTreeMap::new();
    for op in facts {
        let key = ParentState {
            commitment: op.op.parent_commitment,
            epoch: op.op.parent_epoch,
        };
        buckets.entry(key).or_default().push(op);
    }

    // Deterministic winner selection
    let mut winners: Vec<&AttestedOp> = Vec::new();
    for (_, ops) in buckets {
        let winner = ops.into_iter().max_by_key(|op| {
            let op_hash = hash(op);
            (op.op.parent_commitment, op_hash, op.agg_sig.clone(), op.op.clone())
        }).expect("non-empty bucket");
        winners.push(winner);
    }

    // Apply in parent order
    winners.sort_by_key(|op| op.op.parent_epoch);
    let mut state = TreeState::default();
    for op in winners {
        state = state.apply(op);
    }
    state
}
```

The implementation must follow this ordering so every replica reaches the same root commitment.

## 7. Epochs

The epoch is an integer stored in the tree state. The epoch scopes deterministic key derivation. Derived keys depend on the current epoch. Rotation invalidates previous derived keys. The `RotateEpoch` operation updates the epoch for selected subtrees.

Epochs also scope flow budgets and context presence tickets. All context identities must refresh when the epoch changes.

## 8. Derived Context Keys

Derived context keys bind relationship data to the account state. The deterministic key derivation function uses the ratchet tree root commitment and epoch. This ensures that all devices compute the same context keys.

Derived keys do not modify the tree state. They depend solely on reduced account state.

## 9. Interaction with Consensus

Consensus is used when a tree operation must have strong agreement across a committee. Consensus produces a commit fact containing a threshold signature. This fact becomes an attested operation in the journal.

Consensus is used when multiple devices must agree on the same prestate. Simple device initiated changes may use local threshold signing. The account journal treats both cases identically.

Consensus references the root commitment and epoch of the account. This binds the commit fact to the current state.

## 10. Security Properties

The ratchet tree provides fork resistance. Devices refuse to sign under mismatched parent commitments. The reduction function ensures that all replicas converge. Structural opacity hides device membership from external parties.

The threshold signature scheme prevents unauthorized updates. All operations must be signed by the required number of devices. An attacker cannot forge signatures or bypass policies.

The tree design ensures that no external party can identify device structure. The only visible values are the epoch and the root commitment.

## 11. Implementation Architecture

The ratchet tree implementation lives in `aura-journal/src/ratchet_tree/` with a 10-file architecture:

### Core Architecture Diagram

```text
OpLog (CRDT OR-set of AttestedOp) ─────┐
                                        │
                                        ↓ reduce()
                                   TreeState
                                   (derived, materialized on-demand)
                                   - epoch: u64
                                   - root_commitment: Hash32
                                   - nodes: BTreeMap<NodeIndex, Node>
                                   - leaves: BTreeMap<LeafId, LeafNode>
```

### File Organization

| File | Purpose | Key Types/Functions |
|------|---------|---------------------|
| `mod.rs` | Module definition and re-exports | Public API surface |
| `tree_types.rs` | Type definitions (re-exported from aura-core during transition) | Core tree types |
| `local_types.rs` | Authority-internal device types | `DeviceId`, internal-only types |
| `attested_ops.rs` | Fact-to-operation conversion | `AttestedOp` ↔ fact conversion |
| `state.rs` | Tree state representation | `TreeState`, node storage, commitment |
| `authority_state.rs` | Authority-internal state | Device membership tracking |
| `operations.rs` | Operation processing pipeline | `TreeOperationProcessor`, `BatchProcessor`, `TreeStateQuery` |
| `reduction.rs` | Deterministic reduction algorithm | `reduce()`, conflict resolution |
| `application.rs` | Operation application and verification | `apply_verified()`, `validate_invariants()` |
| `compaction.rs` | Garbage collection and state cleanup | `compact()`, snapshot optimization |

### Critical Invariants

The implementation enforces these fundamental rules:

1. **TreeState is NEVER stored in the journal** - It is always derived on-demand via reduction
2. **OpLog is the ONLY persisted tree data** - All tree state can be recovered from the operation log
3. **Reduction is DETERMINISTIC across all replicas** - Same OpLog always produces same TreeState
4. **DeviceId is authority-internal only** - Never exposed in public APIs (see `local_types.rs`)

### Data Flow

```text
1. Tree Operation Initiated
   ↓
2. operations.rs: TreeOperationProcessor validates and processes
   ↓
3. attested_ops.rs: Convert to AttestedOp fact
   ↓
4. Journal stores fact (CRDT OR-set)
   ↓
5. reduction.rs: reduce() processes all facts
   ↓
6. application.rs: apply_verified() builds TreeState
   ↓
7. state.rs: TreeState materialized
   ↓
8. authority_state.rs: Internal device view updated
```

### Reduction Algorithm (reduction.rs)

The reduction function implements the deterministic conflict resolution described in §6:

```rust
fn reduce(facts: &[AttestedOp]) -> TreeState {
    // 1. Group ops by parent commitment + epoch
    // 2. Deterministic winner selection via H(op) ordering
    // 3. Apply winners in topological (parent) order
    // 4. Return materialized TreeState
}
```

### Operation Processing (operations.rs)

Three key abstractions for operation handling:

- **TreeOperationProcessor**: Validates and processes individual operations
- **BatchProcessor**: Efficient bulk operation processing
- **TreeStateQuery**: Query interface for derived tree state

### Compaction (compaction.rs)

Periodic garbage collection removes superseded operations:

```rust
fn compact(op_log: &[AttestedOp]) -> Vec<AttestedOp> {
    // 1. Reduce to current TreeState
    // 2. Identify superseded operations
    // 3. Return minimal op set that produces same state
    // 4. Verify join-preserving property
}
```

### Integration Points

**From Other Modules:**
- `aura-core`: Core tree types during transition (will be moved to `aura-journal`)
- `aura-relational`: Consensus integration for attested operations
- `aura-protocol`: Guard chain enforcement for tree modifications

**Exports To:**
- `aura-agent`: Tree state queries for device management
- `aura-cli`: Authority inspection and debugging
- `aura-testkit`: Test fixtures and builders

### Migration Notes

The implementation is transitioning from the legacy graph-based approach:

- **Old**: `KeyNode`/`KeyEdge` graph structures stored directly in journal
- **New**: `AttestedOp` facts with deterministic reduction to `TreeState`
- **Status**: New implementation complete; old code removed as of Phase 8

### Security Considerations

1. **Structural Opacity**: DeviceId types in `local_types.rs` are never exposed externally
2. **Fork Resistance**: Devices refuse to sign operations with mismatched parent commitments
3. **Threshold Enforcement**: All operations require valid threshold signatures in `AttestedOp.agg_sig`
4. **Deterministic Convergence**: Conflict resolution guarantees eventual consistency

### See Also

- [Journal Architecture](102_journal.md) - Fact-based journal system
- [Relational Contexts](103_relational_contexts.md) - Cross-authority coordination
- [Consensus](104_consensus.md) - Aura Consensus for strong agreement
