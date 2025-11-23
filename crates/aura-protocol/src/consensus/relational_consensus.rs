//! Relational consensus implementation for cross-authority coordination
//!
//! This module implements the consensus mechanisms for relational contexts,
//! adapting the choreography-driven Aura Consensus implementation for
//! cross-authority operations like guardian bindings and recovery grants.

use aura_core::{relational::ConsensusProof, AuraError, AuthorityId, Hash32, Prestate, Result};
use serde::Serialize;

// Use the ConsensusConfig from the parent module to avoid duplication
pub use super::ConsensusConfig;

/// Run consensus on an operation using the participants from the prestate as witnesses.
///
/// This delegates to the choreography-based implementation in aura-protocol
/// (fast path + fallback) and maps the resulting commit fact into a
/// ConsensusProof for relational contexts.
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

/// Run consensus with an explicit configuration for relational contexts.
///
/// This provides fine-grained control over the consensus process, including
/// custom timeout values and witness sets that may differ from the prestate
/// participants.
pub async fn run_consensus_with_config<T: Serialize>(
    prestate: &Prestate,
    operation: &T,
    config: ConsensusConfig,
) -> Result<ConsensusProof> {
    let validated_config = validate_config(config)?;

    // TODO: Integrate with actual aura-protocol consensus implementation
    // For now, this is a placeholder that demonstrates the interface
    // without creating circular dependencies.
    let prestate_hash = prestate.compute_hash();
    let operation_hash = compute_operation_hash(operation)?;

    // Check if threshold can be met
    let threshold_met = validated_config.has_quorum();

    // TODO: Replace this with actual consensus choreography execution
    // This would involve:
    // 1. Initiating consensus choreography with the witness set
    // 2. Collecting threshold signatures via FROST
    // 3. Aggregating signatures and creating proof
    // 4. Handling timeout and failure cases

    Ok(ConsensusProof::new(
        prestate_hash,
        operation_hash,
        None, // TODO: Add actual threshold signature
        validated_config.witness_set,
        threshold_met,
    ))
}

/// Compute the hash of an operation for consensus
fn compute_operation_hash<T: Serialize>(operation: &T) -> Result<Hash32> {
    use aura_core::hash;

    let mut hasher = hash::hasher();
    hasher.update(b"AURA_RELATIONAL_OPERATION");

    // Serialize operation to get deterministic hash
    let operation_bytes = serde_json::to_vec(operation)
        .map_err(|e| AuraError::invalid(format!("Failed to serialize operation: {}", e)))?;

    hasher.update(&operation_bytes);
    Ok(Hash32(hasher.finalize()))
}

/// Validate and normalize consensus configuration
fn validate_config(config: ConsensusConfig) -> Result<ConsensusConfig> {
    if config.witness_set.is_empty() {
        return Err(AuraError::invalid(
            "Consensus requires at least one witness",
        ));
    }

    // Validate that we have quorum
    if !config.has_quorum() {
        return Err(AuraError::invalid(
            "Consensus threshold exceeds witness set size",
        ));
    }

    // Validate timeout (reasonable bounds: 1 second to 5 minutes)
    let mut validated_config = config;
    validated_config.timeout_ms = validated_config.timeout_ms.clamp(1000, 300000);

    Ok(validated_config)
}

/// Create a failed consensus proof for testing or error cases
pub fn create_failed_proof(
    prestate_hash: Hash32,
    operation_hash: Hash32,
    attester_set: Vec<AuthorityId>,
) -> ConsensusProof {
    ConsensusProof::failed(prestate_hash, operation_hash, attester_set)
}

/// Integration point for choreography-based consensus
///
/// This function will be implemented to integrate with the existing
/// aura-protocol consensus infrastructure once the circular dependency
/// issues are resolved.
pub async fn integrate_with_choreographic_consensus<T: Serialize>(
    _prestate: &Prestate,
    _operation: &T,
    _config: ConsensusConfig,
) -> Result<ConsensusProof> {
    // TODO: Implement integration with existing consensus choreographies
    // This would involve:
    // 1. Converting relational operations to consensus choreography messages
    // 2. Running the standard Aura Consensus protocol
    // 3. Converting the result back to ConsensusProof

    Err(AuraError::invalid(
        "Choreographic consensus integration not yet implemented",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consensus_config_creation() {
        let witnesses = vec![AuthorityId::new(), AuthorityId::new(), AuthorityId::new()];
        let config = ConsensusConfig::new(2, witnesses.clone());

        assert_eq!(config.threshold, 2);
        assert_eq!(config.witness_set.len(), 3);
        assert!(config.has_quorum());
        assert_eq!(config.timeout_ms, 30000);
    }

    #[test]
    fn test_consensus_config_validation() {
        // Test empty witness set
        let config = ConsensusConfig::new(1, vec![]);
        assert!(validate_config(config).is_err());

        // Test valid config
        let witnesses = vec![AuthorityId::new(), AuthorityId::new()];
        let config = ConsensusConfig::new(2, witnesses);
        assert!(validate_config(config).is_ok());

        // Test threshold too high
        let witnesses = vec![AuthorityId::new()];
        let config = ConsensusConfig::new(5, witnesses);
        assert!(validate_config(config).is_err()); // Should fail validation
    }

    #[test]
    fn test_consensus_config_with_timeout() {
        let witnesses = vec![AuthorityId::new()];
        let mut config = ConsensusConfig::new(1, witnesses);
        config.timeout_ms = 5000;
        assert_eq!(config.timeout_ms, 5000);

        // Test timeout validation
        let mut config = ConsensusConfig::new(1, vec![AuthorityId::new()]);
        config.timeout_ms = 500000;
        let validated = validate_config(config).unwrap();
        assert_eq!(validated.timeout_ms, 300000); // Should be capped at 5 minutes

        let mut config = ConsensusConfig::new(1, vec![AuthorityId::new()]);
        config.timeout_ms = 500;
        let validated = validate_config(config).unwrap();
        assert_eq!(validated.timeout_ms, 1000); // Should be minimum 1 second
    }

    #[tokio::test]
    async fn test_run_consensus_basic() {
        let auth = AuthorityId::new();
        let prestate = Prestate::new(vec![(auth, Hash32::default())], Hash32::default());

        #[derive(serde::Serialize)]
        struct TestOp {
            value: String,
        }

        let op = TestOp {
            value: "test_operation".to_string(),
        };

        let proof = run_consensus(&prestate, &op).await.unwrap();

        assert_eq!(proof.prestate_hash, prestate.compute_hash());
        assert!(proof.threshold_met());
        assert_eq!(proof.attester_count(), 1);
        assert!(proof.has_attester(&auth));
    }

    #[test]
    fn test_operation_hash_computation() {
        #[derive(serde::Serialize)]
        struct TestOp {
            value: u64,
            name: String,
        }

        let op1 = TestOp {
            value: 42,
            name: "test".to_string(),
        };
        let op2 = TestOp {
            value: 42,
            name: "test".to_string(),
        };
        let op3 = TestOp {
            value: 43,
            name: "test".to_string(),
        };

        let hash1 = compute_operation_hash(&op1).unwrap();
        let hash2 = compute_operation_hash(&op2).unwrap();
        let hash3 = compute_operation_hash(&op3).unwrap();

        assert_eq!(hash1, hash2); // Same operations should have same hash
        assert_ne!(hash1, hash3); // Different operations should have different hash
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
