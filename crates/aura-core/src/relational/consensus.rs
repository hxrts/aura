//! Consensus proof types for relational contexts
//!
//! This module defines the domain types for consensus proofs
//! that validate operations in relational contexts.

use crate::{crypto::frost::ThresholdSignature, AuthorityId, Hash32};
use serde::{Deserialize, Serialize};

/// Proof of consensus for an operation in a relational context
///
/// This is a pure domain type that contains the essential information
/// to validate that an operation was agreed upon through consensus.
/// It mirrors the commit fact produced by Aura Consensus while keeping
/// a lightweight shape for relational consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusProof {
    /// Hash of the prestate this operation is bound to
    pub prestate_hash: Hash32,
    /// Hash of the operation being agreed upon (result identifier: rid = H(Op, prestate))
    pub operation_hash: Hash32,
    /// FROST threshold signature aggregated from witness shares
    pub threshold_signature: Option<ThresholdSignature>,
    /// Set of authorities that provided valid shares
    pub attester_set: Vec<AuthorityId>,
    /// Whether the threshold was met
    pub threshold_met: bool,
}

impl ConsensusProof {
    /// Create a new consensus proof
    pub fn new(
        prestate_hash: Hash32,
        operation_hash: Hash32,
        threshold_signature: Option<ThresholdSignature>,
        attester_set: Vec<AuthorityId>,
        threshold_met: bool,
    ) -> Self {
        Self {
            prestate_hash,
            operation_hash,
            threshold_signature,
            attester_set,
            threshold_met,
        }
    }

    /// Check if this proof has a valid threshold signature
    pub fn has_signature(&self) -> bool {
        self.threshold_signature.is_some()
    }

    /// Check if the consensus threshold was met
    pub fn threshold_met(&self) -> bool {
        self.threshold_met
    }

    /// Get the number of attesters
    pub fn attester_count(&self) -> usize {
        self.attester_set.len()
    }

    /// Check if a specific authority attested to this operation
    pub fn has_attester(&self, authority_id: &AuthorityId) -> bool {
        self.attester_set.contains(authority_id)
    }

    /// Get the list of attester authorities
    pub fn attesters(&self) -> &[AuthorityId] {
        &self.attester_set
    }

    /// Check if this proof is valid (threshold met and has signature)
    pub fn is_valid(&self) -> bool {
        self.threshold_met && self.threshold_signature.is_some()
    }

    /// Check if this proof is complete (has all required components)
    pub fn is_complete(&self) -> bool {
        !self.attester_set.is_empty() && self.threshold_met
    }

    /// Create a proof for testing purposes (without signature)
    #[cfg(test)]
    pub fn test_proof(
        prestate_hash: Hash32,
        operation_hash: Hash32,
        attester_set: Vec<AuthorityId>,
    ) -> Self {
        Self::new(
            prestate_hash,
            operation_hash,
            None, // No signature for test proofs
            attester_set,
            true, // Assume threshold is met for tests
        )
    }

    /// Create a failed consensus proof (threshold not met)
    pub fn failed(
        prestate_hash: Hash32,
        operation_hash: Hash32,
        attester_set: Vec<AuthorityId>,
    ) -> Self {
        Self::new(
            prestate_hash,
            operation_hash,
            None,
            attester_set,
            false, // Threshold not met
        )
    }
}

// Implement equality based on semantic fields, excluding cryptographic signature
// This allows for comparison of proofs based on their logical content rather than
// cryptographic details, which is useful for testing and deduplication
impl PartialEq for ConsensusProof {
    fn eq(&self, other: &Self) -> bool {
        self.prestate_hash == other.prestate_hash
            && self.operation_hash == other.operation_hash
            && self.threshold_met == other.threshold_met
            && self.attester_set == other.attester_set
    }
}

impl Eq for ConsensusProof {}

// Implement ordering based on semantic fields for use in sorted collections
// Orders first by prestate hash, then operation hash, then threshold status,
// then by attester set for deterministic ordering in data structures
impl PartialOrd for ConsensusProof {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ConsensusProof {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.prestate_hash
            .cmp(&other.prestate_hash)
            .then(self.operation_hash.cmp(&other.operation_hash))
            .then(self.threshold_met.cmp(&other.threshold_met))
            .then(self.attester_set.cmp(&other.attester_set))
    }
}

/// Status of a consensus operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ConsensusStatus {
    /// Consensus is pending (operation in progress)
    Pending,
    /// Consensus succeeded (threshold met)
    Succeeded,
    /// Consensus failed (threshold not met)
    Failed,
    /// Consensus timed out
    TimedOut,
}

impl ConsensusStatus {
    /// Check if this status indicates success
    pub fn is_successful(&self) -> bool {
        matches!(self, ConsensusStatus::Succeeded)
    }

