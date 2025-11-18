//! Guardian Key Recovery Choreography
//!
//! Emergency key recovery using threshold guardian approval.
//! This is the only recovery mechanism - all recoveries are considered emergency.

use crate::{
    types::{GuardianProfile, RecoveryRequest, RecoveryResponse, RecoveryShare},
    RecoveryResult,
};
use aura_authenticate::guardian_auth::{RecoveryContext, RecoveryOperationType};
use aura_core::crypto::{IdentityKeyContext, KeyDerivationSpec};
use aura_core::effects::TimeEffects;
use aura_core::frost::ThresholdSignature;
use aura_core::{hash, identifiers::GuardianId, AccountId, AuraError, DeviceId, FlowBudget};
use aura_effects::crypto::derive_key_material;
use aura_macros::choreography;
use aura_protocol::{guards::BiscuitGuardEvaluator, AuraEffectSystem};
use aura_wot::{BiscuitTokenManager, ResourceScope};
use biscuit_auth::Biscuit;
use serde::{Deserialize, Serialize};

/// Guardian key recovery request data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRecoveryRequest {
    /// Unique identifier for this recovery ceremony
    pub recovery_id: String,
    /// Device requesting the key recovery
    pub requesting_device: DeviceId,
    /// Account whose keys are being recovered
    pub account_id: AccountId,
    /// Recovery context and justification
    pub context: RecoveryContext,
    /// Required threshold of guardian approvals
    pub threshold: usize,
}

/// Guardian approval with key share
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianKeyApproval {
    /// Guardian identifier for the approving party
    pub guardian_id: GuardianId,
    /// Encrypted key share contributed by this guardian
    pub key_share: Vec<u8>,
    /// Guardian's partial signature on the recovery approval
    pub partial_signature: Vec<u8>,
    /// Timestamp when the approval was generated
    pub timestamp: u64,
}

/// Recovery completion notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryCompletion {
    /// Unique identifier for the recovery ceremony
    pub recovery_id: String,
    /// Whether the recovery was successful
    pub success: bool,
    /// Hash of recovered key (don't send actual key)
    pub recovered_key_hash: Vec<u8>,
    /// Identifier for recovery evidence record
    pub evidence_id: String,
}

// Guardian Key Recovery Choreography - 3 phase protocol
choreography! {
    #[namespace = "guardian_key_recovery"]
    protocol GuardianKeyRecovery {
        roles: RecoveringDevice, Guardian1, Guardian2, Guardian3;

        // Phase 1: Recovery request to all guardians
        RecoveringDevice[guard_capability = "initiate_emergency_recovery",
                        flow_cost = 300,
                        journal_facts = "emergency_recovery_initiated",
                        leakage_budget = [1, 0, 0]]
        -> Guardian1: RequestRecovery(KeyRecoveryRequest);

        RecoveringDevice[guard_capability = "initiate_emergency_recovery",
                        flow_cost = 300]
        -> Guardian2: RequestRecovery(KeyRecoveryRequest);

        RecoveringDevice[guard_capability = "initiate_emergency_recovery",
                        flow_cost = 300]
        -> Guardian3: RequestRecovery(KeyRecoveryRequest);

        // Phase 2: Guardian approvals back to recovering device
        Guardian1[guard_capability = "approve_emergency_recovery,verify_recovery_context",
                  flow_cost = 200,
                  journal_facts = "guardian_recovery_approved",
                  leakage_budget = [0, 1, 0]]
        -> RecoveringDevice: ApproveRecovery(GuardianKeyApproval);

        Guardian2[guard_capability = "approve_emergency_recovery,verify_recovery_context",
                  flow_cost = 200,
                  journal_facts = "guardian_recovery_approved"]
        -> RecoveringDevice: ApproveRecovery(GuardianKeyApproval);

        Guardian3[guard_capability = "approve_emergency_recovery,verify_recovery_context",
                  flow_cost = 200,
                  journal_facts = "guardian_recovery_approved"]
        -> RecoveringDevice: ApproveRecovery(GuardianKeyApproval);

        // Phase 3: Recovery completion broadcast
        RecoveringDevice[guard_capability = "complete_emergency_recovery",
                        flow_cost = 150,
                        journal_facts = "emergency_recovery_completed",
                        journal_merge = true]
        -> Guardian1: CompleteRecovery(RecoveryCompletion);

        RecoveringDevice[guard_capability = "complete_emergency_recovery",
                        flow_cost = 150,
                        journal_merge = true]
        -> Guardian2: CompleteRecovery(RecoveryCompletion);

        RecoveringDevice[guard_capability = "complete_emergency_recovery",
                        flow_cost = 150,
                        journal_merge = true]
        -> Guardian3: CompleteRecovery(RecoveryCompletion);
    }
}

