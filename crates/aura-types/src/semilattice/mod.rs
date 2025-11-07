//! CRDT foundation layer for Aura workspace
//!
//! This module provides the foundational traits and message types for
//! implementing CRDTs with session types and effect interpreters.

pub use message_types::*;
pub use semantic_traits::*;

pub mod message_types;
pub mod semantic_traits;

use std::collections::BTreeMap;

// Foundational trait implementations for common types

impl JoinSemilattice for u64 {
    fn join(&self, other: &Self) -> Self {
        (*self).max(*other)
    }
}

impl Bottom for u64 {
    fn bottom() -> Self {
        0
    }
}

impl CvState for u64 {}

impl<T: Clone + Ord> JoinSemilattice for Vec<T> {
    fn join(&self, other: &Self) -> Self {
        let mut result = self.clone();
        for item in other {
            if !result.contains(item) {
                result.push(item.clone());
            }
        }
        result.sort();
        result
    }
}

impl<T: Clone + Ord> Bottom for Vec<T> {
    fn bottom() -> Self {
        Vec::new()
    }
}

impl<T: Clone + Ord> CvState for Vec<T> {}

impl<K: Clone + Ord, V: Clone + JoinSemilattice> JoinSemilattice for BTreeMap<K, V> {
    fn join(&self, other: &Self) -> Self {
        let mut result = self.clone();
        for (key, value) in other {
            match result.get(key) {
                Some(existing) => {
                    result.insert(key.clone(), existing.join(value));
                }
                None => {
                    result.insert(key.clone(), value.clone());
                }
            }
        }
        result
    }
}

impl<K: Clone + Ord, V: Clone + JoinSemilattice + Bottom> Bottom for BTreeMap<K, V> {
    fn bottom() -> Self {
        BTreeMap::new()
    }
}

impl<K: Clone + Ord, V: Clone + JoinSemilattice + Bottom> CvState for BTreeMap<K, V> {}

// === Meet Semi-Lattice Foundation Implementations ===

use std::collections::BTreeSet;

impl MeetSemiLattice for u64 {
    fn meet(&self, other: &Self) -> Self {
        (*self).min(*other)
    }
}

impl Top for u64 {
    fn top() -> Self {
        u64::MAX
    }
}

impl MvState for u64 {}

impl<T: Clone + Ord> MeetSemiLattice for BTreeSet<T> {
    fn meet(&self, other: &Self) -> Self {
        // Intersection of sets (more restrictive)
        self.intersection(other).cloned().collect()
    }
}

// Note: BTreeSet<T> does NOT implement Top because there is no universal set
// containing all possible T values. A meet semilattice does not require a top element.

impl<T: Clone + Ord + serde::Serialize + PartialEq> MvState for BTreeSet<T> {}

impl<K: Clone + Ord, V: Clone + MeetSemiLattice> MeetSemiLattice for BTreeMap<K, V> {
    fn meet(&self, other: &Self) -> Self {
        let mut result = BTreeMap::new();

        // Only include keys present in both maps, with meet of values
        for (key, value) in self {
            if let Some(other_value) = other.get(key) {
                result.insert(key.clone(), value.meet(other_value));
            }
        }

        result
    }
}

impl<K: Clone + Ord, V: Clone + MeetSemiLattice + Top> Top for BTreeMap<K, V> {
    fn top() -> Self {
        // Empty map is top for intersection-based semantics
        BTreeMap::new()
    }
}

impl<
        K: Clone + Ord + serde::Serialize + PartialEq,
        V: Clone + MeetSemiLattice + Top + serde::Serialize + PartialEq,
    > MvState for BTreeMap<K, V>
{
}
