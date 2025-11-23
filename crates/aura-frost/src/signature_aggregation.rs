//! G_sigagg: Signature Aggregation Choreography
//!
//! This module implements the G_sigagg choreography for FROST signature
//! aggregation and verification using the Aura effect system pattern.

#![allow(missing_docs)]

use crate::FrostResult;
use aura_core::frost::{
    NonceCommitment, PartialSignature, PublicKeyPackage, ThresholdSignature, TreeSigningContext,
};
use aura_core::{identifiers::AuthorityId, AuraError};
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
    pub signers: Vec<AuthorityId>,
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
    pub signers: Vec<AuthorityId>,
    /// Error message if any
    pub error: Option<String>,
}

/// Partial signature submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialSignatureSubmission {
    /// Session identifier
    pub session_id: String,
    /// Signer authority ID
    pub signer_id: AuthorityId,
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
    context: &TreeSigningContext,
    nonce_commitments: &std::collections::HashMap<AuthorityId, NonceCommitment>,
    public_key_package: &PublicKeyPackage,
) -> FrostResult<ThresholdSignature> {
    use aura_core::frost::tree_signing::{binding_message, frost_aggregate};
    use std::collections::BTreeMap;

    if partial_signatures.len() < public_key_package.threshold as usize {
        return Err(AuraError::invalid(format!(
            "Insufficient partial signatures: {} < {}",
            partial_signatures.len(),
            public_key_package.threshold
        )));
    }

    let bound_message = binding_message(context, message);

    let mut frost_commitments_by_u16 = BTreeMap::new();
    for (i, (authority, commitment)) in nonce_commitments.iter().enumerate() {
        // Map authority to u16 signer ID (simple index-based mapping for now)
        let signer_id = (i + 1) as u16;
        frost_commitments_by_u16.insert(signer_id, commitment.clone());
    }

    // Convert to frost_ed25519::keys::PublicKeyPackage for aggregation
    let frost_public_key_package: frost_ed25519::keys::PublicKeyPackage = public_key_package
        .clone()
        .try_into()
        .map_err(|e| AuraError::invalid(format!("Failed to convert public key package: {}", e)))?;

    // Aggregate the signatures
    let signature_bytes = frost_aggregate(
        partial_signatures,
        &bound_message,
        &frost_commitments_by_u16,
        &frost_public_key_package,
    )
    .map_err(|e| AuraError::crypto(format!("FROST aggregation failed: {}", e)))?;

    // Create threshold signature result
    let participating_signers: Vec<u16> = partial_signatures.iter().map(|p| p.signer).collect();

    Ok(ThresholdSignature::new(
        signature_bytes,
        participating_signers,
    ))
}

fn frost_commitments_from_nonce(
    commitments: &std::collections::HashMap<AuthorityId, NonceCommitment>,
) -> FrostResult<
    std::collections::BTreeMap<
        frost_ed25519::Identifier,
        frost_ed25519::round1::SigningCommitments,
    >,
> {
    use std::collections::BTreeMap;

    let mut frost_commitments = BTreeMap::new();
    for commitment in commitments.values() {
        let identifier = commitment
            .frost_identifier()
            .map_err(|e| AuraError::invalid(e))?;
        let signing_commitments = commitment.to_frost().map_err(|e| AuraError::invalid(e))?;
        frost_commitments.insert(identifier, signing_commitments);
    }

    Ok(frost_commitments)
}

// The choreography macro generates these types and functions automatically:
// - SignatureAggregationChoreography struct
// - Role-specific projection types for Coordinator and Signers
// - Message routing and session type enforcement

#[cfg(test)]
mod tests {
    use super::*;
    use crate::threshold_signing::{FrostCrypto, ThresholdSigningConfig};
    use aura_macros::aura_test;
    use aura_testkit::create_test_fixture;

    #[test]
    fn test_aggregation_config_validation() {
        let valid_request = AggregationRequest {
            session_id: "test_session".to_string(),
            message: b"test message".to_vec(),
            threshold: 2,
            signers: vec![
                aura_core::test_utils::test_authority_id(0),
                aura_core::test_utils::test_authority_id(1),
                aura_core::test_utils::test_authority_id(2),
            ],
            timeout_seconds: 60,
        };
        assert!(validate_aggregation_config(&valid_request).is_ok());

        let invalid_request = AggregationRequest {
            session_id: "test_session".to_string(),
            message: b"test message".to_vec(),
            threshold: 0, // Invalid threshold
            signers: vec![
                aura_core::test_utils::test_authority_id(3),
                aura_core::test_utils::test_authority_id(4),
            ],
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
            signers: vec![
                aura_core::test_utils::test_authority_id(0),
                aura_core::test_utils::test_authority_id(1),
                aura_core::test_utils::test_authority_id(2),
            ],
            timeout_seconds: 60,
        };

        let serialized = serde_json::to_vec(&request).unwrap();
        let deserialized: AggregationRequest = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(request.session_id, deserialized.session_id);
        assert_eq!(request.message, deserialized.message);
        assert_eq!(request.threshold, deserialized.threshold);
        assert_eq!(request.signers.len(), deserialized.signers.len());
    }

