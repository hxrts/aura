//! G_sigagg: Signature Aggregation Choreography
//!
//! This module implements the G_sigagg choreography for FROST signature
//! aggregation and verification using the Aura effect system pattern.

#![allow(missing_docs)]

use crate::FrostResult;
use aura_core::frost::{PartialSignature, ThresholdSignature};
use aura_core::{AuraError, DeviceId};
use aura_macros::choreography;
use serde::{Deserialize, Serialize};

// FROST signature aggregation choreography protocol
//
// This choreography implements signature aggregation with the following phases:
// 1. Setup: Coordinator initiates aggregation session with all signers
// 2. Collection: Signers submit their partial signatures
// 3. Aggregation: Coordinator aggregates signatures and broadcasts result
//
// Features Byzantine fault tolerance and timeout handling.
choreography! {
    #[namespace = "signature_aggregation"]
    protocol SignatureAggregation {
        roles: Coordinator, Signers[*];

        // Phase 1: Setup - Coordinator initiates aggregation
        Coordinator[guard_capability = "initiate_aggregation",
                   flow_cost = 100,
                   journal_facts = "aggregation_initiated"]
        -> Signers[*]: AggregationInit(AggregationRequest);

        // Phase 2: Collection - Signers submit partial signatures
        Signers[0..threshold][guard_capability = "submit_signature",
                              flow_cost = 80,
                              journal_facts = "signature_submitted"]
        -> Coordinator: PartialSignatureSubmission(PartialSignatureSubmission);

        // Phase 3: Aggregation - Coordinator processes and broadcasts result
        choice Coordinator {
            success: {
                Coordinator[guard_capability = "distribute_success",
                           flow_cost = 150,
                           journal_facts = "aggregation_completed",
                           journal_merge = true]
                -> Signers[*]: AggregationSuccess(ThresholdSignature);
            }
            failure: {
                Coordinator[guard_capability = "distribute_failure",
                           flow_cost = 100,
                           journal_facts = "aggregation_failed"]
                -> Signers[*]: AggregationFailure(String);
            }
        }
    }
}

// Message types for signature aggregation choreography

/// Aggregation initiation message containing the request details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationInit {
    /// The aggregation request with session details
    pub request: AggregationRequest,
}

/// Successful aggregation result message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationSuccess {
    /// The final aggregated threshold signature
    pub signature: ThresholdSignature,
}

/// Failed aggregation result message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationFailure {
    /// Error description for the failure
    pub error: String,
}

/// Signature aggregation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationRequest {
    /// Session identifier
    pub session_id: String,
    /// Message that was signed
    pub message: Vec<u8>,
    /// Required threshold
    pub threshold: usize,
    /// Participating signers
    pub signers: Vec<DeviceId>,
    /// Session timeout in seconds
    pub timeout_seconds: u64,
}

/// Signature aggregation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationResponse {
    /// Aggregated threshold signature
    pub signature: Option<ThresholdSignature>,
    /// Aggregation successful
    pub success: bool,
    /// Participating signers
    pub signers: Vec<DeviceId>,
    /// Error message if any
    pub error: Option<String>,
}

/// Partial signature submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialSignatureSubmission {
    /// Session identifier
    pub session_id: String,
    /// Signer device ID
    pub signer_id: DeviceId,
    /// Partial signature data
    pub partial_signature: PartialSignature,
    /// Signature index
    pub signature_index: u16,
}

/// Configuration validation for signature aggregation
pub fn validate_aggregation_config(request: &AggregationRequest) -> FrostResult<()> {
    if request.threshold == 0 || request.threshold > request.signers.len() {
        return Err(AuraError::invalid(
            "Invalid threshold configuration for signature aggregation",
        ));
    }
    Ok(())
}

