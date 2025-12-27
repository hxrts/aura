//! B-tree index for efficient fact lookups.
//!
//! Provides O(log n) lookups by predicate, authority, and timestamp.

use super::time_key::TimeKey;
use aura_core::{
    domain::journal::FactValue,
    effects::indexed::{FactId, IndexedFact},
    effects::IndexStats,
    time::TimeStamp,
    types::identifiers::AuthorityId,
};
use std::collections::{BTreeMap, BTreeSet};

/// Internal structure for managing B-tree indexes
#[derive(Debug)]
pub(crate) struct AuthorityIndex {
    /// B-tree index: predicate -> set of fact IDs
    pub(crate) by_predicate: BTreeMap<String, BTreeSet<FactId>>,
    /// B-tree index: authority -> set of fact IDs
    pub(crate) by_authority: BTreeMap<AuthorityId, BTreeSet<FactId>>,
    /// B-tree index: timestamp -> set of fact IDs (for range queries)
    pub(crate) by_timestamp: BTreeMap<TimeKey, BTreeSet<FactId>>,
    /// All indexed facts (id -> fact)
    pub(crate) facts: BTreeMap<FactId, IndexedFact>,
    /// Next fact ID to assign
    pub(crate) next_id: u64,
}

// Manual impl to avoid derive macro - struct has complex state initialization
impl Default for AuthorityIndex {
    #[allow(clippy::derivable_impls)]
    fn default() -> Self {
        Self {
            by_predicate: BTreeMap::new(),
            by_authority: BTreeMap::new(),
            by_timestamp: BTreeMap::new(),
            facts: BTreeMap::new(),
            next_id: 0,
        }
    }
}

impl AuthorityIndex {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Insert a new fact into all indexes
    pub(crate) fn insert(
        &mut self,
        predicate: String,
        value: FactValue,
        authority: Option<AuthorityId>,
        timestamp: Option<TimeStamp>,
    ) -> FactId {
        let id = FactId::new(self.next_id);
        self.next_id += 1;

        // Clone timestamp for the index before moving into fact
        let ts_for_index = timestamp.clone();

        let fact = IndexedFact {
            id,
            predicate: predicate.clone(),
            value,
            authority,
            timestamp,
        };

        // Insert into facts map
        self.facts.insert(id, fact);

        // Update predicate index
        self.by_predicate.entry(predicate).or_default().insert(id);

        // Update authority index
        if let Some(auth) = authority {
            self.by_authority.entry(auth).or_default().insert(id);
        }

        // Update timestamp index
        if let Some(ts) = ts_for_index {
            let key = TimeKey::from_timestamp(ts);
            self.by_timestamp.entry(key).or_default().insert(id);
        }

        id
    }

    /// Get facts by predicate
    pub(crate) fn get_by_predicate(&self, predicate: &str) -> Vec<IndexedFact> {
        self.by_predicate
            .get(predicate)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.facts.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get facts by authority
    pub(crate) fn get_by_authority(&self, authority: &AuthorityId) -> Vec<IndexedFact> {
        self.by_authority
            .get(authority)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.facts.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get facts in timestamp range (inclusive)
    pub(crate) fn get_in_range(&self, start: &TimeStamp, end: &TimeStamp) -> Vec<IndexedFact> {
        let start_key = TimeKey::from_timestamp(start.clone());
        let end_key = TimeKey::from_timestamp(end.clone());
        self.by_timestamp
            .range(start_key..=end_key)
            .flat_map(|(_, ids)| ids.iter())
            .filter_map(|id| self.facts.get(id).cloned())
            .collect()
    }

    /// Get statistics about the index
    pub(crate) fn stats(&self) -> IndexStats {
        IndexStats {
            fact_count: self.facts.len() as u64,
            predicate_count: self.by_predicate.len() as u64,
            authority_count: self.by_authority.len() as u64,
            bloom_fp_rate: 0.0, // Will be updated by handler
            merkle_depth: self.compute_merkle_depth(),
        }
    }

    /// Compute the depth of a balanced Merkle tree for the current fact count
    fn compute_merkle_depth(&self) -> u32 {
        let count = self.facts.len() as u32;
        if count == 0 {
            0
        } else {
            (count as f64).log2().ceil() as u32
        }
    }
}
