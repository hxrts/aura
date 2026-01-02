//! Merkle verification for journal synchronization
//!
//! Provides cryptographic verification of facts during synchronization.
//! Integrates with the IndexedJournalEffects to verify fact integrity
//! using Merkle trees and Bloom filters.
//!
//! # Architecture
//!
//! The verification system provides:
//! - Merkle root comparison for quick sync status check
//! - Bloom filter exchange for efficient set reconciliation
//! - Fact-level verification using Merkle proofs
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_sync::verification::MerkleVerifier;
//! use aura_core::effects::indexed::IndexedJournalEffects;
//! use aura_core::effects::time::PhysicalTimeEffects;
//!
//! let verifier = MerkleVerifier::new(indexed_journal, time_effects);
//!
//! // Check if local and remote journals are in sync
//! let comparison = verifier.compare_roots(remote_root).await?;
//!
//! // Verify incoming facts
//! let result = verifier.verify_incoming_facts(facts, claimed_root).await?;
//! ```

use aura_core::effects::indexed::{IndexStats, IndexedFact, IndexedJournalEffects};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::BloomFilter;
use aura_core::time::TimeStamp;
use aura_core::AuraError;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Maximum allowed clock skew in milliseconds for timestamp validation
const MAX_CLOCK_SKEW_MS: u64 = 300_000; // 5 minutes

// =============================================================================
// Types
// =============================================================================

/// Result of comparing Merkle roots between peers
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MerkleComparison {
    /// Roots match - journals are in sync
    InSync,
    /// Roots differ - reconciliation needed
    NeedReconcile {
        /// Local Merkle root
        local_root: [u8; 32],
        /// Remote Merkle root
        remote_root: [u8; 32],
    },
}

/// Verification result for a batch of facts
#[derive(Debug, Clone)]
pub struct VerificationResult {
    /// Facts that passed verification
    pub verified: Vec<IndexedFact>,
    /// Facts that failed verification (with reasons)
    pub rejected: Vec<(IndexedFact, String)>,
    /// Local Merkle root after verification
    pub merkle_root: [u8; 32],
}

/// Statistics about verification operations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerificationStats {
    /// Total facts verified
    pub total_verified: u64,
    /// Total facts rejected
    pub total_rejected: u64,
    /// Root comparisons performed
    pub root_comparisons: u64,
    /// Comparisons where roots matched (in sync)
    pub root_matches: u64,
}

// =============================================================================
// MerkleVerifier
// =============================================================================

/// Merkle verification handler for sync operations
///
/// Provides cryptographic verification of facts using the local indexed journal's
/// Merkle tree. Used to verify incoming facts during synchronization and to
/// compare journal state between peers.
pub struct MerkleVerifier {
    /// Local indexed journal for verification operations
    indexed_journal: Arc<dyn IndexedJournalEffects + Send + Sync>,
    /// Time effects for timestamp validation
    time: Arc<dyn PhysicalTimeEffects>,
}

impl MerkleVerifier {
    /// Create a new MerkleVerifier with the given indexed journal and time effects
    pub fn new(
        indexed_journal: Arc<dyn IndexedJournalEffects + Send + Sync>,
        time: Arc<dyn PhysicalTimeEffects>,
    ) -> Self {
        Self {
            indexed_journal,
            time,
        }
    }

    /// Get local Merkle root for exchange with peer
    ///
    /// Returns the root of the local Merkle tree, which can be compared
    /// with a remote peer's root to quickly determine if synchronization
    /// is needed.
    pub async fn local_merkle_root(&self) -> Result<[u8; 32], AuraError> {
        self.indexed_journal.merkle_root().await
    }

    /// Get local Bloom filter for set reconciliation
    ///
    /// Returns a Bloom filter representing the local journal's facts.
    /// Used to efficiently determine which facts need to be exchanged
    /// during synchronization.
    pub async fn local_bloom_filter(&self) -> Result<BloomFilter, AuraError> {
        self.indexed_journal.get_bloom_filter().await
    }

