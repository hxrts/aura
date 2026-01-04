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
/// More efficient version that modifies `j1` in place.
#[inline]
pub fn journal_join_assign(j1: &mut Journal, j2: &Journal) {
    j1.join_assign(j2);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact::{Fact, FactContent, JournalNamespace, SnapshotFact};
    use aura_core::identifiers::AuthorityId;
    use aura_core::time::{OrderTime, TimeStamp};
    use aura_core::Hash32;

    fn make_journal(authority_seed: u8) -> Journal {
        let auth_id = AuthorityId::new_from_entropy([authority_seed; 32]);
        Journal::new(JournalNamespace::Authority(auth_id))
    }

    fn make_fact(order_byte: u8) -> Fact {
        Fact::new(
            OrderTime([order_byte; 32]),
            TimeStamp::OrderClock(OrderTime([order_byte; 32])),
            FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: order_byte as u64,
            }),
        )
    }

    #[test]
    fn test_journal_join_wrapper() {
        let mut j1 = make_journal(1);
        let mut j2 = make_journal(1); // Same namespace

        j1.add_fact(make_fact(1)).unwrap();
        j1.add_fact(make_fact(2)).unwrap();
        j2.add_fact(make_fact(3)).unwrap();

        let merged = journal_join(&j1, &j2);
        assert_eq!(merged.size(), 3);
    }

    #[test]
    fn test_journal_join_commutative() {
        let mut j1 = make_journal(1);
        let mut j2 = make_journal(1);

        j1.add_fact(make_fact(1)).unwrap();
        j2.add_fact(make_fact(2)).unwrap();

        let m12 = journal_join(&j1, &j2);
        let m21 = journal_join(&j2, &j1);

        // Same facts (set equivalence)
        assert_eq!(m12.size(), m21.size());
        assert_eq!(m12.facts, m21.facts);
    }

    #[test]
    fn test_journal_join_idempotent() {
        let mut j = make_journal(1);
        j.add_fact(make_fact(1)).unwrap();
        j.add_fact(make_fact(2)).unwrap();

        let merged = journal_join(&j, &j);
        assert_eq!(j.facts, merged.facts);
    }
}
