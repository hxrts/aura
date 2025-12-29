//! Fact synchronization protocol using Merkle verification
//!
//! Synchronizes journal facts between peers with cryptographic verification.
//! Uses Merkle trees for integrity checking and Bloom filters for efficient
//! set reconciliation.
//!
//! # Protocol Flow
//!
//! ```text
//! ┌──────────────┐     ┌──────────────┐
//! │ Local Node   │     │ Remote Node  │
//! └──────────────┘     └──────────────┘
//!        │                     │
//!        │ ① Exchange Merkle roots
//!        │◄───────────────────►│
//!        │                     │
//!        │ ② Compare roots
//!        │  - Same → In sync, skip
//!        │  - Different → Need reconciliation
//!        │                     │
//!        │ ③ Exchange Bloom filters
//!        │◄───────────────────►│
//!        │                     │
//!        │ ④ Compute deltas using Bloom
//!        │                     │
//!        │ ⑤ Exchange facts
//!        │◄───────────────────►│
//!        │                     │
//!        │ ⑥ Verify each fact
//!        │  verify_fact_inclusion()
//!        │                     │
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_sync::protocols::fact_sync::{FactSyncConfig, FactSyncProtocol};
//! use aura_core::effects::indexed::IndexedJournalEffects;
//!
//! let config = FactSyncConfig::default();
//! let protocol = FactSyncProtocol::new(config, indexed_journal);
//!
//! let (result, facts_to_send) = protocol
//!     .sync_with_peer(peer_root, &peer_bloom, incoming_facts)
//!     .await?;
//! ```

use crate::verification::{MerkleComparison, MerkleVerifier, VerificationResult};
use aura_core::domain::journal::FactValue;
use aura_core::effects::indexed::{IndexedFact, IndexedJournalEffects};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::BloomFilter;
use aura_core::{hash, AuraError};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for fact sync protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactSyncConfig {
    /// Maximum facts to sync per batch
    pub max_batch_size: u32,
    /// Enable Merkle verification (should always be true in production)
    pub verify_merkle: bool,
    /// Enable Bloom filter optimization
    pub use_bloom_filter: bool,
    /// Skip sync if roots match (optimization)
    pub skip_on_root_match: bool,
}

impl Default for FactSyncConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 1000,
            verify_merkle: true,
            use_bloom_filter: true,
            skip_on_root_match: true,
        }
    }
}

impl FactSyncConfig {
    /// Create config for production use with all verification enabled
    pub fn production() -> Self {
        Self::default()
    }

    /// Create config for testing with smaller batch sizes
    pub fn for_testing() -> Self {
        Self {
            max_batch_size: 100,
            verify_merkle: true,
            use_bloom_filter: true,
            skip_on_root_match: true,
        }
    }

    /// Create config that disables optimizations for debugging
    pub fn debug() -> Self {
        Self {
            max_batch_size: 50,
            verify_merkle: true,
            use_bloom_filter: false,
            skip_on_root_match: false,
        }
    }
}

// =============================================================================
// Result Types
// =============================================================================

/// Result of a fact sync session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactSyncResult {
    /// Number of facts received from peer
    pub facts_received: u64,
    /// Number of facts sent to peer
    pub facts_sent: u64,
    /// Number of facts that passed verification
    pub facts_verified: u64,
    /// Number of facts rejected during verification
    pub facts_rejected: u64,
    /// Whether journals are now in sync
    pub in_sync: bool,
    /// Local Merkle root after sync
    pub local_root: [u8; 32],
    /// Remote Merkle root received
    pub remote_root: [u8; 32],
    /// Whether sync was skipped due to matching roots
    pub skipped: bool,
}

/// Statistics about fact sync operations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FactSyncStats {
    /// Total sync sessions
    pub total_sessions: u64,
    /// Sessions skipped due to matching roots
    pub skipped_sessions: u64,
    /// Total facts received across all sessions
    pub total_facts_received: u64,
    /// Total facts sent across all sessions
    pub total_facts_sent: u64,
    /// Total facts verified
    pub total_verified: u64,
    /// Total facts rejected
    pub total_rejected: u64,
}

// =============================================================================
// FactSyncProtocol
// =============================================================================

