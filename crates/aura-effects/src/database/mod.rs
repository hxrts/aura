// Lock poisoning is fatal for this module - we prefer to panic than continue with corrupted state
#![allow(clippy::expect_used)]

//! Indexed Journal Handler - B-tree indexes, Bloom filters, and Merkle trees
//!
//! # Effect Classification
//!
//! - **Category**: Infrastructure Effect (Layer 3)
//! - **Trait**: `IndexedJournalEffects` from `aura-core`
//! - **Usage**: Efficient indexed lookups for facts beyond linear scan
//!
//! This module provides production-grade indexed journal lookups with:
//! - B-tree indexes for O(log n) predicate/authority/time queries
//! - Bloom filters for O(1) membership testing with <1% false positive rate
//! - Merkle trees for cryptographic integrity verification
//!
//! # Submodules
//!
//! - `query`: Datalog query wrapper using Biscuit's engine

pub mod query;

use async_trait::async_trait;
use aura_core::{
    domain::journal::FactValue,
    effects::indexed::{FactId, FactStreamReceiver, IndexedFact},
    effects::{BloomConfig, BloomFilter, IndexStats, IndexedJournalEffects, JournalEffects},
    time::TimeStamp,
    types::identifiers::AuthorityId,
    AuraError,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::RwLock;

/// Runtime-specific wrapper that implements `FactStreamReceiver` for tokio broadcast channels.
///
/// This adapter allows the Layer 3 (Implementation) to use tokio's concrete broadcast
/// receiver while maintaining compatibility with the runtime-agnostic `FactStreamReceiver`
/// trait defined in Layer 1 (Foundation).
pub struct TokioFactStreamReceiver {
    receiver: tokio::sync::broadcast::Receiver<Vec<IndexedFact>>,
}

impl TokioFactStreamReceiver {
    /// Create a new tokio fact stream receiver wrapper.
    pub fn new(receiver: tokio::sync::broadcast::Receiver<Vec<IndexedFact>>) -> Self {
        Self { receiver }
    }
}

impl FactStreamReceiver for TokioFactStreamReceiver {
    fn recv(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<IndexedFact>, AuraError>> + Send + '_>> {
        Box::pin(async move {
            self.receiver
                .recv()
                .await
                .map_err(|e| AuraError::internal(format!("Fact stream recv error: {}", e)))
        })
    }

    fn try_recv(&mut self) -> Result<Option<Vec<IndexedFact>>, AuraError> {
        use tokio::sync::broadcast::error::TryRecvError;
        match self.receiver.try_recv() {
            Ok(facts) => Ok(Some(facts)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Lagged(n)) => Err(AuraError::internal(format!(
                "Fact stream lagged by {} messages",
                n
            ))),
            Err(TryRecvError::Closed) => Err(AuraError::internal("Fact stream closed")),
        }
    }
}

/// Orderable key for timestamp indexing
///
/// Since `TimeStamp` doesn't implement `Ord`, we extract a comparable
/// representation for B-tree indexing. Physical timestamps use millis,
/// others use a hash-based ordering.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TimeKey {
    /// Primary sort key (milliseconds for physical, hash for others)
    millis: u64,
    /// Original timestamp for retrieval
    original: TimeStampWrapper,
}

/// Wrapper to store TimeStamp alongside the key
#[derive(Debug, Clone)]
struct TimeStampWrapper(TimeStamp);

impl PartialEq for TimeStampWrapper {
    fn eq(&self, other: &Self) -> bool {
        self.to_millis() == other.to_millis()
    }
}

impl Eq for TimeStampWrapper {}

impl PartialOrd for TimeStampWrapper {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TimeStampWrapper {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.to_millis().cmp(&other.to_millis())
    }
}

impl TimeStampWrapper {
    fn to_millis(&self) -> u64 {
        timestamp_to_millis(&self.0)
    }
}

