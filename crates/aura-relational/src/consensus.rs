//! Consensus interface for RelationalContexts
//!
//! This module provides the consensus abstraction used by relational
//! contexts. This implements Aura Consensus as described in docs/104_consensus.md.

use crate::prestate::Prestate;
use aura_core::crypto::frost::{PartialSignature, ThresholdSignature};
use aura_core::{hash, AuthorityId, Hash32, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Proof of consensus for an operation
///
/// This structure captures the agreement of witnesses on a specific
/// operation bound to a prestate. This is the CommitFact described in
/// docs/104_consensus.md.
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

// Implement equality based on semantic fields, excluding cryptographic signature
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

/// Witness share for consensus
///
/// Individual witness contribution to threshold signature during consensus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessShare {
    /// Authority that produced this share
    pub authority: AuthorityId,
    /// FROST partial signature share
    pub partial_signature: PartialSignature,
    /// Prestate hash this share is bound to
    pub prestate_hash: Hash32,
}

/// Run consensus on an operation
///
/// Implements the Aura Consensus fast path as described in docs/104_consensus.md.
/// This is a simplified implementation that sets up the structures but delegates
/// actual protocol execution to the coordinator.
///
/// # Protocol Flow
///
/// 1. Create ConsensusId and bind operation to prestate (rid = H(Op, prestate))
/// 2. Broadcast Execute(cid, Op, prestate_hash) to witnesses
/// 3. Collect WitnessShare(cid, rid, partial_sig, prestate_hash) from witnesses
/// 4. Aggregate matching shares (same rid, prestate_hash) into threshold signature
/// 5. Return ConsensusProof with CommitFact
///
/// # Arguments
///
/// * `prestate` - The current state snapshot before the operation
/// * `operation` - The operation to reach consensus on
///
/// # Returns
///
/// A `ConsensusProof` containing the threshold signature and attester set if
/// consensus succeeded, or an indication of failure otherwise.
pub async fn run_consensus<T: Serialize>(
    prestate: &Prestate,
    operation: &T,
) -> Result<ConsensusProof> {
    let prestate_hash = prestate.compute_hash();
    let operation_hash = hash_operation(operation)?;

    // TODO: Full protocol implementation requires:
    // 1. ConsensusId generation (cid)
    // 2. Witness discovery from prestate
    // 3. Network broadcast of Execute messages
    // 4. Share collection with timeout handling
    // 5. FROST signature aggregation
    // 6. Fallback to epidemic gossip on fast path failure
    //
    // For now, return a proof structure with proper types but no actual consensus.
    // Orchestrator integration will provide witness communication and FROST operations.

    Ok(ConsensusProof {
        prestate_hash,
        operation_hash,
        threshold_signature: None, // Will be Some(sig) when FROST aggregation completes
        attester_set: Vec::new(),   // Will contain AuthorityIds of witnesses who provided shares
        threshold_met: false,       // Will be true when threshold of valid shares received
    })
}

/// Hash an operation for consensus
fn hash_operation<T: Serialize>(operation: &T) -> Result<Hash32> {
    let bytes = serde_json::to_vec(operation)
        .map_err(|e| aura_core::AuraError::serialization(e.to_string()))?;

    let mut h = hash::hasher();
    h.update(b"AURA_CONSENSUS_OP");
    h.update(&bytes);
    Ok(Hash32(h.finalize()))
}

/// Consensus configuration for a context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    /// Minimum number of witnesses required
    pub threshold: u16,
    /// Set of eligible witnesses
    pub witness_set: Vec<AuthorityId>,
    /// Timeout for consensus operations
    pub timeout_ms: u64,
}

impl ConsensusConfig {
    /// Create a new consensus configuration
    pub fn new(threshold: u16, witness_set: Vec<AuthorityId>) -> Self {
        Self {
            threshold,
            witness_set,
            timeout_ms: 30000, // 30 seconds default
        }
    }

    /// Check if a set of witness shares meets the threshold
    ///
    /// Validates that:
    /// 1. All shares come from witnesses in the configured witness set
    /// 2. The number of valid shares meets or exceeds the threshold
    pub fn check_threshold(&self, shares: &[WitnessShare]) -> bool {
        let valid_witnesses: Vec<_> = shares
            .iter()
            .filter(|share| self.witness_set.contains(&share.authority))
            .collect();

        valid_witnesses.len() >= self.threshold as usize
    }

    /// Check if prestate hashes match across shares
    ///
    /// All shares for a consensus instance must agree on the prestate hash.
    /// This prevents forks as described in docs/104_consensus.md section 5.
    pub fn verify_prestate_agreement(&self, shares: &[WitnessShare]) -> bool {
        if shares.is_empty() {
            return true;
        }

        let first_prestate = shares[0].prestate_hash;
        shares.iter().all(|s| s.prestate_hash == first_prestate)
    }
}