/// Fact synchronization protocol handler
///
/// Implements Merkle-verified fact synchronization between peers.
/// Uses the `MerkleVerifier` for cryptographic integrity checking
/// and Bloom filters for efficient set reconciliation.
pub struct FactSyncProtocol {
    /// Protocol configuration
    config: FactSyncConfig,
    /// Merkle verifier for integrity checks
    verifier: MerkleVerifier,
    /// Local indexed journal for fact operations
    indexed_journal: Arc<dyn IndexedJournalEffects + Send + Sync>,
}

impl FactSyncProtocol {
    /// Create a new fact sync protocol with the given configuration, journal, and time effects
    pub fn new(
        config: FactSyncConfig,
        indexed_journal: Arc<dyn IndexedJournalEffects + Send + Sync>,
        time: Arc<dyn PhysicalTimeEffects>,
    ) -> Self {
        let verifier = MerkleVerifier::new(indexed_journal.clone(), time);
        Self {
            config,
            verifier,
            indexed_journal,
        }
    }

    /// Execute fact sync with a peer
    ///
    /// # Protocol Steps
    ///
    /// 1. Compare Merkle roots to detect if sync is needed
    /// 2. If roots differ, verify incoming facts
    /// 3. Compute facts to send to peer (using Bloom filter if enabled)
    /// 4. Return sync result and facts to send
    ///
    /// # Arguments
    ///
    /// * `peer_root` - Merkle root from the peer
    /// * `peer_bloom` - Serialized Bloom filter from peer (empty if not available)
    /// * `incoming_facts` - Facts received from peer to verify
    ///
    /// # Returns
    ///
    /// Tuple of (FactSyncResult, Vec<IndexedFact>) where the second element
    /// is the list of facts that should be sent back to the peer.
    pub async fn sync_with_peer(
        &self,
        peer_root: [u8; 32],
        peer_bloom: &[u8],
        incoming_facts: Vec<IndexedFact>,
    ) -> Result<(FactSyncResult, Vec<IndexedFact>), AuraError> {
        // Step 1: Compare Merkle roots
        let comparison = self.verifier.compare_roots(peer_root).await?;

        match comparison {
            MerkleComparison::InSync if self.config.skip_on_root_match => {
                // Already in sync, nothing to do
                tracing::debug!("Merkle roots match - skipping sync");
                let local_root = self.verifier.local_merkle_root().await?;
                return Ok((
                    FactSyncResult {
                        facts_received: 0,
                        facts_sent: 0,
                        facts_verified: 0,
                        facts_rejected: 0,
                        in_sync: true,
                        local_root,
                        remote_root: peer_root,
                        skipped: true,
                    },
                    vec![],
                ));
            }
            _ => {
                // Need to sync - continue with protocol
            }
        }

        // Step 2: Verify incoming facts if Merkle verification is enabled
        let verification = if self.config.verify_merkle {
            self.verifier
                .verify_incoming_facts(incoming_facts.clone(), peer_root)
                .await?
        } else {
            // Accept all facts without verification (not recommended)
            VerificationResult {
                verified: incoming_facts,
                rejected: vec![],
                merkle_root: self.verifier.local_merkle_root().await?,
            }
        };

        // Step 3: Compute facts to send to peer
        let facts_to_send = self.compute_facts_to_send(peer_bloom).await?;

        let result = FactSyncResult {
            facts_received: (verification.verified.len() + verification.rejected.len()) as u64,
            facts_sent: facts_to_send.len() as u64,
            facts_verified: verification.verified.len() as u64,
            facts_rejected: verification.rejected.len() as u64,
            in_sync: verification.rejected.is_empty(),
            local_root: verification.merkle_root,
            remote_root: peer_root,
            skipped: false,
        };

        tracing::debug!(
            facts_received = result.facts_received,
            facts_verified = result.facts_verified,
            facts_rejected = result.facts_rejected,
            facts_to_send = facts_to_send.len(),
            "Fact sync completed"
        );

        Ok((result, facts_to_send))
    }

    /// Compute which facts should be sent to the peer
    ///
    /// Uses the peer's Bloom filter to avoid sending facts they already have.
    /// Falls back to sending all facts if Bloom filter is not available or
    /// the optimization is disabled.
    async fn compute_facts_to_send(
        &self,
        peer_bloom: &[u8],
    ) -> Result<Vec<IndexedFact>, AuraError> {
        let all_facts = self.indexed_journal.all_facts().await?;

        if self.config.use_bloom_filter && !peer_bloom.is_empty() {
            match aura_core::util::serialization::from_slice::<BloomFilter>(peer_bloom) {
                Ok(filter) => {
                    let filtered: Vec<IndexedFact> = all_facts
                        .into_iter()
                        .filter(|fact| !bloom_might_contain(&filter, fact))
                        .take(self.config.max_batch_size as usize)
                        .collect();

                    return Ok(filtered);
                }
                Err(err) => {
                    tracing::warn!("Failed to deserialize peer bloom filter: {}", err);
                }
            }
        }

        // Apply batch size limit
        let facts: Vec<IndexedFact> = all_facts
            .into_iter()
            .take(self.config.max_batch_size as usize)
            .collect();

        Ok(facts)
    }

