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
//! let result = verifier
//!     .verify_incoming_facts(facts, claimed_root, context_id)
//!     .await?;
//! ```

use crate::capabilities::SyncCapability;
use aura_core::crypto::{ed25519_verify, Ed25519Signature, Ed25519VerifyingKey, SimpleMerkleProof};
use aura_core::effects::indexed::{IndexStats, IndexedFact, IndexedJournalEffects};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::BloomFilter;
use aura_core::hash::hash;
use aura_core::time::TimeStamp;
use aura_core::util::serialization;
use aura_core::{verify_merkle_proof, AuraError, AuthorityId, ContextId};
use aura_signature::{encode_transcript, SecurityTranscript};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Maximum allowed clock skew in milliseconds for timestamp validation
const MAX_CLOCK_SKEW_MS: u64 = 300_000; // 5 minutes
const SYNC_FACT_SCHEMA_VERSION: u16 = 1;

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

/// Namespace carried by peer fact envelopes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum FactEnvelopeNamespace {
    Context(ContextId),
}

/// Signer metadata required to authenticate a peer fact envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FactEnvelopeSigner {
    /// Authority claimed for the signer.
    pub authority: AuthorityId,
    /// Ed25519 public key bytes for the signer.
    pub public_key: Vec<u8>,
    /// Key identifier within the signer namespace.
    pub key_id: String,
    /// Optional certificate bytes carried with the signer metadata.
    pub certificate: Option<Vec<u8>>,
}

/// Peer-supplied fact envelope containing proof and signer evidence.
#[derive(Debug, Clone)]
pub struct FactEnvelope {
    /// The fact being synchronized.
    pub fact: IndexedFact,
    /// Merkle proof showing inclusion under the claimed peer root.
    pub merkle_proof: SimpleMerkleProof,
    /// Namespace scoped by the sync session.
    pub namespace: FactEnvelopeNamespace,
    /// Schema version for the envelope.
    pub schema_version: u16,
    /// Signer metadata carried with the envelope.
    pub signer: FactEnvelopeSigner,
    /// Signature over the canonical envelope transcript.
    pub signature: Vec<u8>,
    /// Capability asserted for pushing this fact batch.
    pub capability: String,
}

#[derive(Debug, Clone, Serialize)]
struct FactEnvelopeTranscriptPayload {
    namespace: FactEnvelopeNamespace,
    schema_version: u16,
    claimed_root: [u8; 32],
    fact_hash: [u8; 32],
    proof_hash: [u8; 32],
    signer_authority: AuthorityId,
    signer_public_key: Vec<u8>,
    signer_key_id: String,
    capability: String,
}

struct FactEnvelopeTranscript<'a> {
    envelope: &'a FactEnvelope,
    claimed_root: [u8; 32],
}

