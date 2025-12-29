//! Domain-specific CRDT implementations using foundation traits
//!
//! This module provides journal-specific CRDT types built on the
//! harmonized foundation from `aura-core`.

use crate::effect_api::intent::{Intent, IntentId};
use aura_core::semilattice::{Bottom, CvState, JoinSemilattice};
// Note: OpLog is defined in op_log.rs with full sync helpers (OpLogSummary, diff, etc.)
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Intent pool CRDT with observed-remove semantics
///
/// Manages a pool of pending intents where additions win over removals,
/// providing eventual consistency for intent staging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentPool {
    /// Active intents
    pub intents: BTreeMap<IntentId, Intent>,
    /// Retracted intent IDs (observed-remove set)
    pub retractions: BTreeSet<IntentId>,
}

impl IntentPool {
    /// Create a new empty intent pool
    pub fn new() -> Self {
        Self {
            intents: BTreeMap::new(),
            retractions: BTreeSet::new(),
        }
    }

    /// Add an intent to the pool
    pub fn add_intent(&mut self, intent: Intent) {
        let id = intent.intent_id;

        // Only add if not retracted (observed-remove: add wins)
        if !self.retractions.contains(&id) {
            self.intents.insert(id, intent);
        }
    }

    /// Remove an intent from the pool
    pub fn remove_intent(&mut self, id: IntentId) {
        self.retractions.insert(id);
        self.intents.remove(&id);
    }

    /// Check if an intent is present
    pub fn contains(&self, id: &IntentId) -> bool {
        self.intents.contains_key(id)
    }

    /// Get an intent by ID
    pub fn get(&self, id: &IntentId) -> Option<&Intent> {
        self.intents.get(id)
    }

    /// List all active intents
    pub fn list_intents(&self) -> Vec<&Intent> {
        self.intents.values().collect()
    }

    /// Get number of active intents
    pub fn len(&self) -> usize {
        self.intents.len()
    }

    /// Check if pool is empty
    pub fn is_empty(&self) -> bool {
        self.intents.is_empty()
    }
}

impl JoinSemilattice for IntentPool {
    fn join(&self, other: &Self) -> Self {
        let mut result = self.clone();

        // Merge intents (add wins over remove)
        for (id, intent) in &other.intents {
            if !result.retractions.contains(id) {
                result.intents.insert(*id, intent.clone());
            }
        }

        // Merge retractions (union)
        for id in &other.retractions {
            result.retractions.insert(*id);
            result.intents.remove(id); // Remove if present
        }

        result
    }
}

impl Bottom for IntentPool {
    fn bottom() -> Self {
        Self::new()
    }
}

impl CvState for IntentPool {}

impl Default for IntentPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Epoch-ordered operation log CRDT
///
/// Maintains a grow-only log of operations ordered by epoch,
/// with deterministic conflict resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpochLog<T> {
    /// Operations by epoch
    pub ops: BTreeMap<u64, T>,
}

impl<T: Clone> EpochLog<T> {
    /// Create a new empty log
    pub fn new() -> Self {
        Self {
            ops: BTreeMap::new(),
        }
    }

    /// Add an operation to the epoch log
    pub fn add_operation(&mut self, epoch: u64, op: T) {
        self.ops.insert(epoch, op);
    }

    /// Append an operation at the given epoch
    pub fn append(&mut self, epoch: u64, op: T) {
        self.ops.insert(epoch, op);
    }

    /// Get operation at epoch
    pub fn get(&self, epoch: u64) -> Option<&T> {
        self.ops.get(&epoch)
    }

    /// Get all operations in epoch order
    pub fn ops_ordered(&self) -> Vec<&T> {
        self.ops.values().collect()
    }

    /// Get latest epoch
    pub fn latest_epoch(&self) -> Option<u64> {
        self.ops.keys().max().copied()
    }

    /// Get number of operations
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Check if log is empty
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

impl<T: Clone + Ord> JoinSemilattice for EpochLog<T> {
    fn join(&self, other: &Self) -> Self {
        let mut result = self.clone();

        // Merge operations (for conflicts, keep the greater one by Ord)
        for (epoch, op) in &other.ops {
            if let Some(existing) = result.ops.get(epoch) {
                if op > existing {
                    result.ops.insert(*epoch, op.clone());
                }
            } else {
                result.ops.insert(*epoch, op.clone());
            }
        }

        result
    }
}

impl<T: Clone> Bottom for EpochLog<T> {
    fn bottom() -> Self {
        Self::new()
    }
}

impl<T: Clone + Ord> CvState for EpochLog<T> {}

impl<T: Clone> Default for EpochLog<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epoch_log_conflict_resolution() {
        let mut log1 = EpochLog::<String>::new();
        let mut log2 = EpochLog::<String>::new();

        // Same epoch, different values
        log1.append(1, "value_a".to_string());
        log2.append(1, "value_b".to_string());

        let joined = log1.join(&log2);

        // Higher value should win (lexicographic ordering)
        assert_eq!(joined.get(1), Some(&"value_b".to_string()));
    }

    #[test]
    fn test_crdt_laws() {
        let pool1 = IntentPool::new();
        let pool2 = IntentPool::new();
        let pool3 = IntentPool::new();

        // Commutativity: a ⊔ b = b ⊔ a
        assert_eq!(pool1.join(&pool2), pool2.join(&pool1));

        // Associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
        assert_eq!(
            pool1.join(&pool2).join(&pool3),
            pool1.join(&pool2.join(&pool3))
        );

        // Idempotence: a ⊔ a = a
        assert_eq!(pool1.join(&pool1), pool1);

        // Identity: a ⊔ ⊥ = a
        assert_eq!(pool1.join(&IntentPool::bottom()), pool1);
    }
}
