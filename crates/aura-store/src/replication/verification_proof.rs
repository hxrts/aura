//! Proof-of-Storage Verification and Scoring
//!
//! Verifies proof-of-storage responses and maintains confidence scores for replicas:
//! - **Verification**: Validate challenges against proof responses
//! - **Scoring**: Track replica reliability based on verification history
//! - **Threshold**: Determine when a replica is no longer trustworthy
//!
//! Reference: docs/040_storage.md Section 6.1

use super::verification_challenge::{Challenge, ProofResponse, ReplicaMetadata};
use crate::error::{Result, StoreError, StoreErrorBuilder};
use serde::{Deserialize, Serialize};

/// Result of proof-of-storage verification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationResult {
    /// Proof was valid - replica confirmed to have data
    Valid,

    /// Proof was invalid - data missing or corrupted
    Invalid,

    /// Could not verify due to error (retry later)
    Error,
}

/// Proof-of-storage verifier
///
/// Manages verification of storage proofs and maintains confidence scores
/// for replicas based on verification history.
pub struct ProofOfStorageVerifier {
    /// Replica metadata including verification history
    replicas: std::collections::BTreeMap<Vec<u8>, ReplicaMetadata>,

    /// Minimum success rate required to trust a replica (0.0-1.0)
    min_success_rate: f64,

    /// Maximum failures before marking replica as untrusted
    max_failures: u64,
}

impl ProofOfStorageVerifier {
    /// Create a new verifier with default thresholds
    pub fn new() -> Self {
        Self {
            replicas: std::collections::BTreeMap::new(),
            min_success_rate: 0.8, // 80% minimum success rate
            max_failures: 5,       // 5 consecutive failures
        }
    }

    /// Create a new verifier with custom thresholds
    pub fn with_thresholds(min_success_rate: f64, max_failures: u64) -> Self {
        Self {
            replicas: std::collections::BTreeMap::new(),
            min_success_rate,
            max_failures,
        }
    }

    /// Register a replica for tracking
    pub fn register_replica(&mut self, metadata: ReplicaMetadata) {
        let key = metadata.replica_tag.0.as_bytes().to_vec();
        self.replicas.insert(key, metadata);
    }

    /// Verify a proof-of-storage response
    pub fn verify(
        &mut self,
        challenge: &Challenge,
        response: &ProofResponse,
        verified_at: u64,
    ) -> Result<VerificationResult> {
        // In a full implementation, would:
        // 1. Recompute proof hash: BLAKE3(chunk || replica_tag || nonce || session_epoch)
        // 2. Verify signature over proof_hash
        // 3. Check session epoch freshness

        // For now, simplified verification:
        if response.proof_hash.iter().all(|&b| b == 0) {
            return Ok(VerificationResult::Invalid);
        }

        let key = response.replica_tag.0.as_bytes().to_vec();

        if let Some(metadata) = self.replicas.get_mut(&key) {
            metadata.mark_challenge_success(verified_at);
            Ok(VerificationResult::Valid)
        } else {
            Err(StoreErrorBuilder::not_found(format!(
                "Replica {:?} not registered",
                response.replica_tag
            )))
        }
    }

    /// Mark a proof as invalid
    pub fn mark_invalid(&mut self, response: &ProofResponse) -> Result<()> {
        let key = response.replica_tag.0.as_bytes().to_vec();

        if let Some(metadata) = self.replicas.get_mut(&key) {
            metadata.mark_challenge_failure();

            // Check if we should mark replica as untrusted
            if metadata.challenge_failures >= self.max_failures {
                return Err(StoreErrorBuilder::replication_failed(format!(
                    "Replica exceeded failure threshold: {}",
                    metadata.challenge_failures
                )));
            }

            Ok(())
        } else {
            Err(StoreErrorBuilder::not_found(format!(
                "Replica {:?} not registered",
                response.replica_tag
            )))
        }
    }

    /// Check if a replica is currently trusted
    pub fn is_trusted(&self, replica_key: &[u8]) -> bool {
        if let Some(metadata) = self.replicas.get(replica_key) {
            metadata.success_rate() >= self.min_success_rate
                && metadata.challenge_failures < self.max_failures
        } else {
            false
        }
    }

