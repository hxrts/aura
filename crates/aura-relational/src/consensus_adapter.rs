//! Consensus adapter for relational contexts
//!
//! This module provides a thin adapter that delegates consensus operations
//! to the aura-consensus implementation while maintaining the aura-relational
//! API surface for backward compatibility.

use aura_core::{
    epochs::Epoch,
    frost::{PublicKeyPackage, Share},
};
use aura_core::{relational::ConsensusProof, AuraError, AuthorityId, Prestate, Result};
use aura_effects::random::RealRandomHandler;
use aura_effects::time::PhysicalTimeHandler;
use aura_consensus::relational::{
    run_consensus as run_relational_consensus,
    run_consensus_with_config as run_relational_consensus_with_config,
};
use serde::Serialize;
use std::collections::HashMap;

/// Consensus configuration for relational contexts
///
/// Re-exported from aura-consensus for API compatibility
pub use aura_consensus::types::ConsensusConfig;

/// Run consensus on an operation for relational contexts
///
/// This function delegates to the aura-consensus implementation,
/// providing a stable API for relational context operations while the
/// actual consensus logic lives in the orchestration layer.
pub async fn run_consensus<T: Serialize>(
    prestate: &Prestate,
    operation: &T,
    key_packages: HashMap<AuthorityId, Share>,
    group_public_key: PublicKeyPackage,
    epoch: Epoch,
) -> Result<ConsensusProof> {
    let random = RealRandomHandler;
    let time = PhysicalTimeHandler;
    run_relational_consensus(
        prestate,
        operation,
        key_packages,
        group_public_key,
        epoch,
        &random,
        &time,
    )
    .await
}

/// Run consensus with explicit configuration for relational contexts
///
/// This provides fine-grained control over consensus parameters while
/// delegating to the aura-consensus implementation.
pub async fn run_consensus_with_config<T: Serialize>(
    prestate: &Prestate,
    operation: &T,
    config: ConsensusConfig,
    key_packages: HashMap<AuthorityId, Share>,
    group_public_key: PublicKeyPackage,
) -> Result<ConsensusProof> {
    let random = RealRandomHandler;
    let time = PhysicalTimeHandler;
    run_relational_consensus_with_config(
        prestate,
        operation,
        config,
        key_packages,
        group_public_key,
        &random,
        &time,
    )
    .await
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
        let auth = AuthorityId::new_from_entropy([54u8; 32]);
        let prestate = Prestate::new(vec![(auth, Hash32::default())], Hash32::default());

        #[derive(serde::Serialize)]
        struct TestOp {
            value: String,
        }

        let op = TestOp {
            value: "test_operation".to_string(),
        };

        // Test that the adapter interface works by testing a failing consensus scenario
        // This tests the adapter without requiring a full consensus implementation
        let key_packages = HashMap::new(); // Intentionally empty to trigger controlled failure
        let group_pk_bytes = aura_core::hash::hash(b"relational-consensus-test-key").to_vec();
        let group_public_key = PublicKeyPackage::new(
            group_pk_bytes,
            std::collections::BTreeMap::new(), // empty signer keys for test
            1,                                 // minimal threshold
            1,                                 // minimal max signers
        );
        let epoch = Epoch::from(1);

        // The adapter should handle the consensus failure gracefully
        let result = run_consensus(&prestate, &op, key_packages, group_public_key, epoch).await;

        // We expect this to fail due to insufficient nonce commitments, which tests the error handling
        assert!(
            result.is_err(),
            "Expected consensus to fail with empty key packages"
        );

        // Verify we get the expected error type
        if let Err(error) = result {
            assert!(
                error.to_string().contains("nonce commitments"),
                "Expected error about nonce commitments, got: {}",
                error
            );
        }
    }

    #[test]
    fn test_config_validation() {
        // Test valid config
        let config = ConsensusConfig::new(
            1,
            vec![AuthorityId::new_from_entropy([55u8; 32])],
            Epoch::from(1),
        )
        .unwrap();
        assert!(validate_config(&config).is_ok());

        // Test empty witness set
        let config = ConsensusConfig::new(1, vec![], Epoch::from(1));
        assert!(config.is_err());

        // Test threshold too high
        let config = ConsensusConfig::new(
            5,
            vec![AuthorityId::new_from_entropy([56u8; 32])],
            Epoch::from(1),
        );
        assert!(config.is_err());

        // Test timeout too high
        let mut config = ConsensusConfig::new(
            1,
            vec![AuthorityId::new_from_entropy([57u8; 32])],
            Epoch::from(1),
        )
        .unwrap();
        config.timeout_ms = 400000;
        assert!(validate_config(&config).is_err());

        // Test timeout too low
        let mut config = ConsensusConfig::new(
            1,
            vec![AuthorityId::new_from_entropy([58u8; 32])],
            Epoch::from(1),
        )
        .unwrap();
        config.timeout_ms = 500;
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_failed_proof_creation() {
        let prestate_hash = Hash32::default();
        let operation_hash = Hash32([1u8; 32]);
        let attesters = vec![AuthorityId::new_from_entropy([59u8; 32])];

        let proof = create_failed_proof(prestate_hash, operation_hash, attesters.clone());

        assert_eq!(proof.prestate_hash, prestate_hash);
        assert_eq!(proof.operation_hash, operation_hash);
        assert!(!proof.threshold_met());
        assert_eq!(proof.attesters(), &attesters);
    }
}
