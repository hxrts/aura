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
        return Err(AuraError::permission_denied("Guardian not in context"));
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

    // Verify operation signature
    // The proof.operation_signature contains the guardian's signature over the operation
    // In production, this would verify the signature against the guardian's public key
    // from the binding commitment

    // For now, we perform structural validation of the proof
    // TODO: Add cryptographic signature verification once public key lookup is implemented

    if proof.operation_signature.is_empty() {
        return Ok(GuardianAuthResponse {
            success: false,
            authorized: false,
            error: Some("Missing operation signature".to_string()),
        });
    }

    // Serialize the operation to verify it matches
    let _operation_bytes = bincode::serialize(&request.operation)
        .map_err(|e| AuraError::serialization(format!("Failed to serialize operation: {}", e)))?;

    // TODO: Verify signature cryptographically:
    // 1. Extract guardian's public key from binding commitment
    // 2. Verify proof.operation_signature over operation_bytes
    // For now, we accept if the proof structure is valid

    Ok(GuardianAuthResponse {
        success: true,
        authorized: true,
        error: None,
    })
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
    if proof.threshold_signature.is_none() {
        return false;
    }

    // Check 3: Verify attester set is non-empty
    if proof.attester_set.is_empty() {
        return false;
    }

    // Check 4: Verify prestate hash matches binding
    // The prestate should include the guardian binding commitment
    // This ensures the consensus is about the correct binding operation

    // TODO: Implement proper hash verification:
    // 1. Compute binding commitment from binding fields
    // 2. Reconstruct prestate from binding
    // 3. Hash the prestate
    // 4. Compare with proof.prestate_hash

    // For now, we accept if the proof structure is valid and has a prestate_hash
    if proof.prestate_hash.0 == [0u8; 32] {
        return false; // Invalid/zero prestate hash
    }

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
            GuardianOperation::ApproveRecovery { new_commitment: _ } => {
                // Check if recovery delay has passed
                // Guardian parameters specify minimum delay before approval
                let recovery_delay = binding.parameters.recovery_delay;

                // TODO: Track recovery request time in relational context
                // For now, we check that the delay period is configured
                // In production, verify: current_time >= request_time + recovery_delay

                // Check if notification was required
                if binding.parameters.notification_required {
                    // In production, verify notification was sent and acknowledged
                    // This would be tracked as a fact in the relational context
                }

                // For now, approve if binding exists and has reasonable parameters
                if recovery_delay.as_secs() < 3600 {
                    // Delay too short (less than 1 hour)
                    return Ok(false);
                }

                Ok(true)
            }
            GuardianOperation::DenyRecovery { .. } => {
                // Guardians can always deny recovery attempts
                // This is a safety mechanism that requires no additional checks
                Ok(true)
            }
            GuardianOperation::UpdateParameters {
                recovery_delay_seconds,
            } => {
                // Check if guardian has parameter update permission
                // Only allow reasonable parameter changes

                // Verify new delay is within acceptable bounds (e.g., 1 hour to 30 days)
                const MIN_DELAY_SECS: u64 = 3600; // 1 hour
                const MAX_DELAY_SECS: u64 = 30 * 24 * 3600; // 30 days

                if *recovery_delay_seconds < MIN_DELAY_SECS
                    || *recovery_delay_seconds > MAX_DELAY_SECS
                {
                    return Ok(false); // Invalid delay range
                }

                // Delay is within acceptable range
                Ok(true)
            }
        }
    }
}