    /// Get the internal verifier for direct Merkle operations
    pub fn verifier(&self) -> &MerkleVerifier {
        &self.verifier
    }

    /// Get local Merkle root
    pub async fn local_merkle_root(&self) -> Result<[u8; 32], AuraError> {
        self.verifier.local_merkle_root().await
    }

    /// Get local Bloom filter for exchange with peer
    pub async fn local_bloom_filter(&self) -> Result<Vec<u8>, AuraError> {
        let filter = self.verifier.local_bloom_filter().await?;
        aura_core::util::serialization::to_vec(&filter)
            .map_err(|err| AuraError::serialization(err.to_string()))
    }

    /// Get protocol configuration
    pub fn config(&self) -> &FactSyncConfig {
        &self.config
    }
}

// =============================================================================
// Bloom Filter Helpers
// =============================================================================

fn fact_to_bytes(fact: &IndexedFact) -> Vec<u8> {
    let mut bytes = Vec::new();
    // Include the fact ID for unique identification
    bytes.extend_from_slice(&fact.id.0.to_le_bytes());
    bytes.extend_from_slice(fact.predicate.as_bytes());
    bytes.push(0);
    match &fact.value {
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
        FactValue::Set(set) => {
            bytes.push(3);
            for item in set {
                bytes.extend_from_slice(item.as_bytes());
                bytes.push(0);
            }
        }
        FactValue::Nested(nested) => {
            bytes.push(4);
            if let Ok(serialized) = aura_core::util::serialization::to_vec(nested.as_ref()) {
                let hash = hash::hash(&serialized);
                bytes.extend_from_slice(&hash);
            }
        }
    }
    bytes
}

fn bloom_positions<'a>(
    filter: &'a BloomFilter,
    element: &'a [u8],
) -> impl Iterator<Item = (usize, u8)> + 'a {
    (0..filter.config.num_hash_functions).map(move |i| {
        let mut hasher = hash::hasher();
        hasher.update(&i.to_le_bytes());
        hasher.update(element);
        let hash_bytes = hasher.finalize().to_vec();
        let mut hash_u64_bytes = [0u8; 8];
        hash_u64_bytes.copy_from_slice(&hash_bytes[..8]);
        let hash_value = u64::from_le_bytes(hash_u64_bytes);

        let bit_index = hash_value % filter.config.bit_vector_size;
        let byte_index = (bit_index / 8) as usize;
        let bit_offset = (bit_index % 8) as u8;
        (byte_index, bit_offset)
    })
}

fn bloom_might_contain(filter: &BloomFilter, fact: &IndexedFact) -> bool {
    let element = fact_to_bytes(fact);
    for (byte_index, bit_offset) in bloom_positions(filter, &element) {
        if byte_index >= filter.bits.len() || (filter.bits[byte_index] & (1u8 << bit_offset)) == 0 {
            return false;
        }
    }
    true
}

