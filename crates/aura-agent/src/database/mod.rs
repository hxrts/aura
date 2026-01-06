//! Indexed Journal Handler - B-tree indexes, Bloom filters, and Merkle trees
//!
//! # Blocking Lock Usage
//!
//! This module uses `parking_lot::RwLock` for synchronous interior mutability.
//! This is appropriate because:
//!
//! 1. All lock-protected operations are O(log n) or O(1) - sub-millisecond
//! 2. Locks are never held across `.await` points
//! 3. Lock poisoning is not applicable with parking_lot
//!
//! # Scale Expectations
//!
//! Designed for 10k-100k facts per index. If scaling beyond this, consider:
//! - Migrating to `tokio::sync::RwLock` for async-safe access
//! - Using a dedicated indexing thread with channels
//! - Switching to lock-free concurrent data structures (DashMap)

#![allow(clippy::disallowed_types)]
// Note: Module-level allow covers handler.rs, wrapper.rs, and test code
//!
//! # Effect Classification
//!
//! - **Category**: Runtime-owned state (Layer 6)
//! - **Trait**: `IndexedJournalEffects` from `aura-core`
//! - **Usage**: Efficient indexed lookups for facts beyond linear scan
//!
//! This module provides production-grade indexed journal lookups with:
//! - B-tree indexes for O(log n) predicate/authority/time queries
//! - Bloom filters for O(1) membership testing with <1% false positive rate
//! - Merkle trees for cryptographic integrity verification
//!
//! # Module Structure
//!
//! - [`stream`]: Tokio-specific fact stream receiver
//! - [`time_key`]: Timestamp ordering utilities for B-tree indexing
//! - [`authority_index`]: B-tree index for efficient fact lookups
//! - [`merkle`]: Merkle tree construction for cryptographic integrity
//! - [`handler`]: Production indexed journal handler
//! - [`wrapper`]: Combines JournalEffects with IndexedJournalEffects
//!
//! # Notes
//!
//! The stateless Datalog query wrapper (`AuraQuery`) lives in `aura-effects`.

mod authority_index;
mod handler;
mod merkle;
mod stream;
mod time_key;
mod wrapper;

// Public exports
pub use handler::IndexedJournalHandler;
pub use stream::TokioFactStreamReceiver;
pub use wrapper::IndexedJournalWrapper;

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::{
        domain::journal::FactValue,
        effects::indexed::{FactId, IndexedFact},
        effects::{IndexedJournalEffects, JournalEffects},
        time::{PhysicalTime, TimeStamp},
        types::identifiers::AuthorityId,
        AuraError,
    };
    use parking_lot::RwLock;

    #[test]
    fn test_lock_type_is_parking_lot() {
        let name = std::any::type_name::<RwLock<authority_index::AuthorityIndex>>();
        assert!(
            name.contains("parking_lot"),
            "Expected parking_lot::RwLock, got {name}"
        );
    }

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
            target: aura_core::Journal,
            delta: aura_core::Journal,
        ) -> Result<aura_core::Journal, AuraError> {
            // Simple merge: combine facts from both journals
            let mut result = target;
            for (key, value) in delta.facts.iter() {
                result.facts.insert(key.clone(), value.clone())?;
            }
            Ok(result)
        }

        async fn refine_caps(
            &self,
            target: aura_core::Journal,
            _refinement: aura_core::Journal,
        ) -> Result<aura_core::Journal, AuraError> {
            Ok(target)
        }

        async fn get_journal(&self) -> Result<aura_core::Journal, AuraError> {
            let journal = self.journal.read();
            Ok(journal.clone())
        }

        async fn persist_journal(&self, journal: &aura_core::Journal) -> Result<(), AuraError> {
            let mut stored = self.journal.write();
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
            _cost: aura_core::FlowCost,
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
        journal
            .facts
            .insert(
                "user.name".to_string(),
                FactValue::String("alice".to_string()),
            )
            .expect("insert fact should succeed");
        journal
            .facts
            .insert(
                "user.email".to_string(),
                FactValue::String("alice@example.com".to_string()),
            )
            .expect("insert fact should succeed");

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
        delta
            .facts
            .insert(
                "event.type".to_string(),
                FactValue::String("login".to_string()),
            )
            .expect("insert fact should succeed");
        delta
            .facts
            .insert("event.timestamp".to_string(), FactValue::Number(1234567890))
            .expect("insert fact should succeed");

        // Merge facts - should trigger indexing of delta
        let _result = wrapper.merge_facts(target, delta).await.unwrap();

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
            let mut stored = mock_journal.journal.write();
            stored
                .facts
                .insert(
                    "stored.key".to_string(),
                    FactValue::String("stored_value".to_string()),
                )
                .expect("insert fact should succeed");
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
        journal
            .facts
            .insert(
                "test.key".to_string(),
                FactValue::String("test_value".to_string()),
            )
            .expect("insert fact should succeed");
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
            .insert("fact1".to_string(), FactValue::Number(1))
            .expect("insert fact should succeed");
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
        journal
            .facts
            .insert(
                "unique.key".to_string(),
                FactValue::String("value".to_string()),
            )
            .expect("insert fact should succeed");

        // Persist multiple times
        wrapper.persist_journal(&journal).await.unwrap();
        wrapper.persist_journal(&journal).await.unwrap();
        wrapper.persist_journal(&journal).await.unwrap();

        // Should only have one fact indexed (no duplicates)
        let stats = wrapper.index_stats().await.unwrap();
        assert_eq!(stats.fact_count, 1);
    }
}
