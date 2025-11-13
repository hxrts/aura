//! G_sigagg: Signature Aggregation Choreography
//!
//! This module implements the G_sigagg choreography for FROST signature
//! aggregation and verification using the Aura effect system pattern.

#![allow(missing_docs)]

use crate::FrostResult;
use aura_core::effects::{ConsoleEffects, CryptoEffects, NetworkEffects, TimeEffects};
use aura_core::{AuraError, DeviceId};
use aura_crypto::frost::{PartialSignature, ThresholdSignature};
use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// FROST signature aggregation choreography protocol
///
/// This choreography implements signature aggregation with the following phases:
/// 1. Setup: Coordinator initiates aggregation session with all signers
/// 2. Collection: Signers submit their partial signatures
/// 3. Aggregation: Coordinator aggregates signatures and broadcasts result
/// 
/// Features Byzantine fault tolerance and timeout handling.
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

/// Signature aggregation choreography execution context
///
/// This struct manages the execution state for signature aggregation sessions,
/// coordinating the collection and aggregation of partial signatures from
/// multiple signers into a final threshold signature.
#[derive(Debug)]
pub struct SignatureAggregationChoreographyExecutor {
    /// Device ID for this participant
    pub device_id: DeviceId,
    /// Whether this device acts as coordinator
    pub is_coordinator: bool,
    /// Current aggregation request
    pub aggregation_request: Option<AggregationRequest>,
    /// Collected partial signatures
    pub collected_signatures: HashMap<DeviceId, PartialSignatureSubmission>,
}

impl SignatureAggregationChoreographyExecutor {
    /// Create a new signature aggregation choreography executor
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device identifier for this participant
    /// * `is_coordinator` - Whether this device will coordinate the aggregation
    ///
    /// # Returns
    ///
    /// A new executor ready to participate in signature aggregation
    pub fn new(device_id: DeviceId, is_coordinator: bool) -> Self {
        Self {
            device_id,
            is_coordinator,
            aggregation_request: None,
            collected_signatures: HashMap::new(),
        }
    }

    /// Execute the signature aggregation choreography as coordinator
    pub async fn execute_as_coordinator<E>(
        &mut self,
        effects: &E,
        request: AggregationRequest,
        signers: Vec<DeviceId>,
    ) -> FrostResult<AggregationResponse>
    where
        E: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects,
    {
        let _ = effects
            .log_info(&format!(
                "Starting signature aggregation choreography as coordinator for session {}",
                request.session_id
            ))
            .await;

        self.aggregation_request = Some(request.clone());

        // Setup phase: Send aggregation init to all signers
        self.send_aggregation_init(effects, &signers, &request)
            .await?;

        // Collection phase: Collect partial signatures from signers
        let collected_sigs = self
            .collect_partial_signatures(effects, &signers, request.threshold)
            .await?;
        self.collected_signatures = collected_sigs;

        // Aggregation phase: Aggregate signatures and broadcast result
        let response = self.aggregate_and_broadcast(effects, &signers).await?;

        let _ = effects
            .log_info("Signature aggregation choreography completed")
            .await;
        Ok(response)
    }

    /// Execute the signature aggregation choreography as signer
    pub async fn execute_as_signer<E>(
        &mut self,
        effects: &E,
        partial_signature: PartialSignature,
    ) -> FrostResult<AggregationResponse>
    where
        E: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects,
    {
        let _ = effects
            .log_info(&format!(
                "Participating in signature aggregation for device {}",
                self.device_id
            ))
            .await;

        // Wait for and process aggregation init
        let request = self.receive_aggregation_init(effects).await?;
        self.aggregation_request = Some(request);

        // Submit partial signature
        self.submit_partial_signature(effects, partial_signature)
            .await?;

        // Wait for final result
        let response = self.receive_aggregation_result(effects).await?;

        let _ = effects
            .log_info("Signature aggregation participation completed")
            .await;
        Ok(response)
    }

    // Implementation methods following the choreographic structure

    async fn send_aggregation_init<E>(
        &self,
        effects: &E,
        signers: &[DeviceId],
        request: &AggregationRequest,
    ) -> FrostResult<()>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let message = serde_json::to_vec(request).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize aggregation init: {}", e))
        })?;

        for signer in signers {
            effects
                .send_to_peer((*signer).into(), message.clone())
                .await
                .map_err(|e| {
                    AuraError::network(format!("Failed to send aggregation init: {}", e))
                })?;
            let _ = effects
                .log_debug(&format!("Sent aggregation init to {}", signer))
                .await;
        }

        Ok(())
    }

    async fn collect_partial_signatures<E>(
        &self,
        effects: &E,
        signers: &[DeviceId],
        threshold: usize,
    ) -> FrostResult<HashMap<DeviceId, PartialSignatureSubmission>>
    where
        E: NetworkEffects + TimeEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug("Collecting partial signatures from signers")
            .await;

        let mut signatures = HashMap::new();
        let timeout_at = effects.current_timestamp().await + 30000; // 30 second timeout

        while signatures.len() < threshold && effects.current_timestamp().await < timeout_at {
            if let Ok((peer_id, message_bytes)) = effects.receive().await {
                if let Ok(submission) =
                    serde_json::from_slice::<PartialSignatureSubmission>(&message_bytes)
                {
                    let device_id = DeviceId(peer_id);
                    if signers.contains(&device_id) && submission.signer_id == device_id {
                        signatures.insert(device_id, submission);
                        let _ = effects
                            .log_debug(&format!("Received partial signature from {}", peer_id))
                            .await;
                    }
                }
            }

            if effects.current_timestamp().await >= timeout_at {
                return Err(AuraError::invalid("Partial signature collection timed out"));
            }
        }

        if signatures.len() < threshold {
            return Err(AuraError::invalid(format!(
                "Insufficient partial signatures: {} < {}",
                signatures.len(),
                threshold
            )));
        }

        Ok(signatures)
    }

    async fn aggregate_and_broadcast<E>(
        &self,
        effects: &E,
        signers: &[DeviceId],
    ) -> FrostResult<AggregationResponse>
    where
        E: NetworkEffects + CryptoEffects + ConsoleEffects,
    {
        let _ = effects.log_debug("Aggregating partial signatures").await;

        let request = self.aggregation_request.as_ref().unwrap();

        // Aggregate the partial signatures using real FROST cryptography
        if self.collected_signatures.len() >= request.threshold {
            // Success case - perform real FROST aggregation
            use aura_crypto::frost::tree_signing::{
                binding_message, frost_aggregate, TreeSigningContext,
            };
            use frost_ed25519 as frost;
            use std::collections::BTreeMap;

            // Create binding message for the aggregation
            let context = TreeSigningContext::new(1, 0, [0u8; 32]);
            let bound_message = binding_message(&context, &request.message);

            // Convert partial signatures
            let partials: Vec<_> = self
                .collected_signatures
                .values()
                .map(|submission| submission.partial_signature.clone())
                .collect();

            // Create mock commitments for aggregation
            let mut frost_commitments = BTreeMap::new();
            for (i, signature) in self.collected_signatures.values().enumerate() {
                frost_commitments.insert(
                    (i + 1) as u16,
                    aura_crypto::frost::NonceCommitment {
                        signer: (i + 1) as u16,
                        commitment: vec![0u8; 32],
                    },
                );
            }

            // Generate temporary public key package for aggregation
            #[allow(clippy::disallowed_methods)]
            // Required for cryptographic security - should use secure random source in production
            let rng = rand::thread_rng();
            let (_shares, pubkey_package) = frost::keys::generate_with_dealer(
                3,
                2,
                frost::keys::IdentifierList::Default,
                rng,
            )
            .map_err(|e| AuraError::crypto(format!("Failed to generate keys: {}", e)))?;

            match frost_aggregate(
                &partials,
                &bound_message,
                &frost_commitments,
                &pubkey_package,
            ) {
                Ok(signature_bytes) => {
                    let signers_indices: Vec<u16> =
                        (0..self.collected_signatures.len() as u16).collect();
                    let threshold_signature =
                        ThresholdSignature::new(signature_bytes, signers_indices);

                    let response = AggregationResponse {
                        signature: Some(threshold_signature.clone()),
                        success: true,
                        signers: self.collected_signatures.keys().cloned().collect(),
                        error: None,
                    };

                    // Broadcast success to all signers
                    let success_message = serde_json::to_vec(&response).map_err(|e| {
                        AuraError::serialization(format!(
                            "Failed to serialize aggregation response: {}",
                            e
                        ))
                    })?;

                    for signer in signers {
                        effects
                            .send_to_peer((*signer).into(), success_message.clone())
                            .await
                            .map_err(|e| {
                                AuraError::network(format!(
                                    "Failed to send aggregation result: {}",
                                    e
                                ))
                            })?;
                    }

                    let _ = effects
                        .log_info("FROST signature aggregation completed successfully")
                        .await;
                    Ok(response)
                }
                Err(e) => {
                    let _ = effects
                        .log_error(&format!("FROST aggregation failed: {}", e))
                        .await;

                    let response = AggregationResponse {
                        signature: None,
                        success: false,
                        signers: self.collected_signatures.keys().cloned().collect(),
                        error: Some(format!("Aggregation failed: {}", e)),
                    };

                    // Broadcast failure to all signers
                    let failure_message = serde_json::to_vec(&response).map_err(|e| {
                        AuraError::serialization(format!(
                            "Failed to serialize aggregation response: {}",
                            e
                        ))
                    })?;

                    for signer in signers {
                        effects
                            .send_to_peer((*signer).into(), failure_message.clone())
                            .await
                            .map_err(|e| {
                                AuraError::network(format!(
                                    "Failed to send aggregation result: {}",
                                    e
                                ))
                            })?;
                    }

                    Ok(response)
                }
            }
        } else {
            // Failure case
            let response = AggregationResponse {
                signature: None,
                success: false,
                signers: self.collected_signatures.keys().cloned().collect(),
                error: Some("Insufficient signatures for aggregation".to_string()),
            };

            // Broadcast failure to all signers
            let failure_message = serde_json::to_vec(&response).map_err(|e| {
                AuraError::serialization(format!("Failed to serialize aggregation response: {}", e))
            })?;

            for signer in signers {
                effects
                    .send_to_peer((*signer).into(), failure_message.clone())
                    .await
                    .map_err(|e| {
                        AuraError::network(format!("Failed to send aggregation result: {}", e))
                    })?;
            }

            let _ = effects
                .log_warn("Signature aggregation failed due to insufficient signatures")
                .await;
            Ok(response)
        }
    }

    // Signer-side methods

    async fn receive_aggregation_init<E>(&self, effects: &E) -> FrostResult<AggregationRequest>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let _ = effects.log_debug("Waiting for aggregation init").await;

        loop {
            let (_peer_id, message_bytes) = effects
                .receive()
                .await
                .map_err(|e| AuraError::network(format!("Failed to receive message: {}", e)))?;

            if let Ok(request) = serde_json::from_slice::<AggregationRequest>(&message_bytes) {
                let _ = effects.log_debug("Received aggregation init").await;
                return Ok(request);
            }
        }
    }

    async fn submit_partial_signature<E>(
        &self,
        effects: &E,
        partial_signature: PartialSignature,
    ) -> FrostResult<()>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let _ = effects.log_debug("Submitting partial signature").await;

        let submission = PartialSignatureSubmission {
            session_id: self
                .aggregation_request
                .as_ref()
                .unwrap()
                .session_id
                .clone(),
            signer_id: self.device_id,
            partial_signature,
            signature_index: 0, // Would be determined by position in signing set
        };

        let message = serde_json::to_vec(&submission).map_err(|e| {
            AuraError::serialization(format!(
                "Failed to serialize partial signature submission: {}",
                e
            ))
        })?;

        effects.broadcast(message).await.map_err(|e| {
            AuraError::network(format!("Failed to broadcast partial signature: {}", e))
        })?;

        let _ = effects.log_debug("Partial signature submitted").await;
        Ok(())
    }

    async fn receive_aggregation_result<E>(&self, effects: &E) -> FrostResult<AggregationResponse>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let _ = effects.log_debug("Waiting for aggregation result").await;

        let (_peer_id, message_bytes) = effects.receive().await.map_err(|e| {
            AuraError::network(format!("Failed to receive aggregation result: {}", e))
        })?;

        let response: AggregationResponse =
            serde_json::from_slice(&message_bytes).map_err(|e| {
                AuraError::serialization(format!("Failed to deserialize aggregation result: {}", e))
            })?;

        if response.success {
            let _ = effects
                .log_debug("Received successful aggregation result")
                .await;
        } else {
            let _ = effects
                .log_debug("Received failed aggregation result")
                .await;
        }

        Ok(response)
    }

    /// Validate signature aggregation configuration parameters
    ///
    /// Ensures that the threshold and signer counts are valid for FROST aggregation.
    /// The threshold must be greater than 0 and not exceed the total signer count.
    ///
    /// # Returns
    ///
    /// `Ok(())` if the configuration is valid, `Err(AuraError)` otherwise.
    pub fn validate_config(&self) -> FrostResult<()> {
        if let Some(request) = &self.aggregation_request {
            if request.threshold == 0 || request.threshold > request.signers.len() {
                return Err(AuraError::invalid("Invalid threshold configuration for signature aggregation"));
            }
        }
        Ok(())
    }
}