    #[aura_test]
    async fn test_frost_aggregation_choreography() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let effects = fixture.effect_system();

        let message = b"test message for threshold signing";
        let config = ThresholdSigningConfig::new(3, 4, 300);
        let context = TreeSigningContext::new(1, 0, [1u8; 32]);

        let authorities: Vec<_> = (0..config.total_signers)
            .map(|_| aura_core::AuthorityId::new())
            .collect();
        let key_material =
            FrostCrypto::generate_key_material(&authorities, &config, &*effects.random_effects())
                .await?;

        let mut nonce_commitments = std::collections::HashMap::new();
        let mut signer_nonces =
            std::collections::HashMap::<AuthorityId, frost_ed25519::round1::SigningNonces>::new();

        for authority in &authorities {
            let key_pkg = key_material
                .key_packages
                .get(authority)
                .expect("missing key package");
            let (nonces, commitment) =
                FrostCrypto::generate_nonce_commitment(key_pkg, &*effects.random_effects()).await?;
            signer_nonces.insert(*authority, nonces);
            nonce_commitments.insert(*authority, commitment);
        }

        let frost_commitments = frost_commitments_from_nonce(&nonce_commitments)?;

        let mut partial_signatures = Vec::new();
        for authority in authorities.iter().take(config.threshold) {
            let key_pkg = key_material
                .key_packages
                .get(authority)
                .expect("missing key package");
            let signing_nonces = signer_nonces
                .get(authority)
                .expect("missing nonces for signer");
            let partial_sig = FrostCrypto::generate_partial_signature(
                &context,
                message,
                key_pkg,
                signing_nonces,
                &frost_commitments,
                &*effects.random_effects(),
            )
            .await?;
            partial_signatures.push(partial_sig);
        }

        // Convert frost PublicKeyPackage to aura-core PublicKeyPackage
        let aura_public_key_package =
            PublicKeyPackage::from(key_material.public_key_package.clone());

        let result = perform_frost_aggregation(
            &partial_signatures,
            message,
            &context,
            &nonce_commitments,
            &aura_public_key_package,
        )
        .await?;

        assert_eq!(result.signers.len(), config.threshold);

        println!("✓ FROST aggregation choreography test completed");
        Ok(())
    }

    #[aura_test]
    async fn test_signature_aggregation_multi_round() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let effects = fixture.effect_system();

        for round in 1..=3 {
            let config = ThresholdSigningConfig::new(3 + round, 6, 120);
            let context = TreeSigningContext::new(round as u32, round as u64, [round as u8; 32]);

            let authorities: Vec<_> = (0..config.total_signers)
                .map(|_| aura_core::AuthorityId::new())
                .collect();
            let key_material = FrostCrypto::generate_key_material(
                &authorities,
                &config,
                &*effects.random_effects(),
            )
            .await?;

            let mut nonce_commitments = std::collections::HashMap::new();
            let mut signer_nonces = std::collections::HashMap::<
                AuthorityId,
                frost_ed25519::round1::SigningNonces,
            >::new();

            for authority in &authorities {
                let key_pkg = key_material
                    .key_packages
                    .get(authority)
                    .expect("missing key package");
                let (nonces, commitment) =
                    FrostCrypto::generate_nonce_commitment(key_pkg, &*effects.random_effects())
                        .await?;
                signer_nonces.insert(*authority, nonces);
                nonce_commitments.insert(*authority, commitment);
            }

            let frost_commitments = frost_commitments_from_nonce(&nonce_commitments)?;

            let mut partial_signatures = Vec::new();
            for authority in authorities.iter().take(config.threshold) {
                let key_pkg = key_material
                    .key_packages
                    .get(authority)
                    .expect("missing key package");
                let signing_nonces = signer_nonces
                    .get(authority)
                    .expect("missing nonces for signer");
                let partial_sig = FrostCrypto::generate_partial_signature(
                    &context,
                    b"round message",
                    key_pkg,
                    signing_nonces,
                    &frost_commitments,
                    &*effects.random_effects(),
                )
                .await?;
                partial_signatures.push(partial_sig);
            }

            // Convert frost PublicKeyPackage to aura-core PublicKeyPackage
            let aura_public_key_package =
                PublicKeyPackage::from(key_material.public_key_package.clone());

            let result = perform_frost_aggregation(
                &partial_signatures,
                b"round message",
                &context,
                &nonce_commitments,
                &aura_public_key_package,
            )
            .await?;

            assert_eq!(result.signers.len(), config.threshold);
        }

        println!("✓ Multi-round signature aggregation test completed");
        Ok(())
    }

    #[test]
    fn test_partial_signature_submission_serialization() {
        let submission = PartialSignatureSubmission {
            session_id: "test_session".to_string(),
            signer_id: aura_core::test_utils::test_authority_id(0),
            partial_signature: PartialSignature::from_bytes(vec![1; 32]).unwrap(),
            signature_index: 1,
        };

        let serialized = serde_json::to_vec(&submission).unwrap();
        let deserialized: PartialSignatureSubmission = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(submission.session_id, deserialized.session_id);
        assert_eq!(submission.signer_id, deserialized.signer_id);
        assert_eq!(submission.signature_index, deserialized.signature_index);
    }
}
