//! Production indexed journal handler.
//!
//! Provides efficient O(log n) lookups using B-tree indexes,
//! O(1) membership testing using Bloom filters, and
//! cryptographic verification of integrity using Merkle trees.

use super::authority_index::AuthorityIndex;
use super::merkle::build_merkle_tree;
use super::stream::TokioFactStreamReceiver;
use async_trait::async_trait;
use aura_core::{
    domain::journal::FactValue,
    effects::indexed::{FactId, FactStreamReceiver, IndexedFact},
    effects::{BloomConfig, BloomFilter, IndexStats, IndexedJournalEffects},
    time::TimeStamp,
    types::identifiers::AuthorityId,
    AuraError,
};
use parking_lot::RwLock;
use std::collections::HashSet;

/// Production indexed journal handler
///
/// Provides efficient O(log n) lookups using B-tree indexes,
/// O(1) membership testing using Bloom filters, and
/// cryptographic verification or integrity using Merkle trees.
///
/// # Performance Guarantees
///
/// - `facts_by_predicate`: O(log n + k) where k is result size
/// - `facts_by_authority`: O(log n + k) where k is result size
/// - `facts_in_range`: O(log n + k) where k is result size
/// - `might_contain`: O(1) with <1% false positive rate
/// - `merkle_root`: O(n) on first call, O(1) cached
/// - `verify_fact_inclusion`: O(log n)
///
/// # Concurrency Model
///
/// Uses `parking_lot::RwLock` for synchronous locking of the state bundle.
/// Each operation acquires and releases the lock in sub-millisecond time.
/// See module documentation for scale expectations and alternatives.
pub struct IndexedJournalHandler {
    /// Database handler state (index, bloom, merkle cache)
    state: RwLock<DatabaseState>,
    /// Broadcast channel for streaming fact updates to subscribers
    pub(crate) fact_updates: tokio::sync::broadcast::Sender<Vec<IndexedFact>>,
}

#[derive(Debug)]
struct DatabaseState {
    index: AuthorityIndex,
    bloom_filter: BloomFilter,
    merkle_root_cache: Option<[u8; 32]>,
    fact_hashes: HashSet<[u8; 32]>,
}

impl DatabaseState {
    #[allow(dead_code)] // For use with with_state_mut_validated
    fn validate(&self) -> Result<(), String> {
        if self.bloom_filter.element_count < self.fact_hashes.len() as u64 {
            return Err(format!(
                "bloom filter count {} below fact hash count {}",
                self.bloom_filter.element_count,
                self.fact_hashes.len()
            ));
        }
        Ok(())
    }
}

impl IndexedJournalHandler {
    /// Create a new indexed journal handler with default configuration
    pub fn new() -> Self {
        Self::with_capacity(10000)
    }

    /// Create a new indexed journal handler with specified expected capacity
    pub fn with_capacity(expected_elements: u64) -> Self {
        let bloom_config = BloomConfig::optimal(expected_elements, 0.01);
        let bloom_filter = BloomFilter::new(bloom_config).expect("Failed to create bloom filter");

        // Create broadcast channel for fact streaming (capacity: 100 batches)
        let (fact_updates_tx, _) = tokio::sync::broadcast::channel(100);

        Self {
            state: RwLock::new(DatabaseState {
                index: AuthorityIndex::new(),
                bloom_filter,
                merkle_root_cache: None,
                fact_hashes: HashSet::new(),
            }),
            fact_updates: fact_updates_tx,
        }
    }

    fn with_state<R>(&self, op: impl FnOnce(&DatabaseState) -> R) -> R {
        let guard = self.state.read();
        op(&guard)
    }

    fn with_state_mut<R>(&self, op: impl FnOnce(&mut DatabaseState) -> R) -> R {
        let mut guard = self.state.write();
        let result = op(&mut guard);
        #[cfg(debug_assertions)]
        {
            if let Err(message) = guard.validate() {
                tracing::error!(%message, "IndexedJournalHandler state invariant violated");
                debug_assert!(
                    false,
                    "IndexedJournalHandler invariant violated: {}",
                    message
                );
            }
        }
        result
    }

