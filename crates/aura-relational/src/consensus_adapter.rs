//! Consensus adapter for relational contexts
//!
//! This module provides a thin adapter that delegates consensus operations
//! to the aura-protocol implementation while maintaining the aura-relational
//! API surface for backward compatibility.

use aura_core::{relational::ConsensusProof, AuraError, AuthorityId, Prestate, Result};
use aura_protocol::consensus::{
    run_relational_consensus, run_relational_consensus_with_config, RelationalConsensusConfig,
};
use serde::Serialize;

/// Consensus configuration for relational contexts
///
/// Re-exported from aura-protocol for API compatibility
pub use aura_protocol::consensus::RelationalConsensusConfig as ConsensusConfig;

/// Run consensus on an operation for relational contexts
///
/// This function delegates to the aura-protocol consensus implementation,
/// providing a stable API for relational context operations while the
/// actual consensus logic lives in the orchestration layer.
pub async fn run_consensus<T: Serialize>(
    prestate: &Prestate,
    operation: &T,
) -> Result<ConsensusProof> {
    run_relational_consensus(prestate, operation).await
}

/// Run consensus with explicit configuration for relational contexts
///
/// This provides fine-grained control over consensus parameters while
/// delegating to the aura-protocol implementation.
pub async fn run_consensus_with_config<T: Serialize>(
    prestate: &Prestate,
    operation: &T,
    config: ConsensusConfig,
) -> Result<ConsensusProof> {
    run_relational_consensus_with_config(prestate, operation, config).await
}

/// Create a failed consensus proof for testing or error scenarios
///
/// This provides a stable API for creating failed proofs without exposing
/// the internal consensus implementation details.
pub fn create_failed_proof(
    prestate_hash: aura_core::Hash32,
    operation_hash: aura_core::Hash32,
    attester_set: Vec<AuthorityId>,
) -> ConsensusProof {
    ConsensusProof::failed(prestate_hash, operation_hash, attester_set)
}

/// Validate consensus configuration before use
///
/// This ensures the configuration is valid before attempting consensus
/// operations, providing early error detection.
pub fn validate_config(config: &ConsensusConfig) -> Result<()> {
    if config.witness_set.is_empty() {
        return Err(AuraError::invalid(
            "Consensus requires at least one witness",
        ));
    }

    if !config.has_quorum() {
        return Err(AuraError::invalid(
            "Consensus threshold exceeds witness set size",
        ));
    }

    // Validate reasonable timeout bounds
    if config.timeout_ms > 300000 {
        return Err(AuraError::invalid(
            "Consensus timeout exceeds maximum (5 minutes)",
        ));
    }

    if config.timeout_ms < 1000 {
        return Err(AuraError::invalid(
            "Consensus timeout below minimum (1 second)",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{AuthorityId, Hash32, Prestate};

    #[tokio::test]
    async fn test_consensus_adapter_delegation() {
        let auth = AuthorityId::new();
        let prestate = Prestate::new(vec![(auth, Hash32::default())], Hash32::default());

        #[derive(serde::Serialize)]
        struct TestOp {
            value: String,
        }

        let op = TestOp {
            value: "test_operation".to_string(),
        };

        // Test basic consensus delegation
        let proof = run_consensus(&prestate, &op).await.unwrap();
        assert_eq!(proof.prestate_hash, prestate.compute_hash());
        assert!(proof.threshold_met());
    }

    #[test]
    fn test_config_validation() {
        // Test valid config
        let config = ConsensusConfig::new(1, vec![AuthorityId::new()]);
        assert!(validate_config(&config).is_ok());

        // Test empty witness set
        let config = ConsensusConfig::new(1, vec![]);
        assert!(validate_config(&config).is_err());

        // Test threshold too high
        let config = ConsensusConfig::new(5, vec![AuthorityId::new()]);
        assert!(validate_config(&config).is_err());

        // Test timeout too high
        let mut config = ConsensusConfig::new(1, vec![AuthorityId::new()]);
        config.timeout_ms = 400000;
        assert!(validate_config(&config).is_err());

        // Test timeout too low
        let mut config = ConsensusConfig::new(1, vec![AuthorityId::new()]);
        config.timeout_ms = 500;
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_failed_proof_creation() {
        let prestate_hash = Hash32::default();
        let operation_hash = Hash32([1u8; 32]);
        let attesters = vec![AuthorityId::new()];

        let proof = create_failed_proof(prestate_hash, operation_hash, attesters.clone());

        assert_eq!(proof.prestate_hash, prestate_hash);
        assert_eq!(proof.operation_hash, operation_hash);
        assert!(!proof.threshold_met());
        assert_eq!(proof.attesters(), &attesters);
    }
}