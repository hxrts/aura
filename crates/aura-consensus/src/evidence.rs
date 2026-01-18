//! Evidence middleware for automatic equivocation proof propagation
//!
//! This module provides middleware that automatically attaches evidence deltas
//! to outgoing messages and merges incoming evidence.

use crate::types::ConsensusId;
use aura_core::{AuthorityId, Hash32, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Cryptographic proof of equivocation
///
/// Contains two conflicting shares from the same witness for the same consensus
/// instance, proving malicious behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquivocationProof {
    /// The witness that equivocated
    pub witness: AuthorityId,
    /// Consensus instance ID
    pub consensus_id: ConsensusId,
    /// Prestate hash (for binding verification)
    pub prestate_hash: Hash32,
    /// First result ID voted for
    pub first_result_id: Hash32,
    /// Second (conflicting) result ID voted for
    pub second_result_id: Hash32,
    /// Timestamp (milliseconds) when equivocation was detected
    pub timestamp_ms: u64,
}

impl EquivocationProof {
    /// Create a new equivocation proof
    pub fn new(
        witness: AuthorityId,
        consensus_id: ConsensusId,
        prestate_hash: Hash32,
        first_result_id: Hash32,
        second_result_id: Hash32,
        timestamp_ms: u64,
    ) -> Self {
        Self {
            witness,
            consensus_id,
            prestate_hash,
            first_result_id,
            second_result_id,
            timestamp_ms,
        }
    }

    /// Verify that this proof represents valid equivocation
    pub fn verify(&self) -> Result<()> {
        // Verify the two result IDs are different
        if self.first_result_id == self.second_result_id {
            return Err(aura_core::AuraError::invalid(
                "Equivocation proof must have different result IDs",
            ));
        }

        Ok(())
    }
}

/// Incremental evidence delta attached to messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceDelta {
    /// Consensus instance this evidence pertains to
    pub consensus_id: ConsensusId,
    /// New equivocation proofs since last message
    pub equivocation_proofs: Vec<EquivocationProof>,
    /// Timestamp (milliseconds) of this delta
    pub timestamp_ms: u64,
}

impl EvidenceDelta {
    /// Create an empty evidence delta
    pub fn empty(consensus_id: ConsensusId, timestamp_ms: u64) -> Self {
        Self {
            consensus_id,
            equivocation_proofs: Vec::new(),
            timestamp_ms,
        }
    }

    /// Check if this delta contains any evidence
    pub fn is_empty(&self) -> bool {
        self.equivocation_proofs.is_empty()
    }
}

/// Evidence tracker maintains all known equivocation proofs
pub struct EvidenceTracker {
    /// Equivocation proofs by consensus ID
    evidence_by_cid: HashMap<ConsensusId, Vec<EquivocationProof>>,
    /// Last synchronized timestamp (milliseconds) per consensus ID
    last_sync: HashMap<ConsensusId, u64>,
}

impl EvidenceTracker {
    /// Create a new evidence tracker
    pub fn new() -> Self {
        Self {
            evidence_by_cid: HashMap::new(),
            last_sync: HashMap::new(),
        }
    }

