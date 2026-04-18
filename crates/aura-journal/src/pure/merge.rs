//! Pure journal merge function - thin wrapper for Aeneas translation
//!
//! This module exposes the actual `Journal::join` implementation as a free function
//! for potential Aeneas translation. The implementation lives in `crate::fact`.
//!
//! # Actual Implementation
//!
//! The real merge logic is in `crate::fact::Journal`'s `JoinSemilattice` impl:
//!
//! ```rust,ignore
//! impl JoinSemilattice for Journal {
//!     fn join(&self, other: &Self) -> Self {
//!         assert_eq!(self.namespace, other.namespace);
//!         let mut merged_facts = self.facts.clone();
//!         merged_facts.extend(other.facts.clone());
//!         Self { namespace: self.namespace.clone(), facts: merged_facts }
//!     }
//! }
//! ```
//!
//! # Semilattice Laws (proven in Lean)
//!
//! The Lean model in `verification/lean/Aura/Journal.lean` proves:
//! - **Commutativity**: `merge j1 j2 ≃ merge j2 j1`
//! - **Associativity**: `merge (merge j1 j2) j3 ≃ merge j1 (merge j2 j3)`
//! - **Idempotence**: `merge j j ≃ j`
//!
//! # Aeneas Notes
//!
//! For Aeneas translation, the main challenges are:
//! - `BTreeSet` usage (may need simplification to `Vec` with dedup)
//! - `serde` derives on types
//! - Complex `FactContent` enum variants
//!
//! The differential testing in `crates/aura-testkit/tests/lean_differential.rs`
//! validates that Rust and Lean produce equivalent results.

use crate::fact::Journal;
use aura_core::semilattice::JoinSemilattice;

/// Pure journal merge function.
///
/// This is a thin wrapper around `Journal::join()` exposed as a free function
/// for easier Aeneas targeting.
///
/// # Precondition
///
/// Both journals must have the same namespace. This is checked at runtime
/// via assert. In Lean, this is modeled as a precondition.
///
/// # Semilattice Properties
///
/// This operation satisfies:
/// - Commutativity: `journal_join(a, b) ≃ journal_join(b, a)`
/// - Associativity: `journal_join(journal_join(a, b), c) ≃ journal_join(a, journal_join(b, c))`
/// - Idempotence: `journal_join(a, a) ≃ a`
///
/// Where `≃` denotes set-membership equivalence (same facts, possibly different order).
#[inline]
pub fn journal_join(j1: &Journal, j2: &Journal) -> Journal {
    j1.join(j2)
}

/// In-place journal merge.
///
/// More efficient version that modifies `j1` in place by consuming `j2`.
#[inline]
pub fn journal_join_assign(j1: &mut Journal, j2: Journal) {
    j1.join_assign(j2);
}
