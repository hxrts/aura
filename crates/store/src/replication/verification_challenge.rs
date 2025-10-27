//! Proof-of-Storage Challenge-Response System
//!
//! Implements challenge-response protocol for proving storage peers possess data:
//! - **Challenge**: Cryptographic challenge with random nonce to prevent replays
//! - **Response**: Cryptographic proof of data possession via signature
//! - **Verification**: Validates proof and updates replica confidence
//!
//! This is separate from peer selection to allow verification independent of strategy.
//!
//! Reference: docs/040_storage.md Section 6.1

use crate::error::{Result, StoreError};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type Cid = Vec<u8>;
pub type DeviceId = Vec<u8>;
pub type SessionEpoch = u64;

/// Replica tag for tracking storage replicas
///
/// A unique identifier for each storage replica across devices in the network.
/// Used to distinguish between different copies of the same data chunk.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReplicaTag(pub Uuid);

impl ReplicaTag {
    /// Create a new replica tag using injected effects
    pub fn new_with_effects(effects: &aura_crypto::Effects) -> Self {
        ReplicaTag(effects.gen_uuid())
    }
}

/// Proof-of-storage challenge
///
/// A cryptographic challenge sent to storage providers to verify they possess
/// specific data chunks. Contains a random nonce to prevent replay attacks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Challenge {
    /// Content identifier of the chunk being challenged
    pub chunk_cid: Cid,

    /// Random nonce to prevent replay attacks
    pub nonce: [u8; 32],

    /// Device ID of the challenger for accountability
    pub challenger_id: DeviceId,
}

impl Challenge {
    /// Create a new challenge using injected effects
    pub fn new_with_effects(
        chunk_cid: Cid,
        challenger_id: DeviceId,
        effects: &aura_crypto::Effects,
    ) -> Self {
        let nonce = effects.random_bytes::<32>();

        Challenge {
            chunk_cid,
            nonce,
            challenger_id,
        }
    }
}

/// Proof-of-storage response
///
/// Response to a proof-of-storage challenge, containing cryptographic proof
/// that the responder possesses the challenged data chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofResponse {
    /// Replica tag identifying this specific storage replica
    pub replica_tag: ReplicaTag,

    /// Session epoch when the proof was generated
    pub session_epoch: u64,

    /// Digest of the presence ticket (for authentication)
    pub ticket_digest: [u8; 32],

    /// BLAKE3 hash proving possession of the data
    pub proof_hash: [u8; 32],

    /// Raw signature bytes over the proof hash
    pub signature: Vec<u8>,
}

/// Replica metadata
///
/// Metadata about a storage replica including tracking information
/// for challenge scheduling and verification history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaMetadata {
    /// Unique identifier for this replica
    pub replica_tag: ReplicaTag,

    /// Device storing this replica
    pub device_id: DeviceId,

    /// Content identifier of the stored chunk
    pub chunk_cid: Cid,

    /// Unix timestamp when replica was created
    pub created_at: u64,

    /// Unix timestamp of last successful verification
    pub last_verified: Option<u64>,

    /// Number of successful challenges
    pub challenge_successes: u64,

    /// Number of failed challenges
    pub challenge_failures: u64,
}

impl ReplicaMetadata {
    /// Create new replica metadata
    pub fn new(
        replica_tag: ReplicaTag,
        device_id: DeviceId,
        chunk_cid: Cid,
        created_at: u64,
    ) -> Self {
        Self {
            replica_tag,
            device_id,
            chunk_cid,
            created_at,
            last_verified: None,
            challenge_successes: 0,
            challenge_failures: 0,
        }
    }

    /// Mark successful challenge
    pub fn mark_challenge_success(&mut self, verified_at: u64) {
        self.challenge_successes += 1;
        self.last_verified = Some(verified_at);
    }

    /// Mark failed challenge
    pub fn mark_challenge_failure(&mut self) {
        self.challenge_failures += 1;
    }

    /// Get success rate as percentage
    pub fn success_rate(&self) -> f64 {
        let total = self.challenge_successes + self.challenge_failures;
        if total == 0 {
            1.0 // No challenges yet, assume 100%
        } else {
            (self.challenge_successes as f64) / (total as f64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replica_tag_creation() {
        let effects = aura_crypto::Effects::for_test("test_replica_tag");
        let tag1 = ReplicaTag::new_with_effects(&effects);
        let tag2 = ReplicaTag::new_with_effects(&effects);

        // Tags should be different (UUIDs are unique)
        assert_ne!(tag1, tag2);
    }

    #[test]
    fn test_challenge_creation() {
        let effects = aura_crypto::Effects::for_test("test_challenge");
        let challenge = Challenge::new_with_effects(vec![1u8; 32], vec![2u8; 32], &effects);

        assert_eq!(challenge.chunk_cid, vec![1u8; 32]);
        assert_eq!(challenge.challenger_id, vec![2u8; 32]);
    }

    #[test]
    fn test_replica_metadata_success_rate() {
        let metadata =
            ReplicaMetadata::new(ReplicaTag(Uuid::nil()), vec![1u8; 32], vec![2u8; 32], 1000);

        // New metadata should have 100% success rate
        assert_eq!(metadata.success_rate(), 1.0);
    }

    #[test]
    fn test_replica_metadata_tracking() {
        let mut metadata =
            ReplicaMetadata::new(ReplicaTag(Uuid::nil()), vec![1u8; 32], vec![2u8; 32], 1000);

        metadata.mark_challenge_success(2000);
        metadata.mark_challenge_success(2100);
        metadata.mark_challenge_failure();

        assert_eq!(metadata.challenge_successes, 2);
        assert_eq!(metadata.challenge_failures, 1);
        assert!((metadata.success_rate() - 2.0 / 3.0).abs() < 0.01);
    }
}