    /// Check if this status indicates failure
    pub fn is_failed(&self) -> bool {
        matches!(self, ConsensusStatus::Failed | ConsensusStatus::TimedOut)
    }

    /// Check if this status indicates the operation is still in progress
    pub fn is_pending(&self) -> bool {
        matches!(self, ConsensusStatus::Pending)
    }

    /// Check if this status indicates the operation is complete (success or failure)
    pub fn is_complete(&self) -> bool {
        !self.is_pending()
    }
}

impl std::fmt::Display for ConsensusStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsensusStatus::Pending => write!(f, "pending"),
            ConsensusStatus::Succeeded => write!(f, "succeeded"),
            ConsensusStatus::Failed => write!(f, "failed"),
            ConsensusStatus::TimedOut => write!(f, "timed_out"),
        }
    }
}

impl From<&ConsensusProof> for ConsensusStatus {
    fn from(proof: &ConsensusProof) -> Self {
        if proof.threshold_met {
            ConsensusStatus::Succeeded
        } else {
            ConsensusStatus::Failed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consensus_proof_creation() {
        let prestate_hash = Hash32::default();
        let operation_hash = Hash32([1u8; 32]);
        let attester = AuthorityId::new();

        let proof = ConsensusProof::new(prestate_hash, operation_hash, None, vec![attester], true);

        assert_eq!(proof.prestate_hash, prestate_hash);
        assert_eq!(proof.operation_hash, operation_hash);
        assert!(!proof.has_signature());
        assert!(proof.threshold_met());
        assert_eq!(proof.attester_count(), 1);
        assert!(proof.has_attester(&attester));
    }

    #[test]
    fn test_consensus_proof_validity() {
        let attester = AuthorityId::new();

        // Valid proof with threshold met but no signature (incomplete)
        let proof_no_sig = ConsensusProof::new(
            Hash32::default(),
            Hash32::default(),
            None,
            vec![attester],
            true,
        );
        assert!(!proof_no_sig.is_valid()); // No signature
        assert!(proof_no_sig.is_complete()); // Has attesters and threshold met

        // Failed proof
        let failed_proof =
            ConsensusProof::failed(Hash32::default(), Hash32::default(), vec![attester]);
        assert!(!failed_proof.is_valid());
        assert!(!failed_proof.threshold_met());
    }

    #[test]
    fn test_consensus_proof_equality() {
        let auth1 = AuthorityId::new();
        let auth2 = AuthorityId::new();

        let proof1 = ConsensusProof::new(
            Hash32::default(),
            Hash32::default(),
            None,
            vec![auth1],
            true,
        );

        let proof2 = ConsensusProof::new(
            Hash32::default(),
            Hash32::default(),
            None,
            vec![auth1],
            true,
        );

        let proof3 = ConsensusProof::new(
            Hash32::default(),
            Hash32::default(),
            None,
            vec![auth2],
            true,
        );

        assert_eq!(proof1, proof2);
        assert_ne!(proof1, proof3);
    }

    #[test]
    fn test_consensus_proof_ordering() {
        let auth = AuthorityId::new();
        let prestate1 = Hash32::default();
        let prestate2 = Hash32([1u8; 32]);

        let proof1 = ConsensusProof::new(prestate1, Hash32::default(), None, vec![auth], true);

        let proof2 = ConsensusProof::new(prestate2, Hash32::default(), None, vec![auth], true);

        assert!(proof1 < proof2); // prestate1 < prestate2
    }

    #[test]
    fn test_consensus_status() {
        let auth = AuthorityId::new();

        let success_proof =
            ConsensusProof::new(Hash32::default(), Hash32::default(), None, vec![auth], true);

        let fail_proof = ConsensusProof::failed(Hash32::default(), Hash32::default(), vec![auth]);

        assert_eq!(
            ConsensusStatus::from(&success_proof),
            ConsensusStatus::Succeeded
        );
        assert_eq!(ConsensusStatus::from(&fail_proof), ConsensusStatus::Failed);

        assert!(ConsensusStatus::Succeeded.is_successful());
        assert!(!ConsensusStatus::Failed.is_successful());
        assert!(ConsensusStatus::Failed.is_failed());
        assert!(!ConsensusStatus::Succeeded.is_failed());
    }

    #[test]
    fn test_consensus_proof_test_helper() {
        let auth = AuthorityId::new();

        let test_proof =
            ConsensusProof::test_proof(Hash32::default(), Hash32::default(), vec![auth]);

        assert!(test_proof.threshold_met());
        assert!(!test_proof.has_signature());
        assert!(!test_proof.is_valid()); // No signature makes it invalid
        assert!(test_proof.is_complete()); // Has attesters and threshold met
    }
}