    /// Get replica metadata
    pub fn get_replica(&self, replica_key: &[u8]) -> Option<&ReplicaMetadata> {
        self.replicas.get(replica_key)
    }

    /// Get all registered replicas
    pub fn all_replicas(&self) -> Vec<&ReplicaMetadata> {
        self.replicas.values().collect()
    }

    /// Get trusted replicas for a chunk
    pub fn get_trusted_replicas_for_chunk(&self, chunk_cid: &[u8]) -> Vec<&ReplicaMetadata> {
        self.replicas
            .iter()
            .filter(|(key, metadata)| metadata.chunk_cid == chunk_cid && self.is_trusted(key))
            .map(|(_, metadata)| metadata)
            .collect()
    }

    /// Get confidence score for a replica (0.0-1.0)
    pub fn confidence_score(&self, replica_key: &[u8]) -> f64 {
        if let Some(metadata) = self.replicas.get(replica_key) {
            let success_rate = metadata.success_rate();
            let failure_penalty = (metadata.challenge_failures as f64) / 10.0;
            (success_rate * (1.0 - failure_penalty)).max(0.0)
        } else {
            0.0
        }
    }
}

impl Default for ProofOfStorageVerifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replication::verification_challenge::ReplicaTag;
    use uuid::Uuid;

    #[test]
    fn test_verifier_creation() {
        let verifier = ProofOfStorageVerifier::new();
        assert_eq!(verifier.min_success_rate, 0.8);
        assert_eq!(verifier.max_failures, 5);
    }

    #[test]
    fn test_register_replica() {
        let mut verifier = ProofOfStorageVerifier::new();
        let metadata =
            ReplicaMetadata::new(ReplicaTag(Uuid::nil()), vec![1u8; 32], vec![2u8; 32], 1000);
        let key = metadata.replica_tag.0.as_bytes().to_vec();

        verifier.register_replica(metadata);
        assert!(verifier.get_replica(&key).is_some());
    }

    #[test]
    fn test_confidence_score() {
        let mut verifier = ProofOfStorageVerifier::new();
        let mut metadata =
            ReplicaMetadata::new(ReplicaTag(Uuid::nil()), vec![1u8; 32], vec![2u8; 32], 1000);

        metadata.mark_challenge_success(2000);
        metadata.mark_challenge_success(2100);
        metadata.mark_challenge_failure();

        let key = metadata.replica_tag.0.as_bytes().to_vec();
        verifier.register_replica(metadata);

        let score = verifier.confidence_score(&key);
        // 2/3 success rate = ~0.667, penalty = 1/10 = 0.1
        // Expected: 0.667 * (1.0 - 0.1) = 0.667 * 0.9 = 0.6
        assert_eq!(score, 0.6);
    }

    #[test]
    fn test_is_trusted() {
        let mut verifier = ProofOfStorageVerifier::with_thresholds(0.5, 3);
        let metadata =
            ReplicaMetadata::new(ReplicaTag(Uuid::nil()), vec![1u8; 32], vec![2u8; 32], 1000);
        let key = metadata.replica_tag.0.as_bytes().to_vec();

        verifier.register_replica(metadata);

        // New replica has 100% success rate, should be trusted
        assert!(verifier.is_trusted(&key));
    }

    #[test]
    fn test_get_trusted_replicas_for_chunk() {
        let mut verifier = ProofOfStorageVerifier::new();
        let chunk_cid = vec![1u8; 32];

        let metadata1 = ReplicaMetadata::new(
            ReplicaTag(Uuid::nil()),
            vec![2u8; 32],
            chunk_cid.clone(),
            1000,
        );
        let metadata2 = ReplicaMetadata::new(
            ReplicaTag(Uuid::new_v4()),
            vec![3u8; 32],
            chunk_cid.clone(),
            1000,
        );

        verifier.register_replica(metadata1);
        verifier.register_replica(metadata2);

        let trusted = verifier.get_trusted_replicas_for_chunk(&chunk_cid);
        assert_eq!(trusted.len(), 2);
    }
}