/// Guardian key recovery coordinator
pub struct GuardianKeyRecoveryCoordinator {
    _effect_system: AuraEffectSystem,
    /// Optional token manager for Biscuit authorization
    token_manager: Option<BiscuitTokenManager>,
    /// Optional guard evaluator for Biscuit authorization
    guard_evaluator: Option<BiscuitGuardEvaluator>,
}

impl GuardianKeyRecoveryCoordinator {
    /// Create new coordinator
    pub fn new(effect_system: AuraEffectSystem) -> Self {
        Self {
            _effect_system: effect_system,
            token_manager: None,
            guard_evaluator: None,
        }
    }

    /// Create new coordinator with Biscuit authorization
    pub fn new_with_biscuit(
        effect_system: AuraEffectSystem,
        token_manager: BiscuitTokenManager,
        guard_evaluator: BiscuitGuardEvaluator,
    ) -> Self {
        Self {
            _effect_system: effect_system,
            token_manager: Some(token_manager),
            guard_evaluator: Some(guard_evaluator),
        }
    }

    /// Execute emergency key recovery as recovering device
    pub async fn execute_key_recovery(
        &self,
        request: RecoveryRequest,
    ) -> RecoveryResult<RecoveryResponse> {
        // Check authorization using Biscuit tokens if available
        if let Err(auth_error) = self.check_recovery_authorization(&request).await {
            return Ok(RecoveryResponse {
                success: false,
                error: Some(format!("Authorization failed: {}", auth_error)),
                key_material: None,
                guardian_shares: Vec::new(),
                evidence: self.create_failed_evidence(&request),
                signature: self.create_empty_signature(),
            });
        }

        let recovery_id = self.generate_recovery_id(&request);

        // Convert generic request to choreography-specific request
        let recovery_request = KeyRecoveryRequest {
            recovery_id: recovery_id.clone(),
            requesting_device: request.requesting_device,
            account_id: request.account_id,
            context: request.context.clone(),
            threshold: request.threshold,
        };

        // Execute the choreographic protocol
        let result = self
            .execute_choreographic_key_recovery(recovery_request)
            .await;

        match result {
            Ok(approvals) => {
                // Convert approvals to shares first
                let shares: Vec<_> = approvals
                    .iter()
                    .map(|approval| {
                        RecoveryShare {
                            guardian: GuardianProfile {
                                guardian_id: approval.guardian_id,
                                device_id: DeviceId::new(), // Placeholder
                                label: "Guardian".to_string(),
                                trust_level: aura_core::TrustLevel::High,
                                cooldown_secs: 900,
                            },
                            share: approval.key_share.clone(),
                            partial_signature: approval.partial_signature.clone(),
                            issued_at: approval.timestamp,
                        }
                    })
                    .collect();

                // Check if we have enough approvals
                if approvals.len() < request.threshold {
                    return Ok(RecoveryResponse {
                        success: false,
                        error: Some(format!(
                            "Insufficient guardian approvals: got {}, need {}",
                            approvals.len(),
                            request.threshold
                        )),
                        key_material: None,
                        guardian_shares: shares,
                        evidence: self.create_failed_evidence(&request),
                        signature: self.create_empty_signature(),
                    });
                }

                // Reconstruct key from shares
                let recovered_key = self.reconstruct_key(&approvals, &request)?;

                // Create evidence and signature
                let evidence = self.create_evidence(&request, &shares);
                let signature = self.aggregate_signature(&shares);

                // Create completion message for final phase
                let completion = RecoveryCompletion {
                    recovery_id: recovery_id.clone(),
                    success: true,
                    recovered_key_hash: self.hash_key(&recovered_key),
                    evidence_id: format!("evidence_{}", recovery_id),
                };

                // Phase 3 would broadcast completion through choreography
                self.broadcast_recovery_completion(completion).await?;

                Ok(RecoveryResponse {
                    success: true,
                    error: None,
                    key_material: Some(recovered_key),
                    guardian_shares: shares,
                    evidence,
                    signature,
                })
            }
            Err(e) => Ok(RecoveryResponse {
                success: false,
                error: Some(format!("Recovery choreography failed: {}", e)),
                key_material: None,
                guardian_shares: Vec::new(),
                evidence: self.create_failed_evidence(&request),
                signature: self.create_empty_signature(),
            }),
        }
    }

