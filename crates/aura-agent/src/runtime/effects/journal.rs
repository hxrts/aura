use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::effects::indexed;
use aura_core::effects::{BloomFilter, IndexedJournalEffects, JournalEffects};
use aura_core::{AuraError, AuthorityId, ContextId, FlowBudget, FlowCost, Journal};

// Implementation of JournalEffects
#[async_trait]
impl JournalEffects for AuraEffectSystem {
    async fn merge_facts(&self, target: &Journal, delta: &Journal) -> Result<Journal, AuraError> {
        self.journal_handler().merge_facts(target, delta).await
    }

    async fn refine_caps(
        &self,
        target: &Journal,
        refinement: &Journal,
    ) -> Result<Journal, AuraError> {
        self.journal_handler().refine_caps(target, refinement).await
    }

    async fn get_journal(&self) -> Result<Journal, AuraError> {
        self.journal_handler().get_journal().await
    }

    async fn persist_journal(&self, journal: &Journal) -> Result<(), AuraError> {
        // Persist the journal to storage
        self.journal_handler().persist_journal(journal).await?;

        // Index all facts for efficient lookup (B-tree, Bloom filter, Merkle tree)
        let ts_ms = self.time_handler.current_timestamp().await?;
        let timestamp = aura_core::time::TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
            ts_ms,
            uncertainty: None,
        });
        for (predicate, value) in journal.facts.iter() {
            let predicate_key = predicate.as_str().to_string();
            self.journal.indexed_journal().add_fact(
                predicate_key,
                value.clone(),
                Some(self.authority_id),
                Some(timestamp.clone()),
            );
        }

        Ok(())
    }

    async fn get_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
    ) -> Result<FlowBudget, AuraError> {
        self.journal_handler()
            .get_flow_budget(_context, _peer)
            .await
    }

    async fn update_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
        budget: &FlowBudget,
    ) -> Result<FlowBudget, AuraError> {
        self.journal_handler()
            .update_flow_budget(_context, _peer, budget)
            .await
    }

    async fn charge_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
        _cost: FlowCost,
    ) -> Result<FlowBudget, AuraError> {
        self.journal_handler()
            .charge_flow_budget(_context, _peer, _cost)
            .await
    }
}

// Implementation of IndexedJournalEffects - provides B-tree indexes, Bloom filters, Merkle trees
#[async_trait]
impl IndexedJournalEffects for AuraEffectSystem {
    fn watch_facts(&self) -> Box<dyn indexed::FactStreamReceiver> {
        self.journal.indexed_journal().watch_facts()
    }

    async fn facts_by_predicate(
        &self,
        predicate: &str,
    ) -> Result<Vec<indexed::IndexedFact>, AuraError> {
        self.journal
            .indexed_journal()
            .facts_by_predicate(predicate)
            .await
    }

    async fn facts_by_authority(
        &self,
        authority: &AuthorityId,
    ) -> Result<Vec<indexed::IndexedFact>, AuraError> {
        self.journal
            .indexed_journal()
            .facts_by_authority(authority)
            .await
    }

    async fn facts_in_range(
        &self,
        start: aura_core::time::TimeStamp,
        end: aura_core::time::TimeStamp,
    ) -> Result<Vec<indexed::IndexedFact>, AuraError> {
        self.journal
            .indexed_journal()
            .facts_in_range(start, end)
            .await
    }

    async fn all_facts(&self) -> Result<Vec<indexed::IndexedFact>, AuraError> {
        self.journal.indexed_journal().all_facts().await
    }

    fn might_contain(
        &self,
        predicate: &str,
        value: &aura_core::domain::journal::FactValue,
    ) -> bool {
        self.journal
            .indexed_journal()
            .might_contain(predicate, value)
    }

    async fn merkle_root(&self) -> Result<[u8; 32], AuraError> {
        self.journal.indexed_journal().merkle_root().await
    }

    async fn verify_fact_inclusion(&self, fact: &indexed::IndexedFact) -> Result<bool, AuraError> {
        self.journal
            .indexed_journal()
            .verify_fact_inclusion(fact)
            .await
    }

    async fn get_bloom_filter(&self) -> Result<BloomFilter, AuraError> {
        self.journal.indexed_journal().get_bloom_filter().await
    }

    async fn index_stats(&self) -> Result<indexed::IndexStats, AuraError> {
        self.journal.indexed_journal().index_stats().await
    }
}
