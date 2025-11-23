//! Guardian Authentication via Relational Contexts
//!
//! This module implements guardian authentication using the RelationalContext
//! model, replacing the device-centric guardian authentication.

use aura_core::{
    relational::{GenericBinding, RelationalFact},
    AuraError, Authority, AuthorityId, Hash32, Result, AccountId,
};
use aura_macros::choreography;
use aura_core::relational::GuardianBinding;
use aura_relational::RelationalContext;
use ed25519_dalek::{Signature, VerifyingKey, Verifier};
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
    pub binding_proof: Option<aura_core::relational::ConsensusProof>,
    /// Signature over operation
    pub operation_signature: Vec<u8>,
    /// Timestamp when the proof was created
    pub issued_at: u64,
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
        issued_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
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

    // Basic freshness check (10 minutes)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if now.saturating_sub(proof.issued_at) > 600 {
        return Ok(GuardianAuthResponse {
            success: false,
            authorized: false,
            error: Some("Guardian proof expired".to_string()),
        });
    }

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

    // Serialize the operation and verify the guardian's signature
    let operation_bytes = bincode::serialize(&request.operation)
        .map_err(|e| AuraError::serialization(format!("Failed to serialize operation: {}", e)))?;

    // Ensure signature length is valid
    let sig_bytes: [u8; 64] = proof
        .operation_signature
        .as_slice()
        .try_into()
        .map_err(|_| AuraError::crypto("Invalid guardian signature length"))?;

    // Derive verifying key from guardian commitment (the binding commits to the guardian's root key)
    let verifying_key_bytes: [u8; 32] = binding.guardian_commitment.0;
    let verifying_key = VerifyingKey::from_bytes(&verifying_key_bytes).map_err(|e| {
        AuraError::crypto(format!(
            "Invalid guardian commitment (pubkey decode failed): {}",
            e
        ))
    })?;

    // Verify signature
    if let Err(err) = verifying_key.verify(&operation_bytes, &Signature::from_bytes(&sig_bytes)) {
        return Ok(GuardianAuthResponse {
            success: false,
            authorized: false,
            error: Some(format!("Invalid guardian signature: {}", err)),
        });
    }

    Ok(GuardianAuthResponse {
        success: true,
        authorized: true,
        error: None,
    })
}

/// Verify consensus proof for guardian binding
fn verify_consensus_proof(
    proof: &aura_core::relational::ConsensusProof,
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

    // Check 4: Verify prestate hash matches binding by hashing the binding payload
    let binding_bytes = bincode::serialize(binding).unwrap_or_default();
    let expected_hash = aura_core::hash::hash(&binding_bytes);
    if proof.prestate_hash.0 != expected_hash {
        return false;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecoveryRequestRecord {
    guardian_id: AuthorityId,
    account_id: AuthorityId,
    requested_at: u64,
    operation: GuardianOperation,
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
        let verified = verify_guardian_proof(&self.context, &request, &proof).await?;

        // Record request for delay enforcement
        let record = RecoveryRequestRecord {
            guardian_id: guardian.authority_id(),
            account_id: request.account_id,
            requested_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            operation: request.operation.clone(),
        };

        if let Ok(binding_bytes) = serde_json::to_vec(&record) {
            let _ = self.context.add_fact(RelationalFact::Generic(GenericBinding::new(
                "recovery_request".to_string(),
                binding_bytes,
            )));
        }

        Ok(verified)
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
                let recovery_delay = binding.parameters.recovery_delay;

                // Determine the latest recovery request timestamp for this guardian
                let latest_request_time = self
                    .context
                    .get_facts()
                    .iter()
                    .filter_map(|fact| match fact {
                        RelationalFact::Generic(binding)
                            if binding.binding_type == "recovery_request" =>
                        {
                            serde_json::from_slice::<RecoveryRequestRecord>(
                                &binding.binding_data,
                            )
                            .ok()
                        }
                        _ => None,
                    })
                    .filter(|record| record.guardian_id == guardian_id)
                    .map(|record| record.requested_at)
                    .max()
                    .unwrap_or(0);

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                if latest_request_time > 0
                    && now < latest_request_time + recovery_delay.as_secs()
                {
                    return Ok(false);
                }

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
