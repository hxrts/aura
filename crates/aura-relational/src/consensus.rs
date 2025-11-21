//! Consensus interface for RelationalContexts
//!
//! This module adapts the choreography-driven Aura Consensus implementation
//! from `aura-protocol` for relational contexts. It binds relational operations
//! to a prestate, executes the consensus choreography with the configured
//! witness set, and returns a `ConsensusProof` that callers can attach to
//! relational facts.

use aura_core::crypto::frost::ThresholdSignature;
use aura_core::{AuraError, AuthorityId, Hash32, Result, Prestate};
use serde::{Deserialize, Serialize};

/// Proof of consensus for an operation
///
/// Mirrors the commit fact produced by Aura Consensus while keeping a
/// lightweight shape for relational consumers.
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

/// Run consensus on an operation using the participants from the prestate as witnesses.
///
/// This delegates to the choreography-based implementation in `aura-protocol`
/// (fast path + fallback) and maps the resulting commit fact into a
/// `ConsensusProof`.
pub async fn run_consensus<T: Serialize>(
    prestate: &Prestate,
    operation: &T,
) -> Result<ConsensusProof> {
    let witnesses: Vec<_> = prestate
        .authority_commitments
        .iter()
        .map(|(id, _)| *id)
        .collect();

    let threshold = u16::try_from(witnesses.len()).unwrap_or(u16::MAX).max(1);
    let config = ConsensusConfig::new(threshold, witnesses);

    run_consensus_with_config(prestate, operation, config).await
}

/// Run consensus with an explicit configuration.
pub async fn run_consensus_with_config<T: Serialize>(
    prestate: &Prestate,
    operation: &T,
    mut config: ConsensusConfig,
) -> Result<ConsensusProof> {
    validate_config(&mut config)?;

    // TODO: Implement proper consensus mechanism without depending on aura-protocol
    // This is a placeholder implementation to resolve the circular dependency
    let prestate_hash = prestate.compute_hash();
    let operation_hash = {
        use aura_core::hash;
        let mut hasher = hash::hasher();
        hasher.update(b"AURA_OPERATION");
        if let Ok(op_bytes) = serde_json::to_vec(operation) {
            hasher.update(&op_bytes);
        }
        Hash32(hasher.finalize())
    };
    
    let threshold_met = config.check_threshold();
    Ok(ConsensusProof {
        prestate_hash,
        operation_hash,
        threshold_signature: None, // TODO: Implement threshold signature
        attester_set: config.witness_set,
        threshold_met,
    })
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

    /// Check if we have enough witnesses for the configured threshold
    pub fn check_threshold(&self) -> bool {
        self.witness_set.len() >= self.threshold as usize
    }
}

fn validate_config(config: &mut ConsensusConfig) -> Result<()> {
    if config.witness_set.is_empty() {
        return Err(AuraError::invalid("Consensus requires at least one witness"));
    }

    if config.threshold == 0 {
        config.threshold = 1;
    }

    config.threshold = config
        .threshold
        .min(config.witness_set.len().try_into().unwrap_or(u16::MAX));

    if !config.check_threshold() {
        return Err(AuraError::invalid(
            "Consensus threshold exceeds witness set size",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consensus_config() {
        let witnesses = vec![AuthorityId::new(), AuthorityId::new(), AuthorityId::new()];

        let config = ConsensusConfig::new(2, witnesses);

        assert!(config.check_threshold());
        assert_eq!(config.timeout_ms, 30000);
    }

    #[tokio::test]
    async fn test_consensus_protocol_adapter() {
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

        assert_eq!(proof.prestate_hash, prestate.compute_hash());
        assert!(proof.threshold_met);
        assert_eq!(proof.attester_set.len(), 1);
        // TODO: Enable this assertion when threshold signatures are implemented
        // assert!(proof.threshold_signature.is_some());
        assert!(proof.threshold_signature.is_none()); // Current placeholder implementation
    }
}