    /// Execute as guardian (approve recovery request)
    pub async fn approve_as_guardian(
        &self,
        _request: KeyRecoveryRequest,
    ) -> RecoveryResult<GuardianKeyApproval> {
        // For now, simulate guardian approval
        // In real implementation, this would run the guardian side of the choreography

        Ok(GuardianKeyApproval {
            guardian_id: GuardianId::new(), // Would be actual guardian ID
            key_share: vec![1; 32],         // Placeholder key share
            partial_signature: vec![2; 64], // Placeholder signature
            timestamp: 0,                   // Placeholder timestamp
        })
    }

    /// Execute choreographic key recovery protocol (Phase 1-2)
    async fn execute_choreographic_key_recovery(
        &self,
        _request: KeyRecoveryRequest,
    ) -> RecoveryResult<Vec<GuardianKeyApproval>> {
        // Phase 1: Send recovery requests to all guardians (choreographic send operations)
        // This would be handled by the generated choreography runtime

        // Phase 2: Collect guardian approvals (choreographic receive operations)
        // For now, simulate the expected responses that would come through choreography
        let timestamp = self.current_timestamp().await;
        let approvals = vec![
            GuardianKeyApproval {
                guardian_id: GuardianId::new(),
                key_share: vec![1; 32],
                partial_signature: vec![2; 64],
                timestamp,
            },
            GuardianKeyApproval {
                guardian_id: GuardianId::new(),
                key_share: vec![3; 32],
                partial_signature: vec![4; 64],
                timestamp,
            },
        ];

        Ok(approvals)
    }

    /// Broadcast recovery completion (Phase 3)
    async fn broadcast_recovery_completion(
        &self,
        _completion: RecoveryCompletion,
    ) -> RecoveryResult<()> {
        // This would be handled by the choreographic broadcast in the generated code
        // The choreography runtime would send completion messages to all guardians
        Ok(())
    }

    /// Get current timestamp
    async fn current_timestamp(&self) -> u64 {
        self._effect_system.current_timestamp().await
    }

    // Helper methods
    fn generate_recovery_id(&self, request: &RecoveryRequest) -> String {
        format!(
            "recovery_{}_{}",
            request.account_id, request.requesting_device
        )
    }

    fn reconstruct_key(
        &self,
        approvals: &[GuardianKeyApproval],
        request: &RecoveryRequest,
    ) -> RecoveryResult<Vec<u8>> {
        // Combine all shares
        let mut combined_material = Vec::new();
        for approval in approvals {
            combined_material.extend_from_slice(&approval.key_share);
            combined_material.extend_from_slice(approval.guardian_id.0.as_bytes());
        }

        // Add request context
        combined_material.extend_from_slice(request.account_id.0.as_bytes());
        combined_material.extend_from_slice(request.requesting_device.0.as_bytes());

        // Use proper key derivation for guardian keys
        let guardian_id_bytes = format!("recovery_{}", request.account_id).into_bytes();
        let spec = KeyDerivationSpec::guardian_keys(guardian_id_bytes, 1);

        derive_key_material(&combined_material, &spec, 32)
            .map_err(|e| AuraError::crypto(format!("Key derivation failed: {}", e)))
    }