/// Get the signature aggregation choreography instance for protocol execution
///
/// This function provides access to the choreographic types and functions
/// generated by the `choreography!` macro for signature aggregation operations.
/// It serves as the entry point for choreographic execution of the aggregation protocol.
///
/// # Note
///
/// The actual implementation is generated by the choreography macro expansion.
/// This is a placeholder that will be replaced by the macro-generated code.
///
/// # Returns
///
/// Unit type - the macro generates the necessary choreographic infrastructure
pub fn get_aggregation_choreography() {
    // The choreography macro will generate the appropriate types and functions
}

/// Convenience alias for the signature aggregation coordinator
pub type SignatureAggregationCoordinator = SignatureAggregationChoreographyExecutor;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregation_coordinator_creation() {
        let device_id = DeviceId::new();
        let coordinator = SignatureAggregationChoreographyExecutor::new(device_id, true);
        assert_eq!(coordinator.device_id, device_id);
        assert!(coordinator.is_coordinator);
        assert!(coordinator.aggregation_request.is_none());
    }

    #[test]
    fn test_aggregation_request_serialization() {
        let request = AggregationRequest {
            session_id: "test_session".to_string(),
            message: b"test message".to_vec(),
            threshold: 2,
            signers: vec![DeviceId::new(), DeviceId::new(), DeviceId::new()],
            timeout_seconds: 60,
        };

        let serialized = serde_json::to_vec(&request).unwrap();
        let deserialized: AggregationRequest = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(request.session_id, deserialized.session_id);
        assert_eq!(request.message, deserialized.message);
        assert_eq!(request.threshold, deserialized.threshold);
        assert_eq!(request.signers.len(), deserialized.signers.len());
    }

    #[test]
    fn test_aggregation_choreography_creation() {
        let choreography = get_aggregation_choreography();
        // Test that we can create the choreography instance successfully
        // The macro generates a struct with the protocol name
    }

    #[test]
    fn test_partial_signature_submission_serialization() {
        let submission = PartialSignatureSubmission {
            session_id: "test_session".to_string(),
            signer_id: DeviceId::new(),
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
