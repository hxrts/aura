# hello-crdt

Minimal CRDT example that demonstrates the journal discipline: meet-guarded preconditions and join-only commits.

What it shows
- A tiny CvRDT type with a join operation
- An MPST session that computes a result and commits it as a journal fact
- No negative facts; retries are idempotent and commute

Sketch
```rust
// A tiny join-CRDT
#[derive(Clone, PartialEq, Eq)]
struct MaxU64(u64);
impl JoinSemilattice for MaxU64 {
    fn join(&self, other: &Self) -> Self { MaxU64(self.0.max(other.0)) }
}

// Session computes a value; commit is a single join
async fn compute_and_commit<J: JournalEffects>(j: &J, v: u64) -> Result<()> {
    let facts = j.read_facts().await;
    let cur = facts.get::<MaxU64>("counter").unwrap_or(MaxU64(0));
    let next = cur.join(&MaxU64(v));
    j.merge_facts(facts.put("counter", next)).await;
    Ok(())
}
```

Run
- Integrate this pattern into any test to validate join-only commits and idempotence.

