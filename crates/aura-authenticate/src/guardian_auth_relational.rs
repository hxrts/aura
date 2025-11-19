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

    // Verify operation signature
    // TODO: Implement signature verification using guardian's public key

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
    // TODO: Implement actual consensus proof verification
    // For now, check basic threshold
    proof.threshold_met
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
            GuardianOperation::ApproveRecovery { .. } => {
                // Check if recovery delay has passed
                // TODO: Implement time-based checks
                Ok(true)
            }
            GuardianOperation::DenyRecovery { .. } => {
                // Guardians can always deny
                Ok(true)
            }
            GuardianOperation::UpdateParameters { .. } => {
                // Check if guardian has parameter update permission
                // TODO: Check specific permissions
                Ok(true)
            }
        }
    }
}
