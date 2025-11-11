# hello-crdt

Minimal CRDT example that demonstrates CvRDT (Convergent Replicated Data Type) usage and the journal discipline: meet-guarded preconditions and join-only commits.

## What it shows

- A Counter CRDT implementing `JoinSemilattice` trait from `aura-core`
- Per-replica counter synchronization across multiple replicas
- Join semilattice operation (merge) that computes least upper bound
- CRDT properties verification: idempotency, commutativity, associativity
- Eventual consistency guarantee: all replicas converge to same state
- No negative facts; all operations are monotonically increasing
- Retries are idempotent and commute (safe to re-execute)

## Implementation

The example creates a Counter CRDT that:
1. Tracks increments per replica in a `BTreeMap<String, u64>`
2. Increments are local to each replica (no network needed for local ops)
3. Synchronization happens via `join()` operation on full state
4. Join takes the maximum count for each replica
5. Demonstrates that merge is commutative and associative

## Key CRDT Laws Verified

- **Idempotency**: `a ⊔ a = a` (merging with self doesn't change state)
- **Commutativity**: `a ⊔ b = b ⊔ a` (merge order doesn't matter)
- **Associativity**: `(a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)` (grouping doesn't matter)

## Code Structure

```rust
// Counter CRDT with per-replica tracking
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counter {
    counts: BTreeMap<String, u64>,
}

impl JoinSemilattice for Counter {
    fn join(&self, other: &Counter) -> Counter {
        // Takes max for each replica - least upper bound
    }
}

impl Bottom for Counter {
    fn bottom() -> Self {
        // Empty counter - identity element for join
    }
}

impl CvState for Counter {}
```

## Run

```bash
cargo run -p hello-crdt
```

Or from the workspace root:

```bash
./target/debug/hello-crdt
```

The example demonstrates:
1. Creating two independent replicas
2. Performing offline operations (local increments)
3. Synchronizing via merge (join operation)
4. Verifying eventual consistency
5. Testing CRDT algebraic properties
6. Following the journal discipline of join-only commits
