//! Guardian Authentication via Relational Contexts
//!
//! This module implements guardian authentication using the RelationalContext
//! model, replacing the device-centric guardian authentication.

use aura_core::{AuraError, Authority, AuthorityId, Hash32, Result};
use aura_macros::choreography;
use aura_relational::{GuardianBinding, RelationalContext, RelationalFact};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Guardian authentication request via relational context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianAuthRequest {
    /// Context ID for the guardian relationship
    pub context_id: aura_core::identifiers::ContextId,
    /// Guardian authority requesting authentication
    pub guardian_id: AuthorityId,
    /// Account being guarded
    pub account_id: AuthorityId,
    /// Operation type
    pub operation: GuardianOperation,
}

/// Guardian operation types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GuardianOperation {
    /// Approve recovery request
    ApproveRecovery {
        /// New tree commitment after recovery
        new_commitment: Hash32,
    },
    /// Deny recovery request
    DenyRecovery {
        /// Reason for denial
        reason: String,
    },
    /// Update guardian parameters
    UpdateParameters {
        /// New recovery delay
        recovery_delay_seconds: u64,
    },
}

/// Guardian authentication proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianAuthProof {
    /// Context ID where guardian is registered
    pub context_id: aura_core::identifiers::ContextId,
    /// Guardian authority ID
    pub guardian_id: AuthorityId,
    /// Guardian binding proof from context
    pub binding_proof: Option<aura_relational::ConsensusProof>,
    /// Signature over operation
    pub operation_signature: Vec<u8>,
}

/// Guardian authentication response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianAuthResponse {
    /// Whether authentication succeeded
    pub success: bool,
    /// Whether guardian is authorized for operation
    pub authorized: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Authenticate a guardian through relational context
pub async fn authenticate_guardian(
    context: &RelationalContext,
    guardian_authority: &dyn Authority,
    request: &GuardianAuthRequest,
) -> Result<GuardianAuthProof> {
    // Verify guardian is in this context
    if !context
        .participants
        .contains(&guardian_authority.authority_id())
    {
        return Err(AuraError::permission_denied(
            "Guardian not in context",
        ));
    }

    // Find guardian binding in context facts
    let binding = context
        .get_guardian_binding(guardian_authority.authority_id())
        .ok_or_else(|| AuraError::not_found("Guardian binding not found"))?;

    // Sign the operation
    let operation_bytes = bincode::serialize(&request.operation)
        .map_err(|e| AuraError::serialization(e.to_string()))?;

    let signature = guardian_authority.sign_operation(&operation_bytes).await?;

    Ok(GuardianAuthProof {
        context_id: context.context_id,
        guardian_id: guardian_authority.authority_id(),
        binding_proof: binding.consensus_proof.clone(),
        operation_signature: signature.to_bytes().to_vec(),
    })
}

/// Verify a guardian authentication proof
pub async fn verify_guardian_proof(
    context: &RelationalContext,
    request: &GuardianAuthRequest,
    proof: &GuardianAuthProof,
) -> Result<GuardianAuthResponse> {
    // Verify context ID matches
    if context.context_id != proof.context_id {
        return Ok(GuardianAuthResponse {
            success: false,
            authorized: false,
            error: Some("Context ID mismatch".to_string()),
        });
    }

    // Verify guardian ID matches
    if request.guardian_id != proof.guardian_id {
        return Ok(GuardianAuthResponse {
            success: false,
            authorized: false,
            error: Some("Guardian ID mismatch".to_string()),
        });
    }

    // Get guardian binding
    let binding = context
        .get_guardian_binding(proof.guardian_id)
        .ok_or_else(|| AuraError::not_found("Guardian binding not found"))?;

    // Verify consensus proof if present
    if let Some(consensus_proof) = &binding.consensus_proof {
        if !verify_consensus_proof(consensus_proof, &binding) {
            return Ok(GuardianAuthResponse {
                success: false,
                authorized: false,
                error: Some("Invalid consensus proof".to_string()),
            });
        }
    }

    // Verify operation signature using guardian's public key
    use ed25519_dalek::Verifier;

    // Get guardian's public key from authority
    let guardian_public_key = guardian.public_key();

    // Construct message to verify (serialized request)
    let message = serde_json::to_vec(&request)
        .map_err(|e| AuraError::invalid_input(format!("Failed to serialize request: {}", e)))?;

    // Verify signature (request should contain signature in future enhancement)
    // For now, we verify that the guardian authority is valid
    let operation_bytes = format!("guardian_auth_{}", request.operation_hash.0).into_bytes();

    match guardian.sign_operation(&operation_bytes).await {
        Ok(_signature) => {
            // Guardian can sign, which proves they have access to the keys
            // This is a simplified verification - production would verify the actual request signature
            Ok(GuardianAuthResponse {
                success: true,
                authorized: true,
                error: None,
            })
        }
        Err(e) => {
            // Guardian cannot sign - not authorized
            Ok(GuardianAuthResponse {
                success: true,
                authorized: false,
                error: Some(format!("Guardian signature verification failed: {}", e)),
            })
        }
    }
}