    /// Add a fact to the index
    pub fn add_fact(
        &self,
        predicate: String,
        value: FactValue,
        authority: Option<AuthorityId>,
        timestamp: Option<TimeStamp>,
    ) -> FactId {
        let element = self.fact_to_bytes(&predicate, &value);
        let fact_hash = aura_core::hash::hash(&element);

        let (id, fact_to_broadcast) = self.with_state_mut(|state| {
            // Insert into B-tree indexes
            let id = state
                .index
                .insert(predicate.clone(), value.clone(), authority, timestamp);

            // Insert into Bloom filter
            // Direct bloom filter insertion (synchronous for lock-based access)
            for i in 0..state.bloom_filter.config.num_hash_functions {
                let i = i as u64;
                let hash_bytes = {
                    let mut hasher = aura_core::hash::hasher();
                    hasher.update(&i.to_le_bytes());
                    hasher.update(&element);
                    hasher.finalize().to_vec()
                };
                let mut hash_u64_bytes = [0u8; 8];
                hash_u64_bytes.copy_from_slice(&hash_bytes[..8]);
                let hash_value = u64::from_le_bytes(hash_u64_bytes);

                let bit_index = hash_value % state.bloom_filter.config.bit_vector_size;
                let byte_index = (bit_index / 8) as usize;
                let bit_offset = (bit_index % 8) as u8;

                if byte_index < state.bloom_filter.bits.len() {
                    state.bloom_filter.bits[byte_index] |= 1u8 << bit_offset;
                }
            }
            state.bloom_filter.element_count += 1;

            // Add to Merkle tree hashes
            state.fact_hashes.insert(fact_hash);

            // Invalidate Merkle cache
            state.merkle_root_cache = None;

            // Retrieve the fact we just added for broadcasting.
            let fact = state.index.facts.get(&id).cloned();
            (id, fact)
        });

        if let Some(fact) = fact_to_broadcast {
            // Send as a batch of one fact
            // Ignore send errors (happens when there are no subscribers)
            let _ = self.fact_updates.send(vec![fact]);
        };

        id
    }

    /// Convert a fact to bytes for hashing
    pub(crate) fn fact_to_bytes(&self, predicate: &str, value: &FactValue) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(predicate.as_bytes());
        bytes.push(0); // separator
        match value {
            FactValue::String(s) => {
                bytes.push(0);
                bytes.extend_from_slice(s.as_bytes());
            }
            FactValue::Number(n) => {
                bytes.push(1);
                bytes.extend_from_slice(&n.to_le_bytes());
            }
            FactValue::Bytes(b) => {
                bytes.push(2);
                bytes.extend_from_slice(b);
            }
            FactValue::Set(s) => {
                bytes.push(3);
                for item in s {
                    bytes.extend_from_slice(item.as_bytes());
                    bytes.push(0);
                }
            }
            FactValue::Nested(nested_fact) => {
                bytes.push(4);
                // Hash the nested fact for a stable representation
                // Fact derives Serialize so we can use canonical serialization
                if let Ok(serialized) = aura_core::util::serialization::to_vec(nested_fact.as_ref())
                {
                    let hash = aura_core::hash::hash(&serialized);
                    bytes.extend_from_slice(&hash);
                }
            }
        }
        bytes
    }

    /// Check if a fact might be contained (using Bloom filter)
    pub(crate) fn bloom_check(&self, predicate: &str, value: &FactValue) -> bool {
        let element = self.fact_to_bytes(predicate, value);
        self.with_state(|state| {
            let filter = &state.bloom_filter;
            for i in 0..filter.config.num_hash_functions {
                let i = i as u64;
                let hash_bytes = {
                    let mut hasher = aura_core::hash::hasher();
                    hasher.update(&i.to_le_bytes());
                    hasher.update(&element);
                    hasher.finalize().to_vec()
                };
                let mut hash_u64_bytes = [0u8; 8];
                hash_u64_bytes.copy_from_slice(&hash_bytes[..8]);
                let hash_value = u64::from_le_bytes(hash_u64_bytes);

                let bit_index = hash_value % filter.config.bit_vector_size;
                let byte_index = (bit_index / 8) as usize;
                let bit_offset = (bit_index % 8) as u8;

                if byte_index >= filter.bits.len()
                    || (filter.bits[byte_index] & (1u8 << bit_offset)) == 0
                {
                    return false;
                }
            }
            true
        })
    }

    /// Compute or retrieve cached Merkle root
    pub(crate) fn compute_merkle_root(&self) -> [u8; 32] {
        self.with_state_mut(|state| {
            if let Some(root) = state.merkle_root_cache {
                return root;
            }

            let hashes: Vec<[u8; 32]> = state.fact_hashes.iter().copied().collect();
            let root = if let Some(tree) = build_merkle_tree(hashes) {
                tree.hash
            } else {
                [0u8; 32]
            };

            state.merkle_root_cache = Some(root);
            root
        })
    }
}

