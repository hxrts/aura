//! Concrete CRDT implementations aligned with docs/001_theoretical_foundations.md

use aura_core::semilattice::{Bottom, CvState, JoinSemilattice};
use std::collections::{BTreeMap, HashSet};

/// CRDT operation errors
#[derive(Debug, thiserror::Error)]
pub enum CrdtError {
    /// CRDT operation failed with description
    #[error("CRDT operation failed: {0}")]
    OperationFailed(String),
    /// Serialization of CRDT state failed
    #[error("Serialization failed: {0}")]
    SerializationFailed(String),
    /// Deserialization of CRDT state failed
    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),
    /// Merge conflict detected during CRDT operation
    #[error("Merge conflict: {0}")]
    MergeConflict(String),
    /// Invalid CRDT state
    #[error("Invalid state: {0}")]
    InvalidState(String),
    /// Unsupported CRDT backend implementation
    #[error("Unsupported backend: {0}")]
    UnsupportedBackend(String),
    /// CRDT synchronization failed
    #[error("Sync failed: {0}")]
    SyncFailed(String),
}

// =============================================================================
// GCounter (Grow-only Counter) - CvRDT
// =============================================================================

/// Replica identifier
pub type Replica = String;

/// Grow-only counter (G-Counter)
///
/// A CvRDT that can only increase. Each actor maintains its own counter value,
/// and the total value is the sum of all actor counters. Merging keeps the
/// maximum value for each actor.
///
/// See docs/001_theoretical_foundations.md Section 4
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GCounter(pub BTreeMap<Replica, i64>);

impl GCounter {
    /// Create a new empty grow-only counter
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Increment the counter for a specific actor by the given amount
    pub fn increment(&mut self, actor_id: Replica, amount: i64) {
        let current = self.0.get(&actor_id).unwrap_or(&0);
        self.0.insert(actor_id, current + amount);
    }

    /// Get the total value of the counter (sum of all actor counters)
    pub fn value(&self) -> i64 {
        self.0.values().sum()
    }
}

impl Default for GCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl JoinSemilattice for GCounter {
    fn join(&self, other: &Self) -> Self {
        let mut result = self.0.clone();
        for (actor_id, other_count) in &other.0 {
            let self_count = result.get(actor_id).unwrap_or(&0);
            result.insert(actor_id.clone(), (*self_count).max(*other_count));
        }
        Self(result)
    }
}

impl Bottom for GCounter {
    fn bottom() -> Self {
        Self(BTreeMap::new())
    }
}

impl CvState for GCounter {}

// =============================================================================
// GSet (Grow-only Set) - CvRDT
// =============================================================================

/// Grow-only set (G-Set)
///
/// A CvRDT that can only add elements, never remove them. Merging combines
/// the elements from both sets. Only supports elements that implement Clone,
/// Eq, and Hash traits.
///
/// Grow-only set CRDT. Elements can only be added, never removed.
#[derive(Debug, Clone)]
pub struct GSet<T: Clone + Eq + std::hash::Hash>(pub HashSet<T>);

impl<T: Clone + Eq + std::hash::Hash> GSet<T> {
    /// Create a new empty grow-only set
    pub fn new() -> Self {
        Self(HashSet::new())
    }

    /// Add an element to the set
    pub fn add(&mut self, element: T) {
        self.0.insert(element);
    }

    /// Check if an element is in the set
    pub fn contains(&self, element: &T) -> bool {
        self.0.contains(element)
    }

    /// Get an iterator over all elements in the set
    pub fn elements(&self) -> impl Iterator<Item = &T> {
        self.0.iter()
    }
}

impl<T: Clone + Eq + std::hash::Hash> Default for GSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + Eq + std::hash::Hash> JoinSemilattice for GSet<T> {
    fn join(&self, other: &Self) -> Self {
        let mut result = self.0.clone();
        result.extend(other.0.iter().cloned());
        Self(result)
    }
}

impl<T: Clone + Eq + std::hash::Hash> Bottom for GSet<T> {
    fn bottom() -> Self {
        Self(HashSet::new())
    }
}

impl<T: Clone + Eq + std::hash::Hash> CvState for GSet<T> {}

// =============================================================================
// LwwRegister (Last-Writer-Wins Register) - CvRDT
// =============================================================================

/// Last-Writer-Wins (LWW) register
///
/// A CvRDT that resolves concurrent writes by keeping the value
/// from the write with the highest timestamp. If timestamps are equal,
/// the actor ID is used as a tiebreaker.
///
/// See docs/001_theoretical_foundations.md for LWW semantics
#[derive(Debug, Clone)]
pub struct LwwRegister<T: Clone> {
    value: Option<T>,
    timestamp: u64,
    actor_id: Replica,
}

impl<T: Clone> LwwRegister<T> {
    /// Create a new empty LWW register
    pub fn new() -> Self {
        Self {
            value: None,
            timestamp: 0,
            actor_id: String::new(),
        }
    }

    /// Set the value in the register with metadata
    pub fn set(&mut self, value: T, timestamp: u64, actor_id: Replica) {
        if timestamp > self.timestamp || (timestamp == self.timestamp && actor_id > self.actor_id) {
            self.value = Some(value);
            self.timestamp = timestamp;
            self.actor_id = actor_id;
        }
    }

    /// Get the current value from the register
    pub fn get(&self) -> Option<&T> {
        self.value.as_ref()
    }
}

impl<T: Clone> Default for LwwRegister<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> JoinSemilattice for LwwRegister<T> {
    fn join(&self, other: &Self) -> Self {
        if other.timestamp > self.timestamp
            || (other.timestamp == self.timestamp && other.actor_id > self.actor_id)
        {
            other.clone()
        } else {
            self.clone()
        }
    }
}

impl<T: Clone> Bottom for LwwRegister<T> {
    fn bottom() -> Self {
        Self::new()
    }
}

impl<T: Clone> CvState for LwwRegister<T> {}
