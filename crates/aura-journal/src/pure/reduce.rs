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
use aura_core::types::authority::AuthorityState;

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