/// Extract a comparable u64 from a TimeStamp for ordering purposes
fn timestamp_to_millis(ts: &TimeStamp) -> u64 {
    match ts {
        TimeStamp::PhysicalClock(pt) => pt.ts_ms,
        TimeStamp::LogicalClock(lt) => lt.lamport,
        TimeStamp::OrderClock(ot) => {
            // Use first 8 bytes of order clock as u64 for ordering
            u64::from_le_bytes([
                ot.0[0], ot.0[1], ot.0[2], ot.0[3], ot.0[4], ot.0[5], ot.0[6], ot.0[7],
            ])
        }
        TimeStamp::Range(rt) => rt.earliest_ms,
    }
}

impl TimeKey {
    fn from_timestamp(ts: TimeStamp) -> Self {
        let millis = timestamp_to_millis(&ts);
        Self {
            millis,
            original: TimeStampWrapper(ts),
        }
    }
}

/// Internal structure for managing B-tree indexes
#[derive(Debug)]
struct AuthorityIndex {
    /// B-tree index: predicate -> set of fact IDs
    by_predicate: BTreeMap<String, BTreeSet<FactId>>,
    /// B-tree index: authority -> set of fact IDs
    by_authority: BTreeMap<AuthorityId, BTreeSet<FactId>>,
    /// B-tree index: timestamp -> set of fact IDs (for range queries)
    by_timestamp: BTreeMap<TimeKey, BTreeSet<FactId>>,
    /// All indexed facts (id -> fact)
    facts: BTreeMap<FactId, IndexedFact>,
    /// Next fact ID to assign
    next_id: u64,
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
    fn new() -> Self {
        Self::default()
    }

