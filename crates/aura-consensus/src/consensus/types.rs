//! Core consensus types
//!
//! This module contains the fundamental types used throughout the consensus system.

use aura_core::{
    crypto::tree_signing::frost_verify_aggregate,
    epochs::Epoch,
    frost::{PublicKeyPackage, ThresholdSignature},
    time::ProvenancedTime,
    AuraError, AuthorityId, Hash32, Result,
};
use frost_ed25519;
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

    /// Group public key used to verify the threshold signature (if available)
    pub group_public_key: Option<PublicKeyPackage>,

    /// List of authorities that participated
    pub participants: Vec<AuthorityId>,

    /// Threshold that was required
    pub threshold: u16,

    /// Timestamp (with optional provenance) of consensus completion
    pub timestamp: ProvenancedTime,

    /// Whether fast path was used
    pub fast_path: bool,
}

impl CommitFact {
    /// Create a new commit fact
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        consensus_id: ConsensusId,
        prestate_hash: Hash32,
        operation_hash: Hash32,
        operation_bytes: Vec<u8>,
        threshold_signature: ThresholdSignature,
        group_public_key: Option<PublicKeyPackage>,
        participants: Vec<AuthorityId>,
        threshold: u16,
        fast_path: bool,
        timestamp: ProvenancedTime,
    ) -> Self {
        Self {
            consensus_id,
            prestate_hash,
            operation_hash,
            operation_bytes,
            threshold_signature,
            group_public_key,
            participants,
            threshold,
            timestamp,
            fast_path,
        }
    }

    /// Verify the commit fact is valid
    pub fn verify(&self) -> Result<()> {
        // Check threshold was met
        if self.participants.len() < self.threshold as usize {
            return Err(AuraError::invalid("Insufficient participants for threshold"));
        }

        // Check participants are unique
        let mut unique_check = self.participants.clone();
        unique_check.sort();
        unique_check.dedup();
        if unique_check.len() != self.participants.len() {
            return Err(AuraError::invalid("Duplicate participants"));
        }

        // Verify threshold signature against provided group public key
        let group_pkg = self
            .group_public_key
            .clone()
            .ok_or_else(|| AuraError::invalid("Missing group public key for verification"))?;

        let frost_pkg: frost_ed25519::keys::PublicKeyPackage =
            group_pkg
                .try_into()
                .map_err(|e: String| AuraError::invalid(format!("Invalid group public key package: {}", e)))?;

        frost_verify_aggregate(
            frost_pkg.verifying_key(),
            &self.operation_bytes,
            &self.threshold_signature.signature,
        )
        .map_err(|e| AuraError::crypto(format!("Threshold signature verification failed: {}", e)))?;

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

/// Consensus configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    /// Minimum number of witnesses required
    pub threshold: u16,

    /// Set of eligible witnesses
    pub witness_set: Vec<AuthorityId>,

    /// Timeout for consensus operations in milliseconds
    pub timeout_ms: u64,

    /// Enable fast path optimization (1 RTT pipelining)
    pub enable_pipelining: bool,

    /// Current epoch
    pub epoch: Epoch,
}

impl ConsensusConfig {
    /// Create a new consensus configuration
    pub fn new(threshold: u16, witness_set: Vec<AuthorityId>, epoch: Epoch) -> Result<Self> {
        if witness_set.is_empty() {
            return Err(AuraError::invalid(
                "Consensus requires at least one witness",
            ));
        }

        if threshold == 0 {
            return Err(AuraError::invalid("Consensus threshold must be >= 1"));
        }

        if witness_set.len() < threshold as usize {
            return Err(AuraError::invalid(
                "Consensus threshold exceeds witness set size",
            ));
        }

        Ok(Self {
            threshold,
            witness_set,
            timeout_ms: 30000, // 30 seconds default
            enable_pipelining: true,
            epoch,
        })
    }

    /// Check if we have sufficient witnesses for the threshold
    pub fn has_quorum(&self) -> bool {
        self.witness_set.len() >= self.threshold as usize
    }

    /// Get the minimum number of witnesses needed for fast path
    pub fn fast_path_threshold(&self) -> usize {
        self.threshold as usize
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
    pub timestamp: ProvenancedTime,
}

/// Result of a consensus operation
#[derive(Debug, Clone)]
pub enum ConsensusResult {
    /// Consensus succeeded with commit fact
    Committed(CommitFact),

    /// Consensus failed due to conflicts
    Conflicted(ConflictFact),

    /// Consensus timed out
    Timeout {
        consensus_id: ConsensusId,
        elapsed_ms: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    // Imports for future time-based consensus features
    // use aura_core::time::{PhysicalTime, TimeStamp};

    #[test]
    fn test_consensus_id_generation() {
        let prestate_hash = Hash32::default();
        let operation_hash = Hash32([1u8; 32]);

        let id1 = ConsensusId::new(prestate_hash, operation_hash, 1);
        let id2 = ConsensusId::new(prestate_hash, operation_hash, 2);

        assert_ne!(id1, id2); // Different nonces produce different IDs
    }

    #[test]
    fn test_consensus_config() {
        let witnesses = vec![
            AuthorityId::new_from_entropy([1u8; 32]),
            AuthorityId::new_from_entropy([2u8; 32]),
            AuthorityId::new_from_entropy([3u8; 32]),
        ];
        let config = ConsensusConfig::new(2, witnesses, Epoch::from(1)).unwrap();

        assert!(config.has_quorum());
        assert_eq!(config.threshold, 2);
        assert_eq!(config.timeout_ms, 30000);
        assert!(config.enable_pipelining);
    }
}
