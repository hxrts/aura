//! Core CRDT semantic interfaces for Aura workspace
//!
//! This module provides the foundational traits that define CRDT semantics
//! for use throughout the Aura workspace. Following docs/402_crdt_types.md,
//! these traits are used to type message payloads in session types and
//! enforce CRDT laws through effect interpreters.

/// State-based CRDT (CvRDT) join semilattice
///
/// A join semilattice has a binary operation (join) that is:
/// - Commutative: a ⊔ b = b ⊔ a
/// - Associative: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
/// - Idempotent: a ⊔ a = a
pub trait JoinSemilattice: Clone {
    /// Join operation that produces the least upper bound
    fn join(&self, other: &Self) -> Self;
}

/// Bottom element for join semilattices
pub trait Bottom {
    /// Return the bottom element (minimum value)
    fn bottom() -> Self;
}

/// Complete state-based CRDT interface
///
/// Convergent Replicated Data Types (CvRDTs) synchronize by exchanging
/// full state and merging using the join semilattice operation.
pub trait CvState: JoinSemilattice + Bottom {}

// === Meet Semi-Lattice Foundation ===

/// Meet semi-lattice with greatest lower bound operation
///
/// Algebraic laws that all implementations must satisfy:
/// - Commutativity: a ∧ b = b ∧ a
/// - Associativity: (a ∧ b) ∧ c = a ∧ (b ∧ c)
/// - Idempotence: a ∧ a = a
pub trait MeetSemiLattice: Clone {
    /// Meet operation (greatest lower bound)
    ///
    /// The result satisfies: (a ∧ b) ≤ a and (a ∧ b) ≤ b
    /// This enables constraint satisfaction and capability restriction.
    fn meet(&self, other: &Self) -> Self;
}

/// Top element for meet semi-lattices (most permissive state)
///
/// The top element ⊤ serves as the identity for meet operations:
/// a ∧ ⊤ = a for all a
pub trait Top {
    /// Return the top element (⊤)
    fn top() -> Self;
}

/// Meet-based CRDT state using MeetSemiLattice
///
/// This enables constraint-based CRDTs where operations restrict rather
/// than accumulate state. Useful for capability sets, security policies,
/// and consensus constraints.
///
/// Note: Not all meet semilattices have a top element. Types that do have
/// a top element should also implement the `Top` trait separately.
pub trait MvState: MeetSemiLattice + serde::Serialize + PartialEq {}

/// Delta CRDT for incremental synchronization
///
/// Delta CRDTs optimize bandwidth by transmitting incremental updates
/// rather than full states. Deltas are joined and folded into state.
pub trait Delta: Clone {
    /// Join deltas together
    fn join_delta(&self, other: &Self) -> Self;
}

/// Produce deltas from state transitions
pub trait DeltaProduce<S> {
    /// Compute delta between old and new states
    fn delta_from(old: &S, new: &S) -> Self;
}

/// Causal operation with identity and context
///
/// Operations in CmRDTs carry causal context (vector clocks, etc.)
/// to enable causal delivery and commutativity guarantees.
pub trait CausalOp {
    /// Operation identifier type (for deduplication)
    type Id: Clone;
    /// Causal context type (vector clock, dependency set, etc.)
    type Ctx: Clone;

    /// Get the unique identifier for this operation
    fn id(&self) -> Self::Id;
    /// Get the causal context for this operation
    fn ctx(&self) -> &Self::Ctx;
}

/// Apply operations with commutativity under causal delivery
///
/// Commutative Replicated Data Types (CmRDTs) apply operations that
/// commute when delivered in causal order.
pub trait CmApply<Op> {
    /// Apply an operation to the state
    fn apply(&mut self, op: Op);
}

/// Deduplication for operation-based CRDTs
///
/// Tracks which operations have been seen to prevent duplicate application.
pub trait Dedup<I> {
    /// Check if an operation has been seen
    fn seen(&self, id: &I) -> bool;
    /// Mark an operation as seen
    fn mark_seen(&mut self, id: I);
}

// === Join-Meet Duality ===

/// Convert join semi-lattice to its meet dual
///
/// Enables systematic conversion between accumulative (join) and
/// restrictive (meet) semantics in dual algebraic structures.
pub trait JoinToDual<T> {
    /// Convert this join semi-lattice value to its meet dual
    fn to_dual(&self) -> T;
}

/// Convert meet semi-lattice to its join dual
///
/// Provides the reverse conversion from restrictive to accumulative
/// semantics, completing the duality relationship.
pub trait MeetToDual<T> {
    /// Convert this meet semi-lattice value to its join dual
    fn to_dual(&self) -> T;
}