impl SecurityTranscript for FactEnvelopeTranscript<'_> {
    type Payload = FactEnvelopeTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.sync.fact-envelope";

    fn transcript_payload(&self) -> Self::Payload {
        let fact_hash = hash(&fact_to_bytes(&self.envelope.fact));
        let proof_bytes = match serialization::to_vec(&self.envelope.merkle_proof) {
            Ok(bytes) => bytes,
            Err(error) => panic!("fact envelope proof should serialize: {error}"),
        };
        let proof_hash = hash(&proof_bytes);
        FactEnvelopeTranscriptPayload {
            namespace: self.envelope.namespace.clone(),
            schema_version: self.envelope.schema_version,
            claimed_root: self.claimed_root,
            fact_hash,
            proof_hash,
            signer_authority: self.envelope.signer.authority,
            signer_public_key: self.envelope.signer.public_key.clone(),
            signer_key_id: self.envelope.signer.key_id.clone(),
            capability: self.envelope.capability.clone(),
        }
    }
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
    /// * `context_id` - Namespace context authorized for this sync session
    ///
    /// # Returns
    ///
    /// `VerificationResult` containing verified facts, rejected facts, and
    /// the current local Merkle root.
    pub async fn verify_incoming_facts(
        &self,
        facts: Vec<FactEnvelope>,
        claimed_root: [u8; 32],
        context_id: ContextId,
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

        for envelope in facts {
            let fact = envelope.fact.clone();
            if let Err(reason) = Self::validate_envelope(&envelope, claimed_root, context_id) {
                tracing::warn!(
                    fact_id = ?fact.id,
                    reason = %reason,
                    "Fact rejected: envelope validation failed"
                );
                rejected.push((fact, reason));
                continue;
            }

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
                        if let Err(reason) = Self::validate_authority(&fact, &envelope) {
                            tracing::warn!(
                                fact_id = ?fact.id,
                                reason = %reason,
                                "Fact rejected: authority validation failed"
                            );
                            rejected.push((fact, reason));
                            continue;
                        }

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
    /// Facts must have an associated authority that created them.
    /// This provides accountability and enables signature verification.
    ///
    /// # Returns
    /// - `Ok(())` if authority is present
    /// - `Err(reason)` if authority is missing
    fn validate_authority(fact: &IndexedFact, envelope: &FactEnvelope) -> Result<(), String> {
        if fact.authority.is_none() {
            tracing::warn!(
                fact_id = ?fact.id,
                "Rejecting fact without authority - all facts must have an associated authority"
            );
            return Err(format!(
                "Fact {:?} rejected: authority is required for all facts",
                fact.id
            ));
        }
        if fact.authority != Some(envelope.signer.authority) {
            return Err(format!(
                "Fact {:?} signer authority does not match fact authority",
                fact.id
            ));
        }
        Ok(())
    }

    fn validate_envelope(
        envelope: &FactEnvelope,
        claimed_root: [u8; 32],
        context_id: ContextId,
    ) -> Result<(), String> {
        if envelope.schema_version != SYNC_FACT_SCHEMA_VERSION {
            return Err(format!(
                "unsupported fact envelope schema version {}",
                envelope.schema_version
            ));
        }

        if envelope.capability != SyncCapability::PushOps.as_name().as_str() {
            return Err(format!(
                "fact envelope capability '{}' is not authorized for fact sync",
                envelope.capability
            ));
        }

        if envelope.signer.key_id.trim().is_empty() {
            return Err("fact envelope signer metadata is missing key_id".to_string());
        }

        if envelope.signature.is_empty() {
            return Err("fact envelope is missing signature bytes".to_string());
        }

        let expected_namespace = FactEnvelopeNamespace::Context(context_id);
        if envelope.namespace != expected_namespace {
            return Err(format!(
                "fact envelope namespace {:?} does not match authorized sync namespace {:?}",
                envelope.namespace, expected_namespace
            ));
        }

        let public_key: [u8; 32] = envelope
            .signer
            .public_key
            .as_slice()
            .try_into()
            .map_err(|_| "fact envelope signer public key must be 32 bytes".to_string())?;
        let authority_from_key = AuthorityId::new_from_entropy(hash(&public_key));
        if authority_from_key != envelope.signer.authority {
            return Err("fact envelope signer public key does not bind to authority".to_string());
        }

        let leaf_value = fact_to_bytes(&envelope.fact);
        if !verify_merkle_proof(&envelope.merkle_proof, &claimed_root, &leaf_value) {
            return Err("fact envelope Merkle proof does not match claimed root".to_string());
        }

        let transcript_bytes = fact_envelope_transcript_bytes(envelope, claimed_root)
            .map_err(|error| format!("encode fact envelope transcript: {error}"))?;
        let signature = Ed25519Signature::try_from_slice(&envelope.signature)
            .map_err(|error| format!("invalid fact envelope signature: {error}"))?;
        let verifying_key = Ed25519VerifyingKey(public_key);
        let signature_ok = ed25519_verify(&transcript_bytes, &signature, &verifying_key)
            .map_err(|error| format!("verify fact envelope signature: {error}"))?;
        if !signature_ok {
            return Err("fact envelope signature verification failed".to_string());
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

fn fact_to_bytes(fact: &IndexedFact) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&fact.id.0.to_le_bytes());
    bytes.extend_from_slice(fact.predicate.as_bytes());
    bytes.push(0);
    match &fact.value {
        aura_core::domain::journal::FactValue::String(s) => {
            bytes.push(0);
            bytes.extend_from_slice(s.as_bytes());
        }
        aura_core::domain::journal::FactValue::Number(n) => {
            bytes.push(1);
            bytes.extend_from_slice(&n.to_le_bytes());
        }
        aura_core::domain::journal::FactValue::Bytes(b) => {
            bytes.push(2);
            bytes.extend_from_slice(b);
        }
        aura_core::domain::journal::FactValue::Set(set) => {
            bytes.push(3);
            for item in set {
                bytes.extend_from_slice(item.as_bytes());
                bytes.push(0);
            }
        }
        aura_core::domain::journal::FactValue::Nested(nested) => {
            bytes.push(4);
            if let Ok(serialized) = serialization::to_vec(nested.as_ref()) {
                let nested_hash = hash(&serialized);
                bytes.extend_from_slice(&nested_hash);
            }
        }
    }
    bytes
}

pub(crate) fn fact_envelope_transcript_bytes(
    envelope: &FactEnvelope,
    claimed_root: [u8; 32],
) -> Result<Vec<u8>, AuraError> {
    let transcript = FactEnvelopeTranscript {
        envelope,
        claimed_root,
    };
    encode_transcript(
        FactEnvelopeTranscript::DOMAIN_SEPARATOR,
        FactEnvelopeTranscript::SCHEMA_VERSION,
        &transcript.transcript_payload(),
    )
    .map_err(|error| AuraError::invalid(format!("encode fact envelope transcript: {error}")))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::crypto::{build_merkle_root, generate_merkle_proof};
    use aura_core::domain::journal::FactValue;
    use aura_core::effects::indexed::{FactId, FactStreamReceiver};
    use aura_core::effects::BloomConfig;
    use aura_core::effects::CryptoCoreEffects;
    use aura_core::effects::TimeError;
    use aura_core::time::PhysicalTime;
    use aura_core::{hash::hash, AuthorityId, ContextId};
    use aura_effects::RealCryptoHandler;
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

    struct EmptyFactStreamReceiver;

    impl FactStreamReceiver for EmptyFactStreamReceiver {
        fn recv(
            &mut self,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<IndexedFact>, AuraError>> + Send + '_>,
        > {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn try_recv(&mut self) -> Result<Option<Vec<IndexedFact>>, AuraError> {
            Ok(None)
        }
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
            Box::new(EmptyFactStreamReceiver)
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
            authority: Some(AuthorityId::new_from_entropy([id as u8; 32])),
            timestamp: None,
        }
    }

    async fn signed_fact_envelopes(
        context_id: ContextId,
        facts: Vec<IndexedFact>,
    ) -> ([u8; 32], Vec<FactEnvelope>) {
        let crypto = RealCryptoHandler::for_simulation_seed([0x5Au8; 32]);
        let (private_key, public_key) = crypto.ed25519_generate_keypair().await.unwrap();
        let signer_authority = AuthorityId::new_from_entropy(hash(&public_key));
        let facts: Vec<IndexedFact> = facts
            .into_iter()
            .map(|mut fact| {
                fact.authority = Some(signer_authority);
                fact
            })
            .collect();
        let leaves: Vec<Vec<u8>> = facts.iter().map(fact_to_bytes).collect();
        let claimed_root = build_merkle_root(&leaves);
        let mut envelopes = Vec::with_capacity(facts.len());
        for (index, fact) in facts.into_iter().enumerate() {
            let merkle_proof = generate_merkle_proof(&leaves, index).unwrap();
            let mut envelope = FactEnvelope {
                fact,
                merkle_proof,
                namespace: FactEnvelopeNamespace::Context(context_id),
                schema_version: SYNC_FACT_SCHEMA_VERSION,
                signer: FactEnvelopeSigner {
                    authority: signer_authority,
                    public_key: public_key.clone(),
                    key_id: format!("device-{index}"),
                    certificate: None,
                },
                signature: Vec::new(),
                capability: SyncCapability::PushOps.as_name().to_string(),
            };
            let transcript_bytes = fact_envelope_transcript_bytes(&envelope, claimed_root).unwrap();
            envelope.signature = crypto
                .ed25519_sign(&transcript_bytes, &private_key)
                .await
                .unwrap();
            envelopes.push(envelope);
        }
        (claimed_root, envelopes)
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
        let existing_fact = create_test_fact(1);
        let context_id = ContextId::new_from_entropy([0x11; 32]);
        let (claimed_root, envelopes) =
            signed_fact_envelopes(context_id, vec![existing_fact.clone()]).await;
        let journal = Arc::new(MockIndexedJournal::with_facts(
            claimed_root,
            vec![existing_fact.clone()],
        ));
        let time = MockTimeEffects::new(TEST_TIME_MS);
        let verifier = MerkleVerifier::new(journal, time);

        let result = verifier
            .verify_incoming_facts(envelopes, claimed_root, context_id)
            .await
            .unwrap();

        assert_eq!(result.verified.len(), 1);
        assert!(result.rejected.is_empty());
    }

    #[tokio::test]
    async fn test_verify_new_facts() {
        let journal = Arc::new(MockIndexedJournal::new([1u8; 32]));
        let time = MockTimeEffects::new(TEST_TIME_MS);
        let verifier = MerkleVerifier::new(journal, time);
        let context_id = ContextId::new_from_entropy([0x22; 32]);
        let new_fact = create_test_fact(99);
        let (claimed_root, envelopes) = signed_fact_envelopes(context_id, vec![new_fact]).await;
        let result = verifier
            .verify_incoming_facts(envelopes, claimed_root, context_id)
            .await
            .unwrap();

        assert_eq!(result.verified.len(), 1);
        assert!(result.rejected.is_empty());
    }

    #[tokio::test]
    async fn test_verify_new_facts_rejects_wrong_root_proof() {
        let journal = Arc::new(MockIndexedJournal::new([1u8; 32]));
        let time = MockTimeEffects::new(TEST_TIME_MS);
        let verifier = MerkleVerifier::new(journal, time);
        let context_id = ContextId::new_from_entropy([0x23; 32]);
        let (_claimed_root, envelopes) =
            signed_fact_envelopes(context_id, vec![create_test_fact(100)]).await;
        let wrong_root = [0xFF; 32];

        let result = verifier
            .verify_incoming_facts(envelopes, wrong_root, context_id)
            .await
            .unwrap();

        assert!(result.verified.is_empty());
        assert_eq!(result.rejected.len(), 1);
    }

    #[tokio::test]
    async fn test_verify_new_facts_rejects_missing_signature() {
        let journal = Arc::new(MockIndexedJournal::new([1u8; 32]));
        let time = MockTimeEffects::new(TEST_TIME_MS);
        let verifier = MerkleVerifier::new(journal, time);
        let context_id = ContextId::new_from_entropy([0x24; 32]);
        let (claimed_root, mut envelopes) =
            signed_fact_envelopes(context_id, vec![create_test_fact(101)]).await;
        envelopes[0].signature.clear();

        let result = verifier
            .verify_incoming_facts(envelopes, claimed_root, context_id)
            .await
            .unwrap();

        assert!(result.verified.is_empty());
        assert_eq!(result.rejected.len(), 1);
    }

    #[tokio::test]
    async fn test_verify_new_facts_rejects_wrong_namespace() {
        let journal = Arc::new(MockIndexedJournal::new([1u8; 32]));
        let time = MockTimeEffects::new(TEST_TIME_MS);
        let verifier = MerkleVerifier::new(journal, time);
        let context_id = ContextId::new_from_entropy([0x26; 32]);
        let (claimed_root, mut envelopes) =
            signed_fact_envelopes(context_id, vec![create_test_fact(103)]).await;
        envelopes[0].namespace =
            FactEnvelopeNamespace::Context(ContextId::new_from_entropy([0x27; 32]));

        let result = verifier
            .verify_incoming_facts(envelopes, claimed_root, context_id)
            .await
            .unwrap();

        assert!(result.verified.is_empty());
        assert_eq!(result.rejected.len(), 1);
    }

    #[tokio::test]
    async fn test_verify_new_facts_rejects_invalid_schema_version() {
        let journal = Arc::new(MockIndexedJournal::new([1u8; 32]));
        let time = MockTimeEffects::new(TEST_TIME_MS);
        let verifier = MerkleVerifier::new(journal, time);
        let context_id = ContextId::new_from_entropy([0x28; 32]);
        let (claimed_root, mut envelopes) =
            signed_fact_envelopes(context_id, vec![create_test_fact(104)]).await;
        envelopes[0].schema_version = 99;

        let result = verifier
            .verify_incoming_facts(envelopes, claimed_root, context_id)
            .await
            .unwrap();

        assert!(result.verified.is_empty());
        assert_eq!(result.rejected.len(), 1);
    }

    #[tokio::test]
    async fn test_verify_new_facts_rejects_forged_authority_binding() {
        let journal = Arc::new(MockIndexedJournal::new([1u8; 32]));
        let time = MockTimeEffects::new(TEST_TIME_MS);
        let verifier = MerkleVerifier::new(journal, time);
        let context_id = ContextId::new_from_entropy([0x25; 32]);
        let (claimed_root, mut envelopes) =
            signed_fact_envelopes(context_id, vec![create_test_fact(102)]).await;
        envelopes[0].fact.authority = Some(AuthorityId::new_from_entropy([0xAA; 32]));

        let result = verifier
            .verify_incoming_facts(envelopes, claimed_root, context_id)
            .await
            .unwrap();

        assert!(result.verified.is_empty());
        assert_eq!(result.rejected.len(), 1);
    }

    #[tokio::test]
    async fn test_verify_new_facts_rejects_missing_authority() {
        let journal = Arc::new(MockIndexedJournal::new([1u8; 32]));
        let time = MockTimeEffects::new(TEST_TIME_MS);
        let verifier = MerkleVerifier::new(journal, time);
        let context_id = ContextId::new_from_entropy([0x29; 32]);
        let (claimed_root, mut envelopes) =
            signed_fact_envelopes(context_id, vec![create_test_fact(105)]).await;
        envelopes[0].fact.authority = None;

        let result = verifier
            .verify_incoming_facts(envelopes, claimed_root, context_id)
            .await
            .unwrap();

        assert!(result.verified.is_empty());
        assert_eq!(result.rejected.len(), 1);
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
