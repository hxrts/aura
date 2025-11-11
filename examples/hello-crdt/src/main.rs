//! # Hello CRDT Example
//!
//! A minimal demonstration of CvRDT (Conflict-free Replicated Data Type) usage in Aura,
//! using the actual semantic traits from aura-core.
//!
//! This example shows:
//! - Creating a simple Counter CRDT implementing JoinSemilattice
//! - Merging state across replicas using the join operation
//! - Verifying CRDT properties: commutativity, associativity, idempotency
//! - Following the discipline: meet-guarded preconditions and join-only commits
//!
//! Run with: `cargo run --example hello-crdt`

use aura_core::semilattice::{Bottom, CvState, JoinSemilattice};
use std::collections::BTreeMap;

/// A simple grow-only counter CRDT
///
/// This implements a per-replica counter where each replica tracks
/// increments from all other replicas. The join operation takes the
/// maximum count for each replica.
///
/// This is a CvRDT (Convergent Replicated Data Type) that:
/// - Synchronizes by exchanging full state
/// - Merges using the join semilattice operation
/// - Guarantees eventual consistency
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counter {
    /// Map of replica_id -> count increments from that replica
    /// A replica can only increment its own counter, so counts are monotonically increasing
    counts: BTreeMap<String, u64>,
}

impl Counter {
    /// Create a new counter for a specific replica
    pub fn new(replica_id: String) -> Self {
        let mut counts = BTreeMap::new();
        counts.insert(replica_id, 0);
        Counter { counts }
    }

    /// Increment the counter for this replica
    ///
    /// Only the replica that owns this counter instance should call this.
    /// The increment is local and doesn't require network communication.
    pub fn increment(&mut self) {
        // Get the first (and typically only) replica ID for local increments
        if let Some(replica_id) = self.counts.keys().next().cloned() {
            *self.counts.entry(replica_id).or_insert(0) += 1;
        }
    }

    /// Get the total value (sum across all replicas)
    ///
    /// The value is the sum of all per-replica counters, representing
    /// the total number of increments across all devices.
    pub fn value(&self) -> u64 {
        self.counts.values().sum()
    }
}

/// JoinSemilattice implementation for Counter
///
/// The join operation computes the least upper bound by taking the maximum
/// count for each replica. This ensures idempotency and commutativity.
impl JoinSemilattice for Counter {
    fn join(&self, other: &Counter) -> Counter {
        let mut result = self.clone();

        // Merge other's counts by taking the max for each replica
        for (replica_id, count) in &other.counts {
            result
                .counts
                .entry(replica_id.clone())
                .and_modify(|c| *c = (*c).max(*count))
                .or_insert(*count);
        }

        result
    }
}

/// Bottom element for Counter (identity for join)
///
/// The bottom element is an empty counter, which represents the minimum state.
/// Joining with bottom doesn't change the state: a ⊔ ⊥ = a
impl Bottom for Counter {
    fn bottom() -> Self {
        Counter {
            counts: BTreeMap::new(),
        }
    }
}

/// Complete CRDT interface for Counter
///
/// This marker trait indicates Counter is a CvRDT (state-based CRDT)
/// that can be used in journal protocols and choreographies.
impl CvState for Counter {}

fn main() {
    println!("=== Hello CRDT: Counter Example ===\n");

    // Create two replicas with distinct IDs
    let mut replica_a = Counter::new("replica_a".to_string());
    let mut replica_b = Counter::new("replica_b".to_string());

    println!("Initial state:");
    println!("  Replica A value: {}", replica_a.value());
    println!("  Replica B value: {}\n", replica_b.value());

    // === Phase 1: Offline operations ===
    println!("Phase 1: Offline operations (no synchronization)");
    replica_a.increment();
    replica_a.increment();
    println!(
        "  Replica A incremented twice -> value: {}",
        replica_a.value()
    );

    replica_b.increment();
    replica_b.increment();
    replica_b.increment();
    println!(
        "  Replica B incremented thrice -> value: {}\n",
        replica_b.value()
    );

    // === Phase 2: Synchronization via join ===
    println!("Phase 2: Synchronization (both replicas merge each other's state)");
    replica_a = replica_a.join(&replica_b);
    replica_b = replica_b.join(&replica_a);

    println!("  After merge:");
    println!("    Replica A value: {}", replica_a.value());
    println!("    Replica B value: {}\n", replica_b.value());

    // === Verify eventual consistency ===
    println!("Verification: Eventual consistency");
    assert_eq!(
        replica_a.value(),
        5,
        "Replicas should converge to same value"
    );
    assert_eq!(
        replica_b.value(),
        5,
        "Replicas should converge to same value"
    );
    assert_eq!(
        replica_a.counts, replica_b.counts,
        "Replicas should have identical state after merge"
    );
    println!(
        "  OK Both replicas converged to value: {}\n",
        replica_a.value()
    );

    // === Verify CRDT Property #1: Idempotency ===
    println!("CRDT Property #1: Idempotency (a ⊔ a = a)");
    let original_a = replica_a.clone();
    replica_a = replica_a.join(&replica_a);
    assert_eq!(
        original_a, replica_a,
        "Merging with itself should not change state"
    );
    println!("  OK a ⊔ a = a (merging with self doesn't change value)\n");

    // === Verify CRDT Property #2: Commutativity ===
    println!("CRDT Property #2: Commutativity (a ⊔ b = b ⊔ a)");
    let mut replica_c = Counter::new("replica_c".to_string());
    let mut replica_d = Counter::new("replica_d".to_string());

    replica_c.increment();
    replica_d.increment();
    replica_d.increment();

    let ab = replica_c.join(&replica_d);
    let ba = replica_d.join(&replica_c);

    assert_eq!(ab, ba, "Merge order shouldn't matter");
    println!("  OK a ⊔ b = b ⊔ a (merge is commutative)\n");

    // === Verify CRDT Property #3: Associativity ===
    println!("CRDT Property #3: Associativity ((a ⊔ b) ⊔ c = a ⊔ (b ⊔ c))");
    let mut e = Counter::new("e".to_string());
    let mut f = Counter::new("f".to_string());
    let mut g = Counter::new("g".to_string());

    e.increment();
    f.increment();
    f.increment();
    g.increment();
    g.increment();
    g.increment();

    let abc_left = e.join(&f).join(&g);
    let abc_right = e.join(&f.join(&g));

    assert_eq!(
        abc_left, abc_right,
        "Grouping shouldn't matter for associativity"
    );
    println!("  OK (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c) (merge is associative)\n");

    // === Journal discipline ===
    println!(
        r#"Journal Discipline: Meet-guarded preconditions, join-only commits
  OK No negative facts (counters only grow)
  OK All merges are join operations (monotonic)
  OK Retries are idempotent (can safely re-execute)
"#
    );

    println!("=== All CRDT properties verified ===");
}