#[cfg(test)]
fn bloom_insert(filter: &mut BloomFilter, fact: &IndexedFact) {
    let element = fact_to_bytes(fact);
    // Collect positions first to avoid borrow conflict with filter.bits mutation
    let positions: Vec<_> = bloom_positions(filter, &element).collect();
    for (byte_index, bit_offset) in positions {
        if byte_index < filter.bits.len() {
            filter.bits[byte_index] |= 1u8 << bit_offset;
        }
    }
    filter.element_count += 1;
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::domain::journal::FactValue;
    use aura_core::effects::indexed::{FactId, FactStreamReceiver, IndexStats};
    use aura_core::effects::TimeError;
    use aura_core::effects::{BloomConfig, BloomFilter};
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

    #[aura_macros::aura_test]
    async fn test_sync_skipped_when_roots_match() {
        let root = [1u8; 32];
        let journal = Arc::new(MockIndexedJournal::new(root));
        let time = MockTimeEffects::new(TEST_TIME_MS);
        let protocol = FactSyncProtocol::new(FactSyncConfig::default(), journal, time);

        let (result, facts_to_send) = protocol.sync_with_peer(root, &[], vec![]).await.unwrap();

        assert!(result.skipped);
        assert!(result.in_sync);
        assert_eq!(result.facts_received, 0);
        assert!(facts_to_send.is_empty());
    }

    #[aura_macros::aura_test]
    async fn test_sync_with_different_roots() {
        let local_root = [1u8; 32];
        let remote_root = [2u8; 32];
        let local_facts = vec![create_test_fact(1), create_test_fact(2)];
        let journal = Arc::new(MockIndexedJournal::with_facts(
            local_root,
            local_facts.clone(),
        ));
        let time = MockTimeEffects::new(TEST_TIME_MS);
        let protocol = FactSyncProtocol::new(FactSyncConfig::default(), journal, time);

        let incoming = vec![create_test_fact(3)];
        let (result, facts_to_send) = protocol
            .sync_with_peer(remote_root, &[], incoming)
            .await
            .unwrap();

        assert!(!result.skipped);
        assert_eq!(result.facts_received, 1);
        assert_eq!(result.facts_verified, 1);
        assert_eq!(result.facts_rejected, 0);
        assert_eq!(facts_to_send.len(), 2); // All local facts
    }

    #[aura_macros::aura_test]
    async fn test_sync_with_verification_disabled() {
        let local_root = [1u8; 32];
        let remote_root = [2u8; 32];
        let journal = Arc::new(MockIndexedJournal::new(local_root));
        let time = MockTimeEffects::new(TEST_TIME_MS);

        let config = FactSyncConfig {
            verify_merkle: false,
            ..Default::default()
        };
        let protocol = FactSyncProtocol::new(config, journal, time);

        let incoming = vec![create_test_fact(1)];
        let (result, _) = protocol
            .sync_with_peer(remote_root, &[], incoming)
            .await
            .unwrap();

        // Without verification, all facts are accepted
        assert_eq!(result.facts_verified, 1);
        assert_eq!(result.facts_rejected, 0);
    }

    #[aura_macros::aura_test]
    async fn test_batch_size_limit() {
        let root = [1u8; 32];
        let remote_root = [2u8; 32];
        let mut facts = Vec::new();
        for i in 0..100 {
            facts.push(create_test_fact(i));
        }
        let journal = Arc::new(MockIndexedJournal::with_facts(root, facts));
        let time = MockTimeEffects::new(TEST_TIME_MS);

        let config = FactSyncConfig {
            max_batch_size: 10,
            ..Default::default()
        };
        let protocol = FactSyncProtocol::new(config, journal, time);

        let (_, facts_to_send) = protocol
            .sync_with_peer(remote_root, &[], vec![])
            .await
            .unwrap();

        assert_eq!(facts_to_send.len(), 10); // Limited by batch size
    }

    #[aura_macros::aura_test]
    async fn test_bloom_filter_excludes_known_facts() {
        let local_root = [1u8; 32];
        let remote_root = [2u8; 32];
        let local_facts = vec![create_test_fact(1), create_test_fact(2)];
        let journal = Arc::new(MockIndexedJournal::with_facts(
            local_root,
            local_facts.clone(),
        ));
        let time = MockTimeEffects::new(TEST_TIME_MS);
        let protocol = FactSyncProtocol::new(FactSyncConfig::default(), journal, time);

        // Peer already has fact 1
        let mut filter = BloomFilter::new(BloomConfig::for_sync(10)).unwrap();
        bloom_insert(&mut filter, &local_facts[0]);
        let serialized = aura_core::util::serialization::to_vec(&filter).unwrap();

        let (_, facts_to_send) = protocol
            .sync_with_peer(remote_root, &serialized, vec![])
            .await
            .unwrap();

        assert_eq!(facts_to_send.len(), 1);
        assert_eq!(facts_to_send[0].id, local_facts[1].id);
    }

    #[aura_macros::aura_test]
    async fn test_config_presets() {
        let production = FactSyncConfig::production();
        assert!(production.verify_merkle);
        assert!(production.use_bloom_filter);
        assert_eq!(production.max_batch_size, 1000);

        let testing = FactSyncConfig::for_testing();
        assert!(testing.verify_merkle);
        assert_eq!(testing.max_batch_size, 100);

        let debug = FactSyncConfig::debug();
        assert!(debug.verify_merkle);
        assert!(!debug.use_bloom_filter);
        assert!(!debug.skip_on_root_match);
    }
}
