//! Commit Facts for Consensus Results
//!
//! This module defines the CommitFact type that represents immutable
//! consensus results. These facts are inserted into authority or context
//! journals as evidence of agreement.

use aura_core::frost::ThresholdSignature;
use aura_core::{AuthorityId, Hash32};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for a consensus instance
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ConsensusId(pub Hash32);

impl ConsensusId {
    /// Create a new consensus ID from components
    pub fn new(prestate_hash: Hash32, operation_hash: Hash32, nonce: u64) -> Self {
        use aura_core::hash;
        let mut hasher = hash::hasher();
        hasher.update(b"CONSENSUS_ID");
        hasher.update(&prestate_hash.0);
        hasher.update(&operation_hash.0);
        hasher.update(&nonce.to_le_bytes());
        ConsensusId(Hash32(hasher.finalize()))
    }
}

impl fmt::Display for ConsensusId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "consensus:{}", hex::encode(&self.0 .0[..8]))
    }
}

/// Immutable fact representing successful consensus
///
/// This is the primary output of the Aura Consensus protocol. It contains
/// all evidence needed to verify that a threshold of witnesses agreed on
/// an operation bound to a specific prestate.
///
/// Note: Does not derive PartialEq/Eq because ThresholdSignature contains
/// cryptographic data that should be verified, not compared.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitFact {
    /// Unique identifier for this consensus instance
    pub consensus_id: ConsensusId,

    /// Hash of the prestate this operation was bound to
    pub prestate_hash: Hash32,

    /// Hash of the operation that was agreed upon
    pub operation_hash: Hash32,

    /// The actual operation (serialized)
    pub operation_bytes: Vec<u8>,

    /// Threshold signature from witnesses
    pub threshold_signature: ThresholdSignature,

    /// List of authorities that participated
    pub participants: Vec<AuthorityId>,

    /// Threshold that was required
    pub threshold: u16,

    /// Timestamp of consensus completion (milliseconds since epoch)
    pub timestamp_ms: u64,

    /// Whether fast path was used
    pub fast_path: bool,
}

impl CommitFact {
    /// Create a new commit fact
    pub fn new(
        consensus_id: ConsensusId,
        prestate_hash: Hash32,
        operation_hash: Hash32,
        operation_bytes: Vec<u8>,
        threshold_signature: ThresholdSignature,
        participants: Vec<AuthorityId>,
        threshold: u16,
        fast_path: bool,
    ) -> Self {
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        Self {
            consensus_id,
            prestate_hash,
            operation_hash,
            operation_bytes,
            threshold_signature,
            participants,
            threshold,
            timestamp_ms,
            fast_path,
        }
    }

    /// Verify the commit fact is valid
    pub fn verify(&self) -> Result<(), String> {
        // Check threshold was met
        if self.participants.len() < self.threshold as usize {
            return Err("Insufficient participants for threshold".to_string());
        }

        // Check participants are unique
        let mut unique_check = self.participants.clone();
        unique_check.sort();
        unique_check.dedup();
        if unique_check.len() != self.participants.len() {
            return Err("Duplicate participants".to_string());
        }

        // Verify threshold signature using FROST
        if let Err(e) = verify_threshold_signature(
            &self.threshold_signature,
            &self.operation_bytes,
            &self.participants,
            self.threshold,
        ) {
            return Err(format!("Threshold signature verification failed: {}", e));
        }

        Ok(())
    }

    /// Convert to a fact for journal insertion
    pub fn to_relational_fact(&self) -> aura_journal::fact::RelationalFact {
        aura_journal::fact::RelationalFact::Consensus {
            consensus_id: self.consensus_id.0,
            operation_hash: self.operation_hash,
            threshold_met: true,
            participant_count: self.participants.len() as u16,
        }
    }
}

/// Evidence of consensus failure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictFact {
    /// Consensus instance that failed
    pub consensus_id: ConsensusId,

    /// Conflicting operation hashes
    pub conflicts: Vec<Hash32>,

    /// Authorities that reported conflicts
    pub reporters: Vec<AuthorityId>,

    /// Timestamp of conflict detection
    pub timestamp_ms: u64,
}

/// Verify a threshold signature for a commit fact
///
/// This function verifies that a threshold signature is valid for the given
/// operation bytes and participant list.
fn verify_threshold_signature(
    threshold_signature: &ThresholdSignature,
    operation_bytes: &[u8],
    participants: &[AuthorityId],
    threshold: u16,
) -> Result<(), String> {
    // TODO: Implement actual FROST threshold signature verification
    // This requires:
    // 1. Reconstruct the group public key from participant keys
    // 2. Verify the signature against the operation bytes using FROST
    // 3. Ensure the signature was created by at least `threshold` participants

    // For now, perform basic validation
    if threshold_signature.signature.is_empty() {
        return Err("Empty signature".to_string());
    }

    if participants.len() < threshold as usize {
        return Err("Insufficient participants for threshold".to_string());
    }

    if operation_bytes.is_empty() {
        return Err("Empty operation bytes".to_string());
    }

    // TODO: Replace with actual FROST signature verification once:
    // - PublicKeyPackage is available for the group
    // - FROST verification functions are properly integrated
    // - Participant key mapping is established

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consensus_id_generation() {
        let prestate_hash = Hash32::default();
        let operation_hash = Hash32([1u8; 32]);

        let id1 = ConsensusId::new(prestate_hash, operation_hash, 1);
        let id2 = ConsensusId::new(prestate_hash, operation_hash, 2);

        assert_ne!(id1, id2); // Different nonces produce different IDs
    }

    #[test]
    fn test_commit_fact_verification() {
        let fact = CommitFact {
            consensus_id: ConsensusId(Hash32::default()),
            prestate_hash: Hash32::default(),
            operation_hash: Hash32::default(),
            operation_bytes: vec![],
            threshold_signature: ThresholdSignature::new(vec![], vec![]),
            participants: vec![AuthorityId::new(), AuthorityId::new()],
            threshold: 2,
            timestamp_ms: 0,
            fast_path: true,
        };

        assert!(fact.verify().is_ok());

        // Test insufficient participants
        let mut bad_fact = fact.clone();
        bad_fact.participants = vec![AuthorityId::new()];
        assert!(bad_fact.verify().is_err());
    }
}