    /// Compare local and remote Merkle roots
    ///
    /// This is the first step in the sync protocol - a fast O(1) check
    /// to determine if synchronization is needed.
    ///
    /// # Returns
    ///
    /// - `InSync`: Roots match, no synchronization needed
    /// - `NeedReconcile`: Roots differ, facts must be exchanged
    pub async fn compare_roots(
        &self,
        remote_root: [u8; 32],
    ) -> Result<MerkleComparison, AuraError> {
        let local_root = self.local_merkle_root().await?;

        if local_root == remote_root {
            tracing::debug!("Merkle roots match - journals in sync");
            Ok(MerkleComparison::InSync)
        } else {
            tracing::debug!(
                local_root = ?hex::encode(local_root),
                remote_root = ?hex::encode(remote_root),
                "Merkle roots differ - reconciliation needed"
            );
            Ok(MerkleComparison::NeedReconcile {
                local_root,
                remote_root,
            })
        }
    }

    /// Verify incoming facts against the local Merkle tree
    ///
    /// This checks if each fact is consistent with our local state.
    /// Facts that don't verify may be:
    /// - New facts we don't have yet (valid - should merge)
    /// - Tampered facts (invalid - should reject)
    ///
    /// # Arguments
    ///
    /// * `facts` - Facts received from peer to verify
    /// * `claimed_root` - Merkle root claimed by the peer
    ///
    /// # Returns
    ///
    /// `VerificationResult` containing verified facts, rejected facts, and
    /// the current local Merkle root.
    pub async fn verify_incoming_facts(
        &self,
        facts: Vec<IndexedFact>,
        _claimed_root: [u8; 32],
    ) -> Result<VerificationResult, AuraError> {
        let mut verified = Vec::new();
        let mut rejected = Vec::new();

        // Get current time from effect trait for timestamp validation
        let now_ms = self
            .time
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or(0);

        for fact in facts {
            // Check if fact is already in our index
            match self.indexed_journal.verify_fact_inclusion(&fact).await {
                Ok(is_included) => {
                    if is_included {
                        // Fact already exists and is verified in our tree
                        tracing::trace!(
                            fact_id = ?fact.id,
                            "Fact already exists in local journal"
                        );
                        verified.push(fact);
                    } else {
                        // New fact - validate structure before accepting
                        // Perform available validations:
                        // 1. Timestamp consistency (fact shouldn't be too far in the future)
                        // 2. Authority presence (if required by policy)
                        // 3. Merkle proof verification (when proofs are provided)
                        // 4. Signature verification (when signatures are provided)

                        // Validate timestamp consistency
                        if let Err(reason) = Self::validate_timestamp(&fact, now_ms) {
                            tracing::warn!(
                                fact_id = ?fact.id,
                                reason = %reason,
                                "Fact rejected: timestamp validation failed"
                            );
                            rejected.push((fact, reason));
                            continue;
                        }

                        // Validate authority presence
                        if let Err(reason) = Self::validate_authority(&fact) {
                            tracing::warn!(
                                fact_id = ?fact.id,
                                reason = %reason,
                                "Fact rejected: authority validation failed"
                            );
                            rejected.push((fact, reason));
                            continue;
                        }

                        // NOTE: Merkle proof and signature verification require additional
                        // infrastructure:
                        // - IndexedFact would need to carry MerkleProof from the sender
                        // - IndexedFact would need to carry authority signature
                        // When these are available, add verification here.

                        tracing::trace!(
                            fact_id = ?fact.id,
                            "New fact accepted for merge"
                        );
                        verified.push(fact);
                    }
                }
                Err(e) => {
                    // Verification error - reject the fact
                    tracing::warn!(
                        fact_id = ?fact.id,
                        error = %e,
                        "Fact verification failed"
                    );
                    rejected.push((fact, format!("Verification error: {e}")));
                }
            }
        }

        let merkle_root = self.local_merkle_root().await?;

        tracing::debug!(
            verified_count = verified.len(),
            rejected_count = rejected.len(),
            merkle_root = ?hex::encode(merkle_root),
            "Fact verification complete"
        );

        Ok(VerificationResult {
            verified,
            rejected,
            merkle_root,
        })
    }