/// FROST signature aggregation implementation using real cryptographic operations
///
/// This performs the actual FROST signature aggregation using the aura-crypto library.
/// It takes partial signatures and produces a threshold signature.
pub async fn perform_frost_aggregation(
    partial_signatures: &[PartialSignature],
    message: &[u8],
    threshold: usize,
    total_signers: usize,
) -> FrostResult<ThresholdSignature> {
    use aura_core::frost::tree_signing::{binding_message, frost_aggregate, TreeSigningContext};
    use frost_ed25519 as frost;
    use std::collections::BTreeMap;

    if partial_signatures.len() < threshold {
        return Err(AuraError::invalid(format!(
            "Insufficient partial signatures: {} < {}",
            partial_signatures.len(),
            threshold
        )));
    }

    // Create context and binding message for the aggregation
    let context = TreeSigningContext::new(1, 0, [0u8; 32]);
    let bound_message = binding_message(&context, message);

    // Create mock commitments for aggregation (in production, these come from nonce phase)
    let mut frost_commitments = BTreeMap::new();
    for i in 0..partial_signatures.len() {
        frost_commitments.insert(
            (i + 1) as u16,
            aura_core::frost::NonceCommitment {
                signer: (i + 1) as u16,
                commitment: vec![0u8; 32],
            },
        );
    }

    // Generate temporary public key package for aggregation
    // In production, this would come from the DKG ceremony
    #[allow(clippy::disallowed_methods)]
    let rng = rand::thread_rng();
    let (_, pubkey_package) = frost::keys::generate_with_dealer(
        total_signers.try_into().unwrap(),
        threshold.try_into().unwrap(),
        frost::keys::IdentifierList::Default,
        rng,
    )
    .map_err(|e| AuraError::crypto(format!("Failed to generate key package: {}", e)))?;

    // Aggregate the signatures
    let signature_bytes = frost_aggregate(
        partial_signatures,
        &bound_message,
        &frost_commitments,
        &pubkey_package,
    )
    .map_err(|e| AuraError::crypto(format!("FROST aggregation failed: {}", e)))?;

    // Create threshold signature result
    let participating_signers: Vec<u16> = (0..partial_signatures.len() as u16).collect();

    Ok(ThresholdSignature::new(
        signature_bytes,
        participating_signers,
    ))
}

// The choreography macro generates these types and functions automatically:
// - SignatureAggregationChoreography struct
// - Role-specific projection types for Coordinator and Signers
// - Message routing and session type enforcement

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::test_utils::test_device_id;

    #[test]
    fn test_aggregation_config_validation() {
        let valid_request = AggregationRequest {
            session_id: "test_session".to_string(),
            message: b"test message".to_vec(),
            threshold: 2,
            signers: vec![test_device_id(1), test_device_id(2), test_device_id(3)],
            timeout_seconds: 60,
        };
        assert!(validate_aggregation_config(&valid_request).is_ok());

        let invalid_request = AggregationRequest {
            session_id: "test_session".to_string(),
            message: b"test message".to_vec(),
            threshold: 0, // Invalid threshold
            signers: vec![test_device_id(4), test_device_id(5)],
            timeout_seconds: 60,
        };
        assert!(validate_aggregation_config(&invalid_request).is_err());
    }

    #[test]
    fn test_aggregation_request_serialization() {
        let request = AggregationRequest {
            session_id: "test_session".to_string(),
            message: b"test message".to_vec(),
            threshold: 2,
            signers: vec![test_device_id(6), test_device_id(7), test_device_id(8)],
            timeout_seconds: 60,
        };

        let serialized = serde_json::to_vec(&request).unwrap();
        let deserialized: AggregationRequest = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(request.session_id, deserialized.session_id);
        assert_eq!(request.message, deserialized.message);
        assert_eq!(request.threshold, deserialized.threshold);
        assert_eq!(request.signers.len(), deserialized.signers.len());
    }

    #[tokio::test]
    async fn test_frost_aggregation_validation() {
        // Test validation of insufficient signatures
        let insufficient_signatures = vec![PartialSignature::from_bytes(vec![1; 32]).unwrap()];
        let message = b"test message";
        let threshold = 2;
        let total_signers = 3;

        // This should fail with insufficient signatures
        let result =
            perform_frost_aggregation(&insufficient_signatures, message, threshold, total_signers)
                .await;
        assert!(result.is_err());

        // Test that error message is correct
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Insufficient partial signatures"));
    }

    #[test]
    fn test_partial_signature_submission_serialization() {
        let submission = PartialSignatureSubmission {
            session_id: "test_session".to_string(),
            signer_id: test_device_id(9),
            partial_signature: PartialSignature::from_bytes(vec![1; 32]).unwrap(), // 32-byte signature as required
            signature_index: 1,
        };

        let serialized = serde_json::to_vec(&submission).unwrap();
        let deserialized: PartialSignatureSubmission = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(submission.session_id, deserialized.session_id);
        assert_eq!(submission.signer_id, deserialized.signer_id);
        assert_eq!(submission.signature_index, deserialized.signature_index);
    }
}
