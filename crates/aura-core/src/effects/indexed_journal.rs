//! Indexed Journal Effects - Extension trait for efficient fact lookups
//!
//! # Effect Classification
//!
//! - **Category**: Application Effect (extends JournalEffects)
//! - **Implementation**: `aura-effects/src/database/` (Layer 3)
//! - **Usage**: Efficient indexed lookups for facts beyond linear scan
//!
//! This trait extends `JournalEffects` with indexed query capabilities,
//! enabling O(log n) lookups by predicate, authority, and time range.

use crate::{
    domain::journal::FactValue, effects::BloomFilter, time::TimeStamp,
    types::identifiers::AuthorityId, AuraError,
};
use async_trait::async_trait;

/// A fact identifier for indexing purposes.
///
/// This is a lightweight reference to a fact in the journal,
/// avoiding the need to clone entire facts for index lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FactId(pub u64);

impl FactId {
    /// Create a new fact ID.
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

/// An indexed fact entry containing both the key and value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFact {
    /// Unique identifier for this fact
    pub id: FactId,
    /// The predicate/key of the fact
    pub predicate: String,
    /// The value of the fact
    pub value: FactValue,
    /// The authority that created this fact (if known)
    pub authority: Option<AuthorityId>,
    /// Timestamp when this fact was created
    pub timestamp: Option<TimeStamp>,
}

/// Extension trait for indexed journal lookups.
///
/// Provides efficient O(log n) lookups using B-tree indexes,
/// Bloom filters for fast membership testing, and Merkle trees
/// for integrity verification.
///
/// # Performance Requirements
///
/// - `facts_by_predicate`: ≤10ms for 10k facts
/// - `facts_by_authority`: ≤10ms for 10k facts
/// - `facts_in_range`: ≤10ms for 10k facts
/// - `might_contain`: O(1) with <1% false positive rate
#[async_trait]
pub trait IndexedJournalEffects: Send + Sync {
    /// Get all facts with the given predicate/key.
    ///
    /// Uses B-tree index for O(log n) lookup.
    async fn facts_by_predicate(&self, predicate: &str) -> Result<Vec<IndexedFact>, AuraError>;

    /// Get all facts created by the given authority.
    ///
    /// Uses B-tree index for O(log n) lookup.
    async fn facts_by_authority(
        &self,
        authority: &AuthorityId,
    ) -> Result<Vec<IndexedFact>, AuraError>;

    /// Get all facts within the given time range (inclusive).
    ///
    /// Uses B-tree index for O(log n) lookup.
    async fn facts_in_range(
        &self,
        start: TimeStamp,
        end: TimeStamp,
    ) -> Result<Vec<IndexedFact>, AuraError>;

    /// Return all indexed facts (append-only view).
    async fn all_facts(&self) -> Result<Vec<IndexedFact>, AuraError>;

    /// Fast membership test using Bloom filter.
    ///
    /// Returns `true` if the predicate/value pair might be in the index,
    /// `false` if it is definitely not present.
    ///
    /// This is O(1) with a false positive rate of <1%.
    fn might_contain(&self, predicate: &str, value: &FactValue) -> bool;

    /// Get the Merkle root commitment for the current index state.
    ///
    /// This can be used to verify integrity across replicas.
    async fn merkle_root(&self) -> Result<[u8; 32], AuraError>;

    /// Verify a fact against the Merkle tree.
    ///
    /// Returns `true` if the fact is included in the committed state.
    async fn verify_fact_inclusion(&self, fact: &IndexedFact) -> Result<bool, AuraError>;

    /// Get the Bloom filter for fast membership tests.
    ///
    /// This can be serialized and sent to peers for efficient
    /// set reconciliation.
    async fn get_bloom_filter(&self) -> Result<BloomFilter, AuraError>;

    /// Get statistics about the index.
    async fn index_stats(&self) -> Result<IndexStats, AuraError>;
}

/// Statistics about the journal index.
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    /// Total number of facts indexed
    pub fact_count: u64,
    /// Number of unique predicates
    pub predicate_count: u64,
    /// Number of unique authorities
    pub authority_count: u64,
    /// Bloom filter false positive rate estimate
    pub bloom_fp_rate: f64,
    /// Merkle tree depth
    pub merkle_depth: u32,
}

/// Blanket implementation for Arc<T> where T: IndexedJournalEffects
#[async_trait]
impl<T: IndexedJournalEffects + ?Sized> IndexedJournalEffects for std::sync::Arc<T> {
    async fn facts_by_predicate(&self, predicate: &str) -> Result<Vec<IndexedFact>, AuraError> {
        (**self).facts_by_predicate(predicate).await
    }

    async fn facts_by_authority(
        &self,
        authority: &AuthorityId,
    ) -> Result<Vec<IndexedFact>, AuraError> {
        (**self).facts_by_authority(authority).await
    }

    async fn facts_in_range(
        &self,
        start: TimeStamp,
        end: TimeStamp,
    ) -> Result<Vec<IndexedFact>, AuraError> {
        (**self).facts_in_range(start, end).await
    }

    async fn all_facts(&self) -> Result<Vec<IndexedFact>, AuraError> {
        (**self).all_facts().await
    }

    fn might_contain(&self, predicate: &str, value: &FactValue) -> bool {
        (**self).might_contain(predicate, value)
    }

    async fn merkle_root(&self) -> Result<[u8; 32], AuraError> {
        (**self).merkle_root().await
    }

    async fn verify_fact_inclusion(&self, fact: &IndexedFact) -> Result<bool, AuraError> {
        (**self).verify_fact_inclusion(fact).await
    }

    async fn get_bloom_filter(&self) -> Result<BloomFilter, AuraError> {
        (**self).get_bloom_filter().await
    }

    async fn index_stats(&self) -> Result<IndexStats, AuraError> {
        (**self).index_stats().await
    }
}