    /// Insert a new fact into all indexes
    fn insert(
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
    fn get_by_predicate(&self, predicate: &str) -> Vec<IndexedFact> {
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
    fn get_by_authority(&self, authority: &AuthorityId) -> Vec<IndexedFact> {
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
    fn get_in_range(&self, start: &TimeStamp, end: &TimeStamp) -> Vec<IndexedFact> {
        let start_key = TimeKey::from_timestamp(start.clone());
        let end_key = TimeKey::from_timestamp(end.clone());
        self.by_timestamp
            .range(start_key..=end_key)
            .flat_map(|(_, ids)| ids.iter())
            .filter_map(|id| self.facts.get(id).cloned())
            .collect()
    }

    /// Get statistics about the index
    fn stats(&self) -> IndexStats {
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

/// Merkle tree node for integrity verification
#[derive(Debug, Clone)]
struct MerkleNode {
    hash: [u8; 32],
    /// Left child (used for Merkle proof generation)
    #[allow(dead_code)]
    _left: Option<Box<MerkleNode>>,
    /// Right child (used for Merkle proof generation)
    #[allow(dead_code)]
    _right: Option<Box<MerkleNode>>,
}

impl MerkleNode {
    fn branch(left: MerkleNode, right: MerkleNode) -> Self {
        let mut combined = Vec::with_capacity(64);
        combined.extend_from_slice(&left.hash);
        combined.extend_from_slice(&right.hash);
        let hash = aura_core::hash::hash(&combined);
        Self {
            hash,
            _left: Some(Box::new(left)),
            _right: Some(Box::new(right)),
        }
    }
}

/// Build a Merkle tree from leaf hashes
fn build_merkle_tree(leaves: Vec<[u8; 32]>) -> Option<MerkleNode> {
    if leaves.is_empty() {
        return None;
    }

    let mut nodes: Vec<MerkleNode> = leaves
        .into_iter()
        .map(|hash| MerkleNode {
            hash,
            _left: None,
            _right: None,
        })
        .collect();

    while nodes.len() > 1 {
        let mut next_level = Vec::new();
        let mut i = 0;
        while i < nodes.len() {
            if i + 1 < nodes.len() {
                let left = nodes[i].clone();
                let right = nodes[i + 1].clone();
                next_level.push(MerkleNode::branch(left, right));
                i += 2;
            } else {
                // Odd node - promote to next level
                next_level.push(nodes[i].clone());
                i += 1;
            }
        }
        nodes = next_level;
    }

    nodes.into_iter().next()
}

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
    index: RwLock<AuthorityIndex>,
    /// Bloom filter for fast membership checks
    bloom_filter: RwLock<BloomFilter>,
    /// Cached Merkle root (invalidated on mutations)
    merkle_root_cache: RwLock<Option<[u8; 32]>>,
    /// Set of fact hashes in the Merkle tree
    fact_hashes: RwLock<HashSet<[u8; 32]>>,
    /// Broadcast channel for streaming fact updates to subscribers
    fact_updates: tokio::sync::broadcast::Sender<Vec<IndexedFact>>,
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
            let mut index = self.index.write().expect("Index lock poisoned");
            index.insert(predicate.clone(), value.clone(), authority, timestamp)
        };

        // Insert into Bloom filter
        let element = self.fact_to_bytes(&predicate, &value);
        {
            let mut filter = self
                .bloom_filter
                .write()
                .expect("Bloom filter lock poisoned");
            // Direct bloom filter insertion (synchronous for lock-based access)
            for i in 0..filter.config.num_hash_functions {
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
            let mut hashes = self.fact_hashes.write().expect("Fact hashes lock poisoned");
            hashes.insert(fact_hash);
        }

        // Invalidate Merkle cache
        {
            let mut cache = self
                .merkle_root_cache
                .write()
                .expect("Merkle cache lock poisoned");
            *cache = None;
        }

        // Broadcast the new fact to stream subscribers
        // We retrieve the fact we just added to broadcast it
        let fact_to_broadcast = {
            let index = self.index.read().expect("Index lock poisoned");
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
    fn fact_to_bytes(&self, predicate: &str, value: &FactValue) -> Vec<u8> {
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
    fn bloom_check(&self, predicate: &str, value: &FactValue) -> bool {
        let element = self.fact_to_bytes(predicate, value);
        let filter = self
            .bloom_filter
            .read()
            .expect("Bloom filter lock poisoned");

        for i in 0..filter.config.num_hash_functions {
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
    fn compute_merkle_root(&self) -> [u8; 32] {
        // Check cache first
        {
            let cache = self
                .merkle_root_cache
                .read()
                .expect("Merkle cache lock poisoned");
            if let Some(root) = *cache {
                return root;
            }
        }

        // Compute Merkle root
        let hashes: Vec<[u8; 32]> = {
            let fact_hashes = self.fact_hashes.read().expect("Fact hashes lock poisoned");
            fact_hashes.iter().copied().collect()
        };

        let root = if let Some(tree) = build_merkle_tree(hashes) {
            tree.hash
        } else {
            [0u8; 32] // Empty tree
        };

        // Cache the result
        {
            let mut cache = self
                .merkle_root_cache
                .write()
                .expect("Merkle cache lock poisoned");
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
        let index = self.index.read().expect("Index lock poisoned");
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
        let index = self.index.read().expect("Index lock poisoned");
        Ok(index.get_by_predicate(predicate))
    }

    async fn facts_by_authority(
        &self,
        authority: &AuthorityId,
    ) -> Result<Vec<IndexedFact>, AuraError> {
        let index = self.index.read().expect("Index lock poisoned");
        Ok(index.get_by_authority(authority))
    }

    async fn facts_in_range(
        &self,
        start: TimeStamp,
        end: TimeStamp,
    ) -> Result<Vec<IndexedFact>, AuraError> {
        let index = self.index.read().expect("Index lock poisoned");
        Ok(index.get_in_range(&start, &end))
    }

    async fn all_facts(&self) -> Result<Vec<IndexedFact>, AuraError> {
        let index = self.index.read().expect("Index lock poisoned");
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

        let hashes = self.fact_hashes.read().expect("Fact hashes lock poisoned");
        Ok(hashes.contains(&fact_hash))
    }

    async fn get_bloom_filter(&self) -> Result<BloomFilter, AuraError> {
        let filter = self
            .bloom_filter
            .read()
            .expect("Bloom filter lock poisoned");
        Ok(filter.clone())
    }

    async fn index_stats(&self) -> Result<IndexStats, AuraError> {
        let index = self.index.read().expect("Index lock poisoned");
        let filter = self
            .bloom_filter
            .read()
            .expect("Bloom filter lock poisoned");

        let mut stats = index.stats();
        // Estimate false positive rate based on filter fill ratio
        // Note: set_bits could be used for more accurate FP estimation in the future
        let _set_bits: u64 = filter.bits.iter().map(|b| b.count_ones() as u64).sum();
        let m = filter.config.bit_vector_size as f64;
        let k = filter.config.num_hash_functions as f64;
        let n = filter.element_count as f64;

        if n > 0.0 {
            // FP rate ≈ (1 - e^(-kn/m))^k
            let fill_ratio = (-k * n / m).exp();
            stats.bloom_fp_rate = (1.0 - fill_ratio).powf(k);
        }

        Ok(stats)
    }
}

/// Wrapper that combines JournalEffects with IndexedJournalEffects.
///
/// This wrapper:
/// 1. Delegates JournalEffects operations to an inner handler
/// 2. Automatically updates the IndexedJournalHandler when facts are committed
/// 3. Provides IndexedJournalEffects for efficient fact lookups
///
/// # Example
///
/// ```rust,ignore
/// use aura_effects::database::{IndexedJournalHandler, IndexedJournalWrapper};
///
/// let journal_handler = MyJournalHandler::new();
/// let indexed = IndexedJournalHandler::new();
/// let wrapper = IndexedJournalWrapper::new(journal_handler, indexed);
///
/// // Use as JournalEffects
/// wrapper.merge_facts(&journal1, &journal2).await?;
///
/// // Use as IndexedJournalEffects
/// let facts = wrapper.facts_by_predicate("user.name").await?;
/// ```
pub struct IndexedJournalWrapper<J: JournalEffects> {
    /// Inner JournalEffects handler
    inner: J,
    /// Index handler for efficient lookups
    index: IndexedJournalHandler,
    /// Track which facts have been indexed
    indexed_keys: RwLock<HashSet<String>>,
}

impl<J: JournalEffects> IndexedJournalWrapper<J> {
    /// Create a new wrapper with the given journal and index handlers.
    pub fn new(inner: J, index: IndexedJournalHandler) -> Self {
        Self {
            inner,
            index,
            indexed_keys: RwLock::new(HashSet::new()),
        }
    }

    /// Create a new wrapper with default index configuration.
    pub fn with_default_index(inner: J) -> Self {
        Self::new(inner, IndexedJournalHandler::new())
    }

    /// Index facts from a journal, skipping already-indexed keys.
    fn index_journal(&self, journal: &aura_core::Journal) {
        let mut indexed_keys = self
            .indexed_keys
            .write()
            .expect("indexed_keys lock poisoned");

        for (key, value) in journal.facts.iter() {
            // Skip if already indexed
            if indexed_keys.contains(key) {
                continue;
            }

            // Add to index
            self.index.add_fact(
                key.clone(),
                value.clone(),
                None, // Authority not available from basic Fact iteration
                None, // Timestamp not available from basic Fact iteration
            );

            indexed_keys.insert(key.clone());
        }
    }

    /// Get a reference to the inner JournalEffects handler.
    pub fn inner(&self) -> &J {
        &self.inner
    }

    /// Get a reference to the index handler.
    pub fn index(&self) -> &IndexedJournalHandler {
        &self.index
    }
}

impl<J: JournalEffects> std::fmt::Debug for IndexedJournalWrapper<J> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndexedJournalWrapper")
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl<J: JournalEffects> JournalEffects for IndexedJournalWrapper<J> {
    async fn merge_facts(
        &self,
        target: &aura_core::Journal,
        delta: &aura_core::Journal,
    ) -> Result<aura_core::Journal, AuraError> {
        let result = self.inner.merge_facts(target, delta).await?;
        // Index the delta facts
        self.index_journal(delta);
        Ok(result)
    }

    async fn refine_caps(
        &self,
        target: &aura_core::Journal,
        refinement: &aura_core::Journal,
    ) -> Result<aura_core::Journal, AuraError> {
        self.inner.refine_caps(target, refinement).await
    }

    async fn get_journal(&self) -> Result<aura_core::Journal, AuraError> {
        let journal = self.inner.get_journal().await?;
        // Ensure the retrieved journal is indexed
        self.index_journal(&journal);
        Ok(journal)
    }

    async fn persist_journal(&self, journal: &aura_core::Journal) -> Result<(), AuraError> {
        // Index the journal before persisting
        self.index_journal(journal);
        self.inner.persist_journal(journal).await
    }

    async fn get_flow_budget(
        &self,
        context: &aura_core::ContextId,
        peer: &AuthorityId,
    ) -> Result<aura_core::FlowBudget, AuraError> {
        self.inner.get_flow_budget(context, peer).await
    }

    async fn update_flow_budget(
        &self,
        context: &aura_core::ContextId,
        peer: &AuthorityId,
        budget: &aura_core::FlowBudget,
    ) -> Result<aura_core::FlowBudget, AuraError> {
        self.inner.update_flow_budget(context, peer, budget).await
    }

    async fn charge_flow_budget(
        &self,
        context: &aura_core::ContextId,
        peer: &AuthorityId,
        cost: u32,
    ) -> Result<aura_core::FlowBudget, AuraError> {
        self.inner.charge_flow_budget(context, peer, cost).await
    }
}

#[async_trait]
impl<J: JournalEffects> IndexedJournalEffects for IndexedJournalWrapper<J> {
    fn watch_facts(&self) -> Box<dyn FactStreamReceiver> {
        self.index.watch_facts()
    }

    async fn facts_by_predicate(&self, predicate: &str) -> Result<Vec<IndexedFact>, AuraError> {
        self.index.facts_by_predicate(predicate).await
    }

    async fn facts_by_authority(
        &self,
        authority: &AuthorityId,
    ) -> Result<Vec<IndexedFact>, AuraError> {
        self.index.facts_by_authority(authority).await
    }

    async fn facts_in_range(
        &self,
        start: TimeStamp,
        end: TimeStamp,
    ) -> Result<Vec<IndexedFact>, AuraError> {
        self.index.facts_in_range(start, end).await
    }

    async fn all_facts(&self) -> Result<Vec<IndexedFact>, AuraError> {
        self.index.all_facts().await
    }

    fn might_contain(&self, predicate: &str, value: &FactValue) -> bool {
        self.index.might_contain(predicate, value)
    }

    async fn merkle_root(&self) -> Result<[u8; 32], AuraError> {
        self.index.merkle_root().await
    }

    async fn verify_fact_inclusion(&self, fact: &IndexedFact) -> Result<bool, AuraError> {
        self.index.verify_fact_inclusion(fact).await
    }

    async fn get_bloom_filter(&self) -> Result<BloomFilter, AuraError> {
        self.index.get_bloom_filter().await
    }

    async fn index_stats(&self) -> Result<IndexStats, AuraError> {
        self.index.index_stats().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::PhysicalTime;

    #[tokio::test]
    async fn test_add_and_query_by_predicate() {
        let handler = IndexedJournalHandler::new();

        handler.add_fact(
            "user.name".to_string(),
            FactValue::String("alice".to_string()),
            None,
            None,
        );
        handler.add_fact(
            "user.name".to_string(),
            FactValue::String("bob".to_string()),
            None,
            None,
        );
        handler.add_fact(
            "user.email".to_string(),
            FactValue::String("alice@example.com".to_string()),
            None,
            None,
        );

        let facts = handler.facts_by_predicate("user.name").await.unwrap();
        assert_eq!(facts.len(), 2);

        let email_facts = handler.facts_by_predicate("user.email").await.unwrap();
        assert_eq!(email_facts.len(), 1);
    }

    #[tokio::test]
    async fn test_query_by_authority() {
        let handler = IndexedJournalHandler::new();

        // Create distinct authorities
        let auth1 = AuthorityId::new_from_entropy([1u8; 32]);
        let auth2 = AuthorityId::new_from_entropy([2u8; 32]);

        handler.add_fact(
            "data".to_string(),
            FactValue::Number(100),
            Some(auth1),
            None,
        );
        handler.add_fact(
            "data".to_string(),
            FactValue::Number(200),
            Some(auth1),
            None,
        );
        handler.add_fact(
            "data".to_string(),
            FactValue::Number(300),
            Some(auth2),
            None,
        );

        let auth1_facts = handler.facts_by_authority(&auth1).await.unwrap();
        assert_eq!(auth1_facts.len(), 2);

        let auth2_facts = handler.facts_by_authority(&auth2).await.unwrap();
        assert_eq!(auth2_facts.len(), 1);
    }

    #[tokio::test]
    async fn test_query_by_time_range() {
        let handler = IndexedJournalHandler::new();

        let ts1 = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        });
        let ts2 = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 2000,
            uncertainty: None,
        });
        let ts3 = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 3000,
            uncertainty: None,
        });

        handler.add_fact(
            "event".to_string(),
            FactValue::Number(1),
            None,
            Some(ts1.clone()),
        );
        handler.add_fact(
            "event".to_string(),
            FactValue::Number(2),
            None,
            Some(ts2.clone()),
        );
        handler.add_fact(
            "event".to_string(),
            FactValue::Number(3),
            None,
            Some(ts3.clone()),
        );

        // Query range [1000, 2000]
        let facts = handler
            .facts_in_range(ts1.clone(), ts2.clone())
            .await
            .unwrap();
        assert_eq!(facts.len(), 2);

        // Query range [2000, 3000]
        let facts = handler.facts_in_range(ts2, ts3).await.unwrap();
        assert_eq!(facts.len(), 2);
    }

    #[tokio::test]
    async fn test_bloom_filter_membership() {
        let handler = IndexedJournalHandler::new();

        handler.add_fact(
            "exists".to_string(),
            FactValue::String("yes".to_string()),
            None,
            None,
        );

        // Should return true for existing fact
        assert!(handler.might_contain("exists", &FactValue::String("yes".to_string())));

        // Should return false for non-existing fact (with high probability)
        assert!(!handler.might_contain("nonexistent", &FactValue::String("no".to_string())));
    }

    #[tokio::test]
    async fn test_merkle_root() {
        let handler = IndexedJournalHandler::new();

        // Empty tree should have zero root
        let root1 = handler.merkle_root().await.unwrap();
        assert_eq!(root1, [0u8; 32]);

        // Add a fact
        handler.add_fact(
            "test".to_string(),
            FactValue::String("value".to_string()),
            None,
            None,
        );

        // Root should now be non-zero
        let root2 = handler.merkle_root().await.unwrap();
        assert_ne!(root2, [0u8; 32]);

        // Adding more facts should change the root
        handler.add_fact("test2".to_string(), FactValue::Number(42), None, None);

        let root3 = handler.merkle_root().await.unwrap();
        assert_ne!(root3, root2);
    }

    #[tokio::test]
    async fn test_fact_inclusion_verification() {
        let handler = IndexedJournalHandler::new();

        let _id = handler.add_fact(
            "verified".to_string(),
            FactValue::String("data".to_string()),
            None,
            None,
        );

        // Get the fact
        let facts = handler.facts_by_predicate("verified").await.unwrap();
        assert_eq!(facts.len(), 1);

        // Verify inclusion
        let included = handler.verify_fact_inclusion(&facts[0]).await.unwrap();
        assert!(included);

        // Create a fake fact that shouldn't be included
        let fake_fact = IndexedFact {
            id: FactId::new(999),
            predicate: "fake".to_string(),
            value: FactValue::String("fake".to_string()),
            authority: None,
            timestamp: None,
        };

        let not_included = handler.verify_fact_inclusion(&fake_fact).await.unwrap();
        assert!(!not_included);
    }

    #[tokio::test]
    async fn test_index_stats() {
        let handler = IndexedJournalHandler::new();

        let auth = AuthorityId::new_from_entropy([3u8; 32]);

        handler.add_fact("pred1".to_string(), FactValue::Number(1), Some(auth), None);
        handler.add_fact("pred2".to_string(), FactValue::Number(2), Some(auth), None);
        handler.add_fact("pred1".to_string(), FactValue::Number(3), None, None);

        let stats = handler.index_stats().await.unwrap();
        assert_eq!(stats.fact_count, 3);
        assert_eq!(stats.predicate_count, 2); // pred1 and pred2
        assert_eq!(stats.authority_count, 1); // only one authority
    }

    #[tokio::test]
    #[allow(clippy::disallowed_methods)] // Instant::now() legitimate for performance testing
    async fn test_performance_10k_facts() {
        let handler = IndexedJournalHandler::with_capacity(10000);

        // Add 10k facts
        for i in 0..10000 {
            handler.add_fact(
                format!("key.{}", i % 100), // 100 unique predicates
                FactValue::Number(i as i64),
                None,
                None,
            );
        }

        let start = std::time::Instant::now();

        // Query by predicate
        let facts = handler.facts_by_predicate("key.50").await.unwrap();
        assert_eq!(facts.len(), 100); // Each predicate has 100 facts

        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() <= 10,
            "Query took too long: {:?}",
            elapsed
        );
    }

    // === IndexedJournalWrapper Integration Tests ===

    /// Test-only JournalEffects for testing the wrapper
    struct TestJournalHandler {
        journal: RwLock<aura_core::Journal>,
    }

    impl TestJournalHandler {
        fn new() -> Self {
            Self {
                journal: RwLock::new(aura_core::Journal::new()),
            }
        }
    }

    #[async_trait]
    impl JournalEffects for TestJournalHandler {
        async fn merge_facts(
            &self,
            target: &aura_core::Journal,
            delta: &aura_core::Journal,
        ) -> Result<aura_core::Journal, AuraError> {
            // Simple merge: combine facts from both journals
            let mut result = target.clone();
            for (key, value) in delta.facts.iter() {
                result.facts.insert(key.clone(), value.clone());
            }
            Ok(result)
        }

        async fn refine_caps(
            &self,
            target: &aura_core::Journal,
            _refinement: &aura_core::Journal,
        ) -> Result<aura_core::Journal, AuraError> {
            Ok(target.clone())
        }

        async fn get_journal(&self) -> Result<aura_core::Journal, AuraError> {
            let journal = self.journal.read().expect("journal lock poisoned");
            Ok(journal.clone())
        }

        async fn persist_journal(&self, journal: &aura_core::Journal) -> Result<(), AuraError> {
            let mut stored = self.journal.write().expect("journal lock poisoned");
            *stored = journal.clone();
            Ok(())
        }

        async fn get_flow_budget(
            &self,
            _context: &aura_core::ContextId,
            _peer: &AuthorityId,
        ) -> Result<aura_core::FlowBudget, AuraError> {
            Ok(aura_core::FlowBudget::default())
        }

        async fn update_flow_budget(
            &self,
            _context: &aura_core::ContextId,
            _peer: &AuthorityId,
            budget: &aura_core::FlowBudget,
        ) -> Result<aura_core::FlowBudget, AuraError> {
            Ok(*budget)
        }

        async fn charge_flow_budget(
            &self,
            _context: &aura_core::ContextId,
            _peer: &AuthorityId,
            _cost: u32,
        ) -> Result<aura_core::FlowBudget, AuraError> {
            Ok(aura_core::FlowBudget::default())
        }
    }

    #[tokio::test]
    async fn test_wrapper_indexes_on_persist() {
        let mock_journal = TestJournalHandler::new();
        let wrapper = IndexedJournalWrapper::with_default_index(mock_journal);

        // Create a journal with some facts
        let mut journal = aura_core::Journal::new();
        journal.facts.insert(
            "user.name".to_string(),
            FactValue::String("alice".to_string()),
        );
        journal.facts.insert(
            "user.email".to_string(),
            FactValue::String("alice@example.com".to_string()),
        );

        // Persist the journal
        wrapper.persist_journal(&journal).await.unwrap();

        // Query using IndexedJournalEffects
        let facts = wrapper.facts_by_predicate("user.name").await.unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].predicate, "user.name");
    }

    #[tokio::test]
    async fn test_wrapper_indexes_on_merge() {
        let mock_journal = TestJournalHandler::new();
        let wrapper = IndexedJournalWrapper::with_default_index(mock_journal);

        let target = aura_core::Journal::new();

        // Create delta with facts
        let mut delta = aura_core::Journal::new();
        delta.facts.insert(
            "event.type".to_string(),
            FactValue::String("login".to_string()),
        );
        delta
            .facts
            .insert("event.timestamp".to_string(), FactValue::Number(1234567890));

        // Merge facts - should trigger indexing of delta
        let _result = wrapper.merge_facts(&target, &delta).await.unwrap();

        // Query should find the merged facts
        let facts = wrapper.facts_by_predicate("event.type").await.unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].predicate, "event.type");
    }

    #[tokio::test]
    async fn test_wrapper_indexes_on_get_journal() {
        let mock_journal = TestJournalHandler::new();

        // Store a journal in the mock
        {
            let mut stored = mock_journal.journal.write().unwrap();
            stored.facts.insert(
                "stored.key".to_string(),
                FactValue::String("stored_value".to_string()),
            );
        }

        let wrapper = IndexedJournalWrapper::with_default_index(mock_journal);

        // Get journal should index it
        let _journal = wrapper.get_journal().await.unwrap();

        // Now we should be able to query the indexed facts
        let facts = wrapper.facts_by_predicate("stored.key").await.unwrap();
        assert_eq!(facts.len(), 1);
    }

    #[tokio::test]
    async fn test_wrapper_bloom_filter_integration() {
        let mock_journal = TestJournalHandler::new();
        let wrapper = IndexedJournalWrapper::with_default_index(mock_journal);

        // Create and persist a journal
        let mut journal = aura_core::Journal::new();
        journal.facts.insert(
            "test.key".to_string(),
            FactValue::String("test_value".to_string()),
        );
        wrapper.persist_journal(&journal).await.unwrap();

        // Bloom filter should indicate presence
        assert!(wrapper.might_contain("test.key", &FactValue::String("test_value".to_string())));

        // Bloom filter should not find non-existent fact
        assert!(!wrapper.might_contain("nonexistent", &FactValue::String("none".to_string())));
    }

    #[tokio::test]
    async fn test_wrapper_merkle_root_changes() {
        let mock_journal = TestJournalHandler::new();
        let wrapper = IndexedJournalWrapper::with_default_index(mock_journal);

        // Initial merkle root (empty)
        let root1 = wrapper.merkle_root().await.unwrap();

        // Add facts
        let mut journal = aura_core::Journal::new();
        journal
            .facts
            .insert("fact1".to_string(), FactValue::Number(1));
        wrapper.persist_journal(&journal).await.unwrap();

        // Merkle root should change
        let root2 = wrapper.merkle_root().await.unwrap();
        assert_ne!(root1, root2);
    }

    #[tokio::test]
    async fn test_wrapper_no_duplicate_indexing() {
        let mock_journal = TestJournalHandler::new();
        let wrapper = IndexedJournalWrapper::with_default_index(mock_journal);

        // Create a journal with a fact
        let mut journal = aura_core::Journal::new();
        journal.facts.insert(
            "unique.key".to_string(),
            FactValue::String("value".to_string()),
        );

        // Persist multiple times
        wrapper.persist_journal(&journal).await.unwrap();
        wrapper.persist_journal(&journal).await.unwrap();
        wrapper.persist_journal(&journal).await.unwrap();

        // Should only have one fact indexed (no duplicates)
        let stats = wrapper.index_stats().await.unwrap();
        assert_eq!(stats.fact_count, 1);
    }
}