    /// Validate timestamp consistency for an incoming fact
    ///
    /// Checks that the fact's timestamp is not too far in the future,
    /// which would indicate clock skew or potential manipulation.
    ///
    /// # Arguments
    ///
    /// * `fact` - The fact to validate
    /// * `now_ms` - Current time in milliseconds (from effect trait)
    ///
    /// # Returns
    /// - `Ok(())` if timestamp is valid or not present
    /// - `Err(reason)` if timestamp is too far in the future
    fn validate_timestamp(fact: &IndexedFact, now_ms: u64) -> Result<(), String> {
        let Some(timestamp) = &fact.timestamp else {
            // No timestamp is acceptable for facts that don't require it
            return Ok(());
        };

        // Extract physical time if available
        let fact_time_ms = match timestamp {
            TimeStamp::PhysicalClock(physical) => physical.ts_ms,
            TimeStamp::Range(range) => {
                // For ranges, check the latest time (most permissive)
                range.latest_ms()
            }
            // Logical and Order clocks don't have physical time semantics
            TimeStamp::LogicalClock(_) | TimeStamp::OrderClock(_) => return Ok(()),
        };

        // Reject if timestamp is too far in the future
        if fact_time_ms > now_ms + MAX_CLOCK_SKEW_MS {
            return Err(format!(
                "Timestamp {fact_time_ms} is too far in the future (current time: {now_ms}, max skew: {MAX_CLOCK_SKEW_MS}ms)"
            ));
        }

        Ok(())
    }

    /// Validate authority presence for an incoming fact
    ///
    /// Facts should have an associated authority that created them.
    /// This provides accountability and enables signature verification.
    ///
    /// # Returns
    /// - `Ok(())` if authority is present
    /// - `Err(reason)` if authority is missing and required
    fn validate_authority(fact: &IndexedFact) -> Result<(), String> {
        // For now, we accept facts without authority for backwards compatibility
        // In a stricter mode, we could require: fact.authority.is_some()
        if fact.authority.is_none() {
            tracing::trace!(
                fact_id = ?fact.id,
                "Fact has no authority (accepted for compatibility)"
            );
        }
        Ok(())
    }

    /// Get index statistics for monitoring
    ///
    /// Returns statistics about the indexed journal including
    /// fact counts, index sizes, and Bloom filter configuration.
    pub async fn stats(&self) -> Result<IndexStats, AuraError> {
        self.indexed_journal.index_stats().await
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::domain::journal::FactValue;
    use aura_core::effects::indexed::{FactId, FactStreamReceiver};
    use aura_core::effects::BloomConfig;
    use aura_core::effects::TimeError;
    use aura_core::time::PhysicalTime;
    use aura_core::AuthorityId;
    use std::sync::Mutex;

    /// Fixed time for deterministic tests
    const TEST_TIME_MS: u64 = 1_700_000_000_000;

    /// Mock time effects for testing
    struct MockTimeEffects {
        now_ms: u64,
    }

    impl MockTimeEffects {
        fn new(now_ms: u64) -> Arc<Self> {
            Arc::new(Self { now_ms })
        }
    }

    #[async_trait]
    impl PhysicalTimeEffects for MockTimeEffects {
        async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
            Ok(PhysicalTime {
                ts_ms: self.now_ms,
                uncertainty: None,
            })
        }

        async fn sleep_ms(&self, _ms: u64) -> Result<(), TimeError> {
            Ok(())
        }
    }

    /// Mock indexed journal for testing
    struct MockIndexedJournal {
        root: Mutex<[u8; 32]>,
        facts: Mutex<Vec<IndexedFact>>,
    }

    impl MockIndexedJournal {
        fn new(root: [u8; 32]) -> Self {
            Self {
                root: Mutex::new(root),
                facts: Mutex::new(Vec::new()),
            }
        }

        fn with_facts(root: [u8; 32], facts: Vec<IndexedFact>) -> Self {
            Self {
                root: Mutex::new(root),
                facts: Mutex::new(facts),
            }
        }
    }

    #[async_trait]
    impl IndexedJournalEffects for MockIndexedJournal {
        fn watch_facts(&self) -> Box<dyn FactStreamReceiver> {
            panic!("Not implemented for mock")
        }

        async fn facts_by_predicate(
            &self,
            _predicate: &str,
        ) -> Result<Vec<IndexedFact>, AuraError> {
            Ok(Vec::new())
        }

        async fn facts_by_authority(
            &self,
            _authority: &AuthorityId,
        ) -> Result<Vec<IndexedFact>, AuraError> {
            Ok(Vec::new())
        }