impl Default for IndexedJournalHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for IndexedJournalHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (fact_count, predicate_count, authority_count) = self.with_state(|state| {
            (
                state.index.facts.len(),
                state.index.by_predicate.len(),
                state.index.by_authority.len(),
            )
        });
        f.debug_struct("IndexedJournalHandler")
            .field("fact_count", &fact_count)
            .field("predicate_count", &predicate_count)
            .field("authority_count", &authority_count)
            .finish()
    }
}

#[async_trait]
impl IndexedJournalEffects for IndexedJournalHandler {
    fn watch_facts(&self) -> Box<dyn FactStreamReceiver> {
        Box::new(TokioFactStreamReceiver::new(self.fact_updates.subscribe()))
    }

    async fn facts_by_predicate(&self, predicate: &str) -> Result<Vec<IndexedFact>, AuraError> {
        Ok(self.with_state(|state| state.index.get_by_predicate(predicate)))
    }

    async fn facts_by_authority(
        &self,
        authority: &AuthorityId,
    ) -> Result<Vec<IndexedFact>, AuraError> {
        Ok(self.with_state(|state| state.index.get_by_authority(authority)))
    }

    async fn facts_in_range(
        &self,
        start: TimeStamp,
        end: TimeStamp,
    ) -> Result<Vec<IndexedFact>, AuraError> {
        Ok(self.with_state(|state| state.index.get_in_range(&start, &end)))
    }

    async fn all_facts(&self) -> Result<Vec<IndexedFact>, AuraError> {
        Ok(self.with_state(|state| state.index.facts.values().cloned().collect()))
    }

    fn might_contain(&self, predicate: &str, value: &FactValue) -> bool {
        self.bloom_check(predicate, value)
    }

    async fn merkle_root(&self) -> Result<[u8; 32], AuraError> {
        Ok(self.compute_merkle_root())
    }

    async fn verify_fact_inclusion(&self, fact: &IndexedFact) -> Result<bool, AuraError> {
        let element = self.fact_to_bytes(&fact.predicate, &fact.value);
        let fact_hash = aura_core::hash::hash(&element);
        Ok(self.with_state(|state| state.fact_hashes.contains(&fact_hash)))
    }

    async fn get_bloom_filter(&self) -> Result<BloomFilter, AuraError> {
        Ok(self.with_state(|state| state.bloom_filter.clone()))
    }

    async fn index_stats(&self) -> Result<IndexStats, AuraError> {
        let (stats, filter) =
            self.with_state(|state| (state.index.stats(), state.bloom_filter.clone()));
        let mut stats = stats;
        // Estimate false positive rate based on filter fill ratio
        // Note: set_bits could be used for more accurate FP estimation in the future
        let _set_bits: u64 = filter.bits.iter().map(|b| b.count_ones() as u64).sum();
        let m = filter.config.bit_vector_size as f64;
        let k = filter.config.num_hash_functions as f64;
        let n = filter.element_count as f64;

        if n > 0.0 {
            // FP rate ≈ (1 - e^(-kn/m))^k
            let fill_ratio: f64 = (-k * n / m).exp();
            stats.bloom_fp_rate = (1.0_f64 - fill_ratio).powf(k);
        }

        Ok(stats)
    }
}
