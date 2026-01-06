//! Indexed journal wrapper combining JournalEffects with IndexedJournalEffects.

use super::handler::IndexedJournalHandler;
use async_trait::async_trait;
use aura_core::{
    domain::journal::FactValue,
    effects::indexed::{FactStreamReceiver, IndexedFact},
    effects::{BloomFilter, IndexStats, IndexedJournalEffects, JournalEffects},
    time::TimeStamp,
    types::identifiers::AuthorityId,
    AuraError,
};
use parking_lot::RwLock;
use std::collections::HashSet;

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
/// use aura_agent::database::{IndexedJournalHandler, IndexedJournalWrapper};
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
        let mut indexed_keys = self.indexed_keys.write();

        for (key, value) in journal.facts.iter() {
            // Skip if already indexed
            if indexed_keys.contains(key.as_str()) {
                continue;
            }

            let key_owned = key.as_str().to_string();

            // Add to index
            self.index.add_fact(
                key_owned.clone(),
                value.clone(),
                None, // Authority not available from basic Fact iteration
                None, // Timestamp not available from basic Fact iteration
            );

            indexed_keys.insert(key_owned);
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
        target: aura_core::Journal,
        delta: aura_core::Journal,
    ) -> Result<aura_core::Journal, AuraError> {
        // Index the delta facts before consuming it
        self.index_journal(&delta);
        let result = self.inner.merge_facts(target, delta).await?;
        Ok(result)
    }

    async fn refine_caps(
        &self,
        target: aura_core::Journal,
        refinement: aura_core::Journal,
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
        cost: aura_core::FlowCost,
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
