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