    fn aggregate_signature(&self, shares: &[RecoveryShare]) -> ThresholdSignature {
        let mut combined_signature = Vec::new();
        for share in shares {
            combined_signature.extend_from_slice(&share.partial_signature);
        }

        let signature_bytes = if combined_signature.len() >= 64 {
            combined_signature[..64].to_vec()
        } else {
            let mut padded = combined_signature;
            padded.resize(64, 0);
            padded
        };

        let signers: Vec<u16> = shares
            .iter()
            .enumerate()
            .map(|(idx, _)| idx as u16)
            .collect();

        ThresholdSignature::new(signature_bytes, signers)
    }

    fn hash_key(&self, key: &[u8]) -> Vec<u8> {
        // Simple hash for evidence
        hash::hash(key).to_vec()
    }

    fn create_evidence(
        &self,
        _request: &RecoveryRequest,
        _shares: &[RecoveryShare],
    ) -> crate::types::RecoveryEvidence {
        // Placeholder implementation
        crate::types::RecoveryEvidence {
            account_id: AccountId::new(),
            recovering_device: DeviceId::new(),
            guardians: Vec::new(),
            issued_at: 0,
            cooldown_expires_at: 0,
            dispute_window_ends_at: 0,
            guardian_profiles: Vec::new(),
            disputes: Vec::new(),
            threshold_signature: None,
        }
    }

    fn create_failed_evidence(&self, _request: &RecoveryRequest) -> crate::types::RecoveryEvidence {
        // Placeholder implementation
        crate::types::RecoveryEvidence {
            account_id: AccountId::new(),
            recovering_device: DeviceId::new(),
            guardians: Vec::new(),
            issued_at: 0,
            cooldown_expires_at: 0,
            dispute_window_ends_at: 0,
            guardian_profiles: Vec::new(),
            disputes: Vec::new(),
            threshold_signature: None,
        }
    }

    fn create_empty_signature(&self) -> ThresholdSignature {
        ThresholdSignature::new(vec![0; 64], vec![])
    }

    /// Check if the recovery request is authorized using Biscuit tokens
    async fn check_recovery_authorization(&self, request: &RecoveryRequest) -> Result<(), String> {
        let (token_manager, guard_evaluator) = match (&self.token_manager, &self.guard_evaluator) {
            (Some(tm), Some(ge)) => (tm, ge),
            _ => return Err("Biscuit authorization components not available".to_string()),
        };

        let token = token_manager.current_token();

        // Map recovery operation type to resource scope
        let resource_scope = match request.context.operation_type {
            RecoveryOperationType::DeviceKeyRecovery => ResourceScope::Recovery {
                recovery_type: aura_wot::RecoveryType::DeviceKey,
            },
            RecoveryOperationType::AccountAccessRecovery => ResourceScope::Recovery {
                recovery_type: aura_wot::RecoveryType::AccountAccess,
            },
            RecoveryOperationType::GuardianSetModification => ResourceScope::Recovery {
                recovery_type: aura_wot::RecoveryType::GuardianSet,
            },
            RecoveryOperationType::EmergencyFreeze | RecoveryOperationType::AccountUnfreeze => {
                ResourceScope::Recovery {
                    recovery_type: aura_wot::RecoveryType::EmergencyFreeze,
                }
            }
        };

        // Check authorization for emergency recovery initiation
        let authorized = guard_evaluator
            .check_guard(token, "initiate_emergency_recovery", &resource_scope)
            .map_err(|e| format!("Biscuit authorization error: {}", e))?;

        if !authorized {
            return Err(
                "Biscuit token does not grant permission to initiate emergency recovery"
                    .to_string(),
            );
        }

        Ok(())
    }
}