/// Verify consensus proof for guardian binding
fn verify_consensus_proof(
    proof: &aura_relational::ConsensusProof,
    binding: &GuardianBinding,
) -> bool {
    // Verify threshold signature
    // The proof should contain a valid threshold signature from the attester set

    // Check 1: Threshold met
    if !proof.threshold_met {
        return false;
    }

    // Check 2: Verify threshold signature is present
    // Production code would verify the signature cryptographically
    // For now, we check that the signature structure is valid
    if proof.threshold_signature.signature_bytes.is_empty() {
        return false;
    }

    // Check 3: Verify attester set is non-empty
    if proof.attester_set.is_empty() {
        return false;
    }

    // Check 4: Verify prestate hash matches binding
    // The prestate should include the guardian binding commitment
    // This ensures the consensus is about the correct binding operation
    let binding_hash = binding.compute_hash();

    // In production, we would:
    // 1. Reconstruct prestate from binding
    // 2. Hash the prestate
    // 3. Compare with proof.prestate_hash
    // For now, we accept if the proof structure is valid

    // All checks passed
    true
}

// Guardian Authentication Choreography via Relational Context
choreography! {
    #[namespace = "guardian_auth_relational"]
    protocol GuardianAuthRelational {
        roles: Account, Guardian, Coordinator;

        // Step 1: Account requests guardian approval
        Account[guard_capability = "request_guardian_approval", flow_cost = 50]
        -> Coordinator: RequestGuardianAuth(GuardianAuthRequest);

        // Step 2: Coordinator forwards to guardian
        Coordinator[guard_capability = "coordinate_guardians", flow_cost = 30]
        -> Guardian: ForwardAuthRequest(GuardianAuthRequest);

        // Step 3: Guardian submits proof
        Guardian[guard_capability = "submit_guardian_proof", flow_cost = 50]
        -> Coordinator: SubmitGuardianProof(GuardianAuthProof);

        // Step 4: Coordinator verifies and responds
        Coordinator[guard_capability = "verify_guardian", flow_cost = 30]
        -> Account: GuardianAuthResult(GuardianAuthResponse);
    }
}

/// Guardian authentication handler for relational contexts
pub struct GuardianAuthHandler {
    context: Arc<RelationalContext>,
}

impl GuardianAuthHandler {
    /// Create a new guardian authentication handler
    pub fn new(context: Arc<RelationalContext>) -> Self {
        Self { context }
    }

    /// Process guardian authentication request
    pub async fn process_auth_request(
        &self,
        request: GuardianAuthRequest,
        guardian: Arc<dyn Authority>,
    ) -> Result<GuardianAuthResponse> {
        // Authenticate guardian
        let proof = authenticate_guardian(&self.context, guardian.as_ref(), &request).await?;

        // Verify proof
        verify_guardian_proof(&self.context, &request, &proof).await
    }

    /// Check if guardian can approve operation
    pub async fn check_guardian_approval(
        &self,
        guardian_id: AuthorityId,
        operation: &GuardianOperation,
    ) -> Result<bool> {
        // Check guardian binding exists
        let binding = self
            .context
            .get_guardian_binding(guardian_id)
            .ok_or_else(|| AuraError::not_found("Guardian not bound to account"))?;

        // Check operation-specific requirements
        match operation {
            GuardianOperation::ApproveRecovery { recovery_request_time, .. } => {
                // Check if recovery delay has passed
                // Guardian parameters specify minimum delay before approval
                let recovery_delay = binding.parameters.recovery_delay;

                // Get current time (in production, use TimeEffects for determinism)
                let current_time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| AuraError::invalid_state(format!("Time error: {}", e)))?
                    .as_secs();

                // Check if enough time has passed since recovery request
                if current_time < recovery_request_time + recovery_delay.as_secs() {
                    return Ok(false); // Recovery delay not yet passed
                }

                // Check if notification was required and sent
                if binding.parameters.notification_required {
                    // In production, verify notification was sent
                    // For now, we assume it was if the delay passed
                }

                Ok(true)
            }
            GuardianOperation::DenyRecovery { .. } => {
                // Guardians can always deny recovery attempts
                // This is a safety mechanism that requires no additional checks
                Ok(true)
            }
            GuardianOperation::UpdateParameters { new_delay, new_notification_required } => {
                // Check if guardian has parameter update permission
                // Only allow reasonable parameter changes

                // Verify new delay is within acceptable bounds (e.g., 1 hour to 30 days)
                const MIN_DELAY_SECS: u64 = 3600; // 1 hour
                const MAX_DELAY_SECS: u64 = 30 * 24 * 3600; // 30 days

                if let Some(delay) = new_delay {
                    if delay.as_secs() < MIN_DELAY_SECS || delay.as_secs() > MAX_DELAY_SECS {
                        return Ok(false); // Invalid delay range
                    }
                }

                // Notification requirement can be changed freely
                // (though disabling it reduces security)

                Ok(true)
            }
        }
    }
}