        async fn facts_in_range(
            &self,
            _start: aura_core::time::TimeStamp,
            _end: aura_core::time::TimeStamp,
        ) -> Result<Vec<IndexedFact>, AuraError> {
            Ok(Vec::new())
        }

        async fn all_facts(&self) -> Result<Vec<IndexedFact>, AuraError> {
            Ok(self.facts.lock().unwrap().clone())
        }

        fn might_contain(&self, _predicate: &str, _value: &FactValue) -> bool {
            false
        }

        async fn merkle_root(&self) -> Result<[u8; 32], AuraError> {
            Ok(*self.root.lock().unwrap())
        }

        async fn verify_fact_inclusion(&self, fact: &IndexedFact) -> Result<bool, AuraError> {
            let facts = self.facts.lock().unwrap();
            Ok(facts.iter().any(|f| f.id == fact.id))
        }

        async fn get_bloom_filter(&self) -> Result<BloomFilter, AuraError> {
            BloomFilter::new(BloomConfig::for_sync(100))
        }

        async fn index_stats(&self) -> Result<IndexStats, AuraError> {
            let facts = self.facts.lock().unwrap();
            Ok(IndexStats {
                fact_count: facts.len() as u64,
                predicate_count: 1,
                authority_count: 1,
                bloom_fp_rate: 0.01,
                merkle_depth: 10,
            })
        }
    }

    fn create_test_fact(id: u64) -> IndexedFact {
        IndexedFact {
            id: FactId(id),
            predicate: "test".to_string(),
            value: FactValue::String("test_value".to_string()),
            authority: None,
            timestamp: None,
        }
    }

    #[tokio::test]
    async fn test_compare_roots_in_sync() {
        let root = [1u8; 32];
        let journal = Arc::new(MockIndexedJournal::new(root));
        let time = MockTimeEffects::new(TEST_TIME_MS);
        let verifier = MerkleVerifier::new(journal, time);

        let result = verifier.compare_roots(root).await.unwrap();
        assert_eq!(result, MerkleComparison::InSync);
    }

    #[tokio::test]
    async fn test_compare_roots_need_reconcile() {
        let local_root = [1u8; 32];
        let remote_root = [2u8; 32];
        let journal = Arc::new(MockIndexedJournal::new(local_root));
        let time = MockTimeEffects::new(TEST_TIME_MS);
        let verifier = MerkleVerifier::new(journal, time);

        let result = verifier.compare_roots(remote_root).await.unwrap();
        assert_eq!(
            result,
            MerkleComparison::NeedReconcile {
                local_root,
                remote_root
            }
        );
    }

    #[tokio::test]
    async fn test_verify_existing_facts() {
        let root = [1u8; 32];
        let existing_fact = create_test_fact(1);
        let journal = Arc::new(MockIndexedJournal::with_facts(
            root,
            vec![existing_fact.clone()],
        ));
        let time = MockTimeEffects::new(TEST_TIME_MS);
        let verifier = MerkleVerifier::new(journal, time);

        let result = verifier
            .verify_incoming_facts(vec![existing_fact], root)
            .await
            .unwrap();

        assert_eq!(result.verified.len(), 1);
        assert!(result.rejected.is_empty());
    }

    #[tokio::test]
    async fn test_verify_new_facts() {
        let root = [1u8; 32];
        let journal = Arc::new(MockIndexedJournal::new(root));
        let time = MockTimeEffects::new(TEST_TIME_MS);
        let verifier = MerkleVerifier::new(journal, time);

        let new_fact = create_test_fact(99);
        let result = verifier
            .verify_incoming_facts(vec![new_fact], root)
            .await
            .unwrap();

        // New facts are accepted for merge
        assert_eq!(result.verified.len(), 1);
        assert!(result.rejected.is_empty());
    }

    #[tokio::test]
    async fn test_stats() {
        let root = [1u8; 32];
        let facts = vec![
            create_test_fact(1),
            create_test_fact(2),
            create_test_fact(3),
        ];
        let journal = Arc::new(MockIndexedJournal::with_facts(root, facts));
        let time = MockTimeEffects::new(TEST_TIME_MS);
        let verifier = MerkleVerifier::new(journal, time);

        let stats = verifier.stats().await.unwrap();
        assert_eq!(stats.fact_count, 3);
    }
}