/// Result of a consensus operation
#[derive(Debug, Clone)]
pub enum ConsensusResult {
    /// Consensus succeeded with proof
    Success(ConsensusProof),
    /// Consensus failed due to timeout
    Timeout,
    /// Consensus failed due to insufficient witnesses
    InsufficientWitnesses,
    /// Consensus failed due to conflicting operations
    Conflict(Vec<Hash32>),
}

/// Consensus coordinator for managing instances
///
/// This will be replaced with actual coordinator from aura-protocol
pub struct ConsensusCoordinator {
    /// Active consensus instances
    instances: BTreeMap<Hash32, ConsensusInstance>,
}

/// A single consensus instance
///
/// Tracks state for one consensus operation as described in docs/104_consensus.md.
/// Each instance maintains:
/// - Configuration (threshold, witness set)
/// - Bound prestate (prevents forks)
/// - Operation hash (result identifier when combined with prestate)
/// - Collected witness shares (FROST partial signatures)
struct ConsensusInstance {
    config: ConsensusConfig,
    prestate: Prestate,
    operation_hash: Hash32,
    collected_shares: Vec<WitnessShare>,
    decided: bool, // Prevents double-voting
}

impl ConsensusCoordinator {
    /// Create a new coordinator
    pub fn new() -> Self {
        Self {
            instances: BTreeMap::new(),
        }
    }

    /// Start a new consensus instance
    ///
    /// Initiates a consensus operation following docs/104_consensus.md fast path:
    /// 1. Generate instance ID from prestate and operation
    /// 2. Initialize instance tracking
    /// 3. Broadcast Execute message to all witnesses
    ///
    /// # Arguments
    ///
    /// * `config` - Consensus configuration (threshold, witness set, timeout)
    /// * `prestate` - Current state snapshot
    /// * `operation` - Operation to reach consensus on
    ///
    /// # Returns
    ///
    /// Instance ID (cid) for tracking this consensus operation
    pub async fn start_consensus<T: Serialize>(
        &mut self,
        config: ConsensusConfig,
        prestate: Prestate,
        operation: &T,
    ) -> Result<Hash32> {
        let operation_hash = hash_operation(operation)?;
        let instance_id = prestate.bind_operation(operation);

        let instance = ConsensusInstance {
            config,
            prestate,
            operation_hash,
            collected_shares: Vec::new(),
            decided: false,
        };

        self.instances.insert(instance_id, instance);

        // TODO: Full protocol initiation requires:
        // 1. Broadcasting Execute(cid, Op, prestate_hash) to all witnesses
        // 2. Setting up timeout handler for fallback gossip
        // 3. Registering share collection handler
        //
        // This will be integrated with aura-protocol orchestrator and
        // choreographic effects for witness communication.

        Ok(instance_id)
    }
}

impl Default for ConsensusCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consensus_config() {
        let witnesses = vec![AuthorityId::new(), AuthorityId::new(), AuthorityId::new()];

        let config = ConsensusConfig::new(2, witnesses.clone());

        // Create mock witness shares (using zero partial signatures for testing)
        let partial_sig = PartialSignature(vec![0u8; 64]);
        let prestate_hash = Hash32([0u8; 32]);

        let shares = vec![
            WitnessShare {
                authority: witnesses[0],
                partial_signature: partial_sig.clone(),
                prestate_hash,
            },
            WitnessShare {
                authority: witnesses[1],
                partial_signature: partial_sig.clone(),
                prestate_hash,
            },
        ];

        // Test threshold checking
        assert!(config.check_threshold(&shares));

        // Test with insufficient shares
        let insufficient = vec![shares[0].clone()];
        assert!(!config.check_threshold(&insufficient));

        // Test prestate agreement
        assert!(config.verify_prestate_agreement(&shares));

        // Test prestate disagreement
        let mut mismatched = shares.clone();
        mismatched[1].prestate_hash = Hash32([1u8; 32]);
        assert!(!config.verify_prestate_agreement(&mismatched));
    }

    #[tokio::test]
    async fn test_consensus_protocol() {
        let auth = AuthorityId::new();
        let prestate = Prestate::new(vec![(auth, Hash32::default())], Hash32::default());

        #[derive(Serialize)]
        struct TestOp {
            value: String,
        }

        let op = TestOp {
            value: "test".to_string(),
        };

        let proof = run_consensus(&prestate, &op).await.unwrap();

        // Verify proof structure
        assert_eq!(proof.prestate_hash, prestate.compute_hash());
        assert!(!proof.threshold_met); // No actual witnesses yet
        assert!(proof.threshold_signature.is_none()); // No FROST aggregation yet
        assert!(proof.attester_set.is_empty()); // No witnesses provided shares
    }
}
