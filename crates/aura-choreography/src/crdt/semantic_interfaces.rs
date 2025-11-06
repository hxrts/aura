//! CRDT semantic interfaces for algebraic effects
//!
//! Following docs/402_crdt_types.md, we define CRDT laws via traits that handlers enforce.
//! These are orthogonal to session typing and used to type message payloads.

/// State-based CRDT (CvRDT) - join semilattice
pub trait JoinSemilattice: Clone {
    /// Join operation that forms a semilattice
    /// Must be: commutative, associative, idempotent
    fn join(&self, other: &Self) -> Self;
}

/// Bottom element for state-based CRDTs
pub trait Bottom {
    /// Return the bottom element of the semilattice
    fn bottom() -> Self;
}

/// Complete state-based CRDT interface
pub trait CvState: JoinSemilattice + Bottom {}

/// Delta CRDT interface for efficient gossip
pub trait Delta: Clone {
    /// Join delta states together
    fn join_delta(&self, other: &Self) -> Self;
}

/// Produce deltas from state transitions
pub trait DeltaProduce<S> {
    /// Compute delta between old and new states
    fn delta_from(old: &S, new: &S) -> Self;
}

/// Causal operation with identity and context
pub trait CausalOp {
    /// Operation identifier type
    type Id: Clone;
    /// Causal context type (vector clock, etc.)
    type Ctx: Clone;
    
    /// Get the unique identifier for this operation
    fn id(&self) -> Self::Id;
    /// Get the causal context for this operation
    fn ctx(&self) -> &Self::Ctx;
}

/// Apply operations with commutativity under causal delivery
pub trait CmApply<Op> {
    /// Apply an operation to the state
    fn apply(&mut self, op: Op);
}

/// Deduplication for operation-based CRDTs
pub trait Dedup<I> {
    /// Check if operation has been seen before
    fn seen(&self, id: &I) -> bool;
    /// Mark operation as seen
    fn mark_seen(&mut self, id: I);
}