//! Algebraic law and property tests.
//!
//! Verify the mathematical invariants that CRDT and semilattice
//! implementations must satisfy for correctness and convergence.

#[path = "laws/semilattice_join.rs"]
mod semilattice_join;

#[path = "laws/semilattice_meet.rs"]
mod semilattice_meet;

#[path = "laws/flow_budget_crdt.rs"]
mod flow_budget_crdt;

#[path = "laws/tree_policy_meet.rs"]
mod tree_policy_meet;

#[path = "laws/time_ordering.rs"]
mod time_ordering;
