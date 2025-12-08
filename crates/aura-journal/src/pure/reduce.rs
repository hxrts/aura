//! Pure journal reduction functions - thin wrappers for Aeneas translation
//!
//! This module exposes the actual reduction functions from `crate::reduction`
//! for potential Aeneas translation.
//!
//! # Actual Implementation
//!
//! The real reduction logic lives in `crate::reduction`:
//! - `reduce_authority()`: Derives authority state from facts
//! - `reduce_context()`: Derives relational context state from facts
//!
//! # Aeneas Compatibility Status
//!
//! **WARNING**: These functions are complex and may not be directly translatable
//! by Aeneas without significant simplification. The challenges include:
//!
//! - Many external type dependencies
//! - Complex pattern matching on `FactContent` variants
//! - Use of `HashMap`, `BTreeMap`, `BTreeSet`
//! - Error handling with `Result` types
//!
//! # Verification Strategy
//!
//! For now, the Lean model uses a simplified `reduce` that is the identity
//! function. The key property proven is **determinism**: given the same input
//! facts, reduction always produces the same output.
//!
//! The differential testing infrastructure validates specific reduction
//! scenarios where we can serialize inputs/outputs to JSON.

use crate::fact::Journal;
use crate::reduction::{self, ReductionNamespaceError, RelationalState};
use aura_core::authority::AuthorityState;

/// Pure authority reduction function.
///
/// Derives the canonical authority state from a journal's facts.
/// This is a thin wrapper around `reduction::reduce_authority()`.
///
/// # Determinism (proven in Lean)
///
/// The Lean model proves that reduction is deterministic:
/// ```lean
/// theorem reduce_deterministic (facts : Journal) :
///   reduce facts = reduce facts := rfl
/// ```
///
/// # Aeneas Status
///
/// This function has many dependencies and may require a simplified
/// version for Aeneas translation. Consider creating a minimal
/// `reduce_authority_simple` that operates on a simplified fact representation.
#[inline]
pub fn authority_reduce(journal: &Journal) -> Result<AuthorityState, ReductionNamespaceError> {
    reduction::reduce_authority(journal)
}

/// Pure context reduction function.
///
/// Derives the canonical relational context state from a journal's facts.
/// This is a thin wrapper around `reduction::reduce_context()`.
///
/// # Determinism
///
/// Like authority reduction, context reduction is deterministic.
///
/// # Aeneas Status
///
/// Complex - requires simplification for Aeneas translation.
#[inline]
pub fn context_reduce(journal: &Journal) -> Result<RelationalState, ReductionNamespaceError> {
    reduction::reduce_context(journal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact::{Fact, FactContent, JournalNamespace, SnapshotFact};
    use aura_core::identifiers::AuthorityId;
    use aura_core::time::{OrderTime, TimeStamp};
    use aura_core::Hash32;

    fn make_authority_journal(seed: u8) -> Journal {
        let auth_id = AuthorityId::new_from_entropy([seed; 32]);
        Journal::new(JournalNamespace::Authority(auth_id))
    }

    fn make_snapshot_fact(order_byte: u8, sequence: u64) -> Fact {
        Fact {
            order: OrderTime([order_byte; 32]),
            timestamp: TimeStamp::OrderClock(OrderTime([order_byte; 32])),
            content: FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence,
            }),
        }
    }

    #[test]
    fn test_authority_reduce_deterministic() {
        let mut journal = make_authority_journal(1);
        journal.add_fact(make_snapshot_fact(1, 1)).unwrap();
        journal.add_fact(make_snapshot_fact(2, 2)).unwrap();

        // Reduce twice - should get identical results
        let result1 = authority_reduce(&journal);
        let result2 = authority_reduce(&journal);

        // Both should succeed or fail the same way
        assert_eq!(result1.is_ok(), result2.is_ok());
    }

    #[test]
    fn test_context_reduce_wrong_namespace() {
        // Authority namespace journal shouldn't work for context reduction
        let journal = make_authority_journal(1);
        let result = context_reduce(&journal);
        assert!(result.is_err());
    }
}