    /// Get evidence delta for a consensus instance
    ///
    /// Returns all new equivocation proofs since the last call for this CID.
    pub fn get_delta(&mut self, cid: ConsensusId, timestamp_ms: u64) -> EvidenceDelta {
        let proofs = self
            .evidence_by_cid
            .get(&cid)
            .map(|vec| {
                let last_ts = self.last_sync.get(&cid).copied();
                vec.iter()
                    .filter(|p| last_ts.map(|ts| p.timestamp_ms > ts).unwrap_or(true))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        // Update last sync timestamp
        self.last_sync.insert(cid, timestamp_ms);

        EvidenceDelta {
            consensus_id: cid,
            equivocation_proofs: proofs,
            timestamp_ms,
        }
    }

    /// Merge incoming evidence delta
    pub fn merge(&mut self, delta: EvidenceDelta) -> Result<usize> {
        let mut new_proofs = 0;

        for proof in delta.equivocation_proofs {
            // Verify proof before merging
            proof.verify()?;

            let proofs = self.evidence_by_cid.entry(delta.consensus_id).or_default();

            // Check if we already have this proof (simple duplicate detection)
            if !proofs.iter().any(|p| {
                p.witness == proof.witness
                    && p.first_result_id == proof.first_result_id
                    && p.second_result_id == proof.second_result_id
            }) {
                proofs.push(proof);
                new_proofs += 1;
            }
        }

        Ok(new_proofs)
    }

    /// Add a single equivocation proof
    pub fn add_proof(&mut self, proof: EquivocationProof) -> Result<bool> {
        proof.verify()?;

        let proofs = self.evidence_by_cid.entry(proof.consensus_id).or_default();

        // Check for duplicates
        if proofs.iter().any(|p| {
            p.witness == proof.witness
                && p.first_result_id == proof.first_result_id
                && p.second_result_id == proof.second_result_id
        }) {
            return Ok(false);
        }

        proofs.push(proof);
        Ok(true)
    }

    /// Get all equivocation proofs for a consensus instance
    pub fn get_proofs(&self, cid: ConsensusId) -> Vec<EquivocationProof> {
        self.evidence_by_cid.get(&cid).cloned().unwrap_or_default()
    }

    /// Check if a witness has any equivocation proofs
    pub fn has_equivocated(&self, cid: ConsensusId, witness: &AuthorityId) -> bool {
        self.evidence_by_cid
            .get(&cid)
            .map(|proofs| proofs.iter().any(|p| p.witness == *witness))
            .unwrap_or(false)
    }
}

impl Default for EvidenceTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Augmented message with evidence delta
///
/// This wrapper automatically attaches evidence to messages during send
/// and extracts it during receive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AugmentedMessage<M> {
    /// The actual message payload
    pub payload: M,
    /// Evidence delta attached to this message
    pub evidence_delta: EvidenceDelta,
}

/// Evidence middleware wrapper for transport
///
/// Automatically attaches evidence deltas to outgoing messages and merges
/// incoming evidence deltas.
pub struct EvidenceMiddleware<T> {
    /// Inner transport
    inner: T,
    /// Shared evidence tracker
    evidence: Arc<RwLock<EvidenceTracker>>,
}

impl<T> EvidenceMiddleware<T> {
    /// Create a new evidence middleware wrapper
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            evidence: Arc::new(RwLock::new(EvidenceTracker::new())),
        }
    }

    /// Create with existing evidence tracker
    pub fn with_evidence(inner: T, evidence: Arc<RwLock<EvidenceTracker>>) -> Self {
        Self { inner, evidence }
    }

    /// Get reference to evidence tracker
    pub fn evidence(&self) -> Arc<RwLock<EvidenceTracker>> {
        Arc::clone(&self.evidence)
    }

    /// Get inner transport
    pub fn into_inner(self) -> T {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn test_consensus_id(seed: u8) -> ConsensusId {
        ConsensusId(Hash32::new([seed; 32]))
    }

    fn test_timestamp_ms(ms: u64) -> u64 {
        ms
    }

    #[test]
    fn test_equivocation_proof_verify() {
        let proof = EquivocationProof::new(
            test_authority(1),
            test_consensus_id(1),
            Hash32::new([1; 32]),
            Hash32::new([2; 32]),
            Hash32::new([3; 32]),
            test_timestamp_ms(1000),
        );

        assert!(proof.verify().is_ok());

        // Same result IDs should fail
        let bad_proof = EquivocationProof::new(
            test_authority(1),
            test_consensus_id(1),
            Hash32::new([1; 32]),
            Hash32::new([2; 32]),
            Hash32::new([2; 32]), // Same as first!
            test_timestamp_ms(1000),
        );

        assert!(bad_proof.verify().is_err());
    }

    #[test]
    fn test_evidence_tracker_add_proof() {
        let mut tracker = EvidenceTracker::new();

        let proof1 = EquivocationProof::new(
            test_authority(1),
            test_consensus_id(1),
            Hash32::new([1; 32]),
            Hash32::new([2; 32]),
            Hash32::new([3; 32]),
            test_timestamp_ms(1000),
        );

        // First insert should succeed
        assert!(tracker.add_proof(proof1.clone()).unwrap());

        // Duplicate insert should return false
        assert!(!tracker.add_proof(proof1).unwrap());
    }

    #[test]
    fn test_evidence_delta_only_includes_new_proofs() {
        let mut tracker = EvidenceTracker::new();
        let cid = test_consensus_id(1);

        let proof1 = EquivocationProof::new(
            test_authority(1),
            cid,
            Hash32::new([1; 32]),
            Hash32::new([2; 32]),
            Hash32::new([3; 32]),
            test_timestamp_ms(1000),
        );

        let proof2 = EquivocationProof::new(
            test_authority(2),
            cid,
            Hash32::new([1; 32]),
            Hash32::new([4; 32]),
            Hash32::new([5; 32]),
            test_timestamp_ms(2000),
        );

        tracker.add_proof(proof1.clone()).unwrap();

        // Get delta at t=1500 - should include proof1
        let delta1 = tracker.get_delta(cid, test_timestamp_ms(1500));
        assert_eq!(delta1.equivocation_proofs.len(), 1);

        // Add proof2 after first delta
        tracker.add_proof(proof2.clone()).unwrap();

        // Get delta at t=2500 - should only include proof2 (new since last sync)
        let delta2 = tracker.get_delta(cid, test_timestamp_ms(2500));
        assert_eq!(delta2.equivocation_proofs.len(), 1);
        assert_eq!(delta2.equivocation_proofs[0].witness, test_authority(2));
    }

    #[test]
    fn test_evidence_merge() {
        let mut tracker = EvidenceTracker::new();
        let cid = test_consensus_id(1);

        let proof1 = EquivocationProof::new(
            test_authority(1),
            cid,
            Hash32::new([1; 32]),
            Hash32::new([2; 32]),
            Hash32::new([3; 32]),
            test_timestamp_ms(1000),
        );

        let delta = EvidenceDelta {
            consensus_id: cid,
            equivocation_proofs: vec![proof1],
            timestamp_ms: 1000,
        };

        let new_count = tracker.merge(delta).unwrap();
        assert_eq!(new_count, 1);

        // Verify proof was added
        assert!(tracker.has_equivocated(cid, &test_authority(1)));
    }

    #[test]
    fn test_has_equivocated() {
        let mut tracker = EvidenceTracker::new();
        let cid = test_consensus_id(1);

        assert!(!tracker.has_equivocated(cid, &test_authority(1)));

        let proof = EquivocationProof::new(
            test_authority(1),
            cid,
            Hash32::new([1; 32]),
            Hash32::new([2; 32]),
            Hash32::new([3; 32]),
            test_timestamp_ms(1000),
        );

        tracker.add_proof(proof).unwrap();

        assert!(tracker.has_equivocated(cid, &test_authority(1)));
        assert!(!tracker.has_equivocated(cid, &test_authority(2)));
    }
}
