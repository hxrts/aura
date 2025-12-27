//! Production indexed journal handler.
//!
//! Provides efficient O(log n) lookups using B-tree indexes,
//! O(1) membership testing using Bloom filters, and
//! cryptographic integrity verification using Merkle trees.

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
/// cryptographic integrity verification using Merkle trees.
///
/// # Performance Guarantees
///
/// - `facts_by_predicate`: O(log n + k) where k is result size
/// - `facts_by_authority`: O(log n + k) where k is result size
/// - `facts_in_range`: O(log n + k) where k is result size
/// - `might_contain`: O(1) with <1% false positive rate
/// - `merkle_root`: O(n) on first call, O(1) cached
/// - `verify_fact_inclusion`: O(log n)
pub struct IndexedJournalHandler {
    /// B-tree indexes protected by RwLock for concurrent access
    pub(crate) index: RwLock<AuthorityIndex>,
    /// Bloom filter for fast membership checks
    pub(crate) bloom_filter: RwLock<BloomFilter>,
    /// Cached Merkle root (invalidated on mutations)
    pub(crate) merkle_root_cache: RwLock<Option<[u8; 32]>>,
    /// Set of fact hashes in the Merkle tree
    pub(crate) fact_hashes: RwLock<HashSet<[u8; 32]>>,
    /// Broadcast channel for streaming fact updates to subscribers
    pub(crate) fact_updates: tokio::sync::broadcast::Sender<Vec<IndexedFact>>,
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
            index: RwLock::new(AuthorityIndex::new()),
            bloom_filter: RwLock::new(bloom_filter),
            merkle_root_cache: RwLock::new(None),
            fact_hashes: RwLock::new(HashSet::new()),
            fact_updates: fact_updates_tx,
        }
    }

    /// Add a fact to the index
    pub fn add_fact(
        &self,
        predicate: String,
        value: FactValue,
        authority: Option<AuthorityId>,
        timestamp: Option<TimeStamp>,
    ) -> FactId {
        // Insert into B-tree indexes
        let id = {
            let mut index = self.index.write();
            index.insert(predicate.clone(), value.clone(), authority, timestamp)
        };

        // Insert into Bloom filter
        let element = self.fact_to_bytes(&predicate, &value);
        {
            let mut filter = self.bloom_filter.write();
            // Direct bloom filter insertion (synchronous for lock-based access)
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

                if byte_index < filter.bits.len() {
                    filter.bits[byte_index] |= 1u8 << bit_offset;
                }
            }
            filter.element_count += 1;
        }

        // Add to Merkle tree hashes
        {
            let fact_hash = aura_core::hash::hash(&element);
            let mut hashes = self.fact_hashes.write();
            hashes.insert(fact_hash);
        }

        // Invalidate Merkle cache
        {
            let mut cache = self.merkle_root_cache.write();
            *cache = None;
        }

        // Broadcast the new fact to stream subscribers
        // We retrieve the fact we just added to broadcast it
        let fact_to_broadcast = {
            let index = self.index.read();
            index.facts.get(&id).cloned()
        };

        if let Some(fact) = fact_to_broadcast {
            // Send as a batch of one fact
            // Ignore send errors (happens when there are no subscribers)
            let _ = self.fact_updates.send(vec![fact]);
        }

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
        let filter = self.bloom_filter.read();

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
    }

    /// Compute or retrieve cached Merkle root
    pub(crate) fn compute_merkle_root(&self) -> [u8; 32] {
        // Check cache first
        {
            let cache = self.merkle_root_cache.read();
            if let Some(root) = *cache {
                return root;
            }
        }

        // Compute Merkle root
        let hashes: Vec<[u8; 32]> = {
            let fact_hashes = self.fact_hashes.read();
            fact_hashes.iter().copied().collect()
        };

        let root = if let Some(tree) = build_merkle_tree(hashes) {
            tree.hash
        } else {
            [0u8; 32] // Empty tree
        };

        // Cache the result
        {
            let mut cache = self.merkle_root_cache.write();
            *cache = Some(root);
        }

        root
    }
}

impl Default for IndexedJournalHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for IndexedJournalHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let index = self.index.read();
        f.debug_struct("IndexedJournalHandler")
            .field("fact_count", &index.facts.len())
            .field("predicate_count", &index.by_predicate.len())
            .field("authority_count", &index.by_authority.len())
            .finish()
    }
}

#[async_trait]
impl IndexedJournalEffects for IndexedJournalHandler {
    fn watch_facts(&self) -> Box<dyn FactStreamReceiver> {
        Box::new(TokioFactStreamReceiver::new(self.fact_updates.subscribe()))
    }

    async fn facts_by_predicate(&self, predicate: &str) -> Result<Vec<IndexedFact>, AuraError> {
        let index = self.index.read();
        Ok(index.get_by_predicate(predicate))
    }

    async fn facts_by_authority(
        &self,
        authority: &AuthorityId,
    ) -> Result<Vec<IndexedFact>, AuraError> {
        let index = self.index.read();
        Ok(index.get_by_authority(authority))
    }

    async fn facts_in_range(
        &self,
        start: TimeStamp,
        end: TimeStamp,
    ) -> Result<Vec<IndexedFact>, AuraError> {
        let index = self.index.read();
        Ok(index.get_in_range(&start, &end))
    }

    async fn all_facts(&self) -> Result<Vec<IndexedFact>, AuraError> {
        let index = self.index.read();
        Ok(index.facts.values().cloned().collect())
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

        let hashes = self.fact_hashes.read();
        Ok(hashes.contains(&fact_hash))
    }

    async fn get_bloom_filter(&self) -> Result<BloomFilter, AuraError> {
        let filter = self.bloom_filter.read();
        Ok(filter.clone())
    }

    async fn index_stats(&self) -> Result<IndexStats, AuraError> {
        let index = self.index.read();
        let filter = self.bloom_filter.read();

        let mut stats = index.stats();
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
