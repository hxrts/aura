# Journal CRDT Invariants

## CRDT Convergence Invariant

### Invariant Name
`CRDT_CONVERGENCE`

### Description
Identical sets of facts must always produce identical reduced state, regardless of the order in which facts arrive or are processed. This ensures eventual consistency across all replicas.

### Enforcement Locus

1. **Fact Validation**:
   - Module: `aura-journal/src/reduction.rs`
   - Function: `reduce_authority_with_validation()` - Validates attested ops against tree state before reduction
   - Module: `aura-journal/src/commitment_tree/application.rs`
   - Function: `validate_invariants()` - Enforces commitment tree structural invariants

2. **Deterministic Reduction**:
   - Module: `aura-journal/src/reduction.rs`
   - Function: `reduce_authority()` / `reduce_account_facts()` - Authority state reduction
   - Key property: Pure function over ordered facts (OrderTime)

3. **Relational Reduction**:
   - Module: `aura-journal/src/reduction.rs`
   - Function: `reduce_context()` - Context state reduction
   - Key property: Commutative and associative operations

4. **Join Semilattice**:
   - Module: `aura-journal/src/semilattice/mod.rs`
   - Trait: `JoinSemilattice` implementation for facts
   - Property: `join(a, b) = join(b, a)` (commutative)
   - Property: `join(join(a, b), c) = join(a, join(b, c))` (associative)
   - Property: `join(a, a) = a` (idempotent)

### Failure Mode

**Observable Consequences**:
1. **State Divergence**: Same facts produce different states on different nodes
2. **Consensus Failure**: Nodes cannot agree on canonical state
3. **Authority Corruption**: Account state becomes inconsistent

**Failure Scenarios**:
- Non-deterministic reduction function (e.g., using HashMap iteration)
- Time-dependent reduction logic
- Floating point arithmetic in reduction
- External state dependencies

### Detection Method

1. **Property Tests**:
   ```rust
   #[test]
   fn test_reduction_determinism() {
       let facts = generate_facts();
       // Test all permutations produce same result
       for permutation in facts.permutations() {
           assert_eq!(reduce(facts), reduce(permutation));
       }
   }
   ```

2. **Simulator Scenarios**:
   - Test: `test_convergence_under_partition()`
   - Scenario: Partition network, apply facts in different orders
   - Expected: All nodes converge to same state when partition heals

3. **Reduction Invariants**:
   - No floating point operations
   - No system time access during reduction
   - No HashMap iteration without sorting (prefer BTreeMap/BTreeSet)
   - Pure functions only (no side effects)
   - Use `OrderTime` for deterministic ordering

### Related Invariants
- `FACT_IMMUTABILITY`: Facts never change after creation
- `MONOTONE_GROWTH`: State only grows, never retracts
- `SNAPSHOT_DETERMINISM`: Snapshots are deterministic at fact boundaries

### Implementation Notes

Reduction must be a pure function of facts:

```rust
// CORRECT: Deterministic reduction
fn reduce_facts(facts: &[Fact]) -> State {
    // Sort facts by deterministic order (OrderTime + hash tie-break)
    let sorted = facts.iter()
        .sorted_by_key(|f| (f.order, f.hash()))
        .collect();
    
    // Reduce in deterministic order
    sorted.fold(State::default(), |state, fact| {
        match fact {
            Fact::Relational(op) => state.apply_relational(op),
            Fact::AttestedOp(op) => state.apply_attested(op),
            _ => state
        }
    })
}

// WRONG: Non-deterministic
fn bad_reduce(facts: &[Fact]) -> State {
    let mut map = HashMap::new();
    // HashMap iteration order is non-deterministic!
    for (k, v) in map.iter() { ... }
}
```

### Verification

Run convergence tests:
```bash
cargo test -p aura-journal convergence
cargo test -p aura-simulator crdt_convergence_scenario
```

## Authority Tree Topology + Commitment Invariant

### Invariant Name
`AUTHORITY_TREE_TOPOLOGY_COMMITMENT_COHERENCE`

### Description
For authority-internal tree state (`AuthorityTreeState`), topology and commitment caches must remain coherent after every mutation:

1. Every active leaf has exactly one parent branch.
2. Every non-root branch has exactly one parent branch.
3. Every branch has exactly two ordered children `(left, right)`.
4. Parent and child pointers are bidirectionally consistent.
5. Branch commitments are recomputed bottom-up from the same deterministic topology.
6. `root_commitment` must match deterministic finalization of the current root branch commitment.

### Enforcement Locus

1. **Topology materialization**:
   - Module: `aura-journal/src/commitment_tree/authority_state.rs`
   - Function: `rebuild_topology_from_active_leaves()`
   - Deterministic shape:
     - leaves sorted by `LeafId`
     - stable pair composition per level
     - root is always `NodeIndex(0)`
     - remaining branches assigned breadth-first

2. **Incremental commitment propagation**:
   - Function: `mark_dirty_from_leaf()`
   - Function: `collect_dirty_paths_to_root()`
   - Function: `flush_dirty_commitments_bottom_up()`
   - Function: `recompute_branch_commitment()`

3. **Invariant checking**:
   - Function: `assert_topology_invariants()`
   - Public wrapper: `validate_topology_invariants()`
   - Debug assertions run after mutation entry points (`add_device`, `remove_device`, `update_*`, `rotate_epoch`)

4. **Proof/cache coherence**:
   - Function: `update_merkle_proof_paths()`
   - Proof paths are derived from the materialized topology, not from ad-hoc structure reconstruction.

### Failure Mode

**Observable Consequences**:
1. Invalid or stale parent/child pointers.
2. Commitment mismatch across replicas for identical fact sets.
3. Invalid Merkle proofs after mutation.
4. Non-deterministic branch indexing under replay.

### Detection Method

1. `cargo test -p aura-journal --test authority_tree_correctness`
2. Differential checks: incremental root vs `recompute_root_commitment_full()`
3. Topology invariant checks after random mutation sequences (property tests)

### Implementation Notes

- Determinism depends on stable ordering and on avoiding nondeterministic map iteration.
- Structural changes currently use deterministic topology rebuild (correctness-first).
- Non-structural updates (leaf key / policy / epoch) recompute only affected branch-to-root paths.
