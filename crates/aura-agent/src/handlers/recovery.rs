//! Recovery Handlers
//!
//! Handlers for guardian-based key recovery operations including initiating
//! recovery, collecting guardian approvals, and completing recovery ceremonies.

use super::shared::{HandlerContext, HandlerUtilities};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::services::RecoveryManager;
use crate::runtime::AuraEffectSystem;
use aura_core::effects::RandomExtendedEffects;
use aura_core::identifiers::{AuthorityId, RecoveryId};
use aura_core::FlowCost;
use aura_guards::chain::create_send_guard;
use aura_guards::types::CapabilityId;
use aura_protocol::effects::EffectApiEffects;
use serde::{Deserialize, Serialize};

/// Recovery operation state
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryState {
    /// No recovery in progress
    Idle,
    /// Recovery has been initiated, waiting for guardian approvals
    Initiated {
        /// Unique recovery ceremony ID
        recovery_id: RecoveryId,
        /// Number of approvals required
        threshold: u32,
        /// Approvals collected so far
        collected: u32,
    },
    /// Collecting guardian shares
    CollectingShares {
        /// Recovery ceremony ID
        recovery_id: RecoveryId,
        /// Shares collected
        collected: u32,
        /// Shares required
        required: u32,
    },
    /// Reconstructing the key
    Reconstructing {
        /// Recovery ceremony ID
        recovery_id: RecoveryId,
    },
    /// Recovery completed successfully
    Complete {
        /// Recovery ceremony ID
        recovery_id: RecoveryId,
        /// Completion timestamp
        completed_at: u64,
    },
    /// Recovery failed
    Failed {
        /// Recovery ceremony ID
        recovery_id: RecoveryId,
        /// Failure reason
        reason: String,
    },
}

/// Recovery operation type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryOperation {
    /// Replace the entire tree (full key recovery)
    ReplaceTree {
        /// New public key
        new_public_key: Vec<u8>,
    },
    /// Add a new device to the tree
    AddDevice {
        /// Device public key
        device_public_key: Vec<u8>,
    },
    /// Remove a compromised device
    RemoveDevice {
        /// Leaf index of device to remove
        leaf_index: u32,
    },
    /// Update guardian set
    UpdateGuardians {
        /// New guardian authorities
        new_guardians: Vec<AuthorityId>,
        /// New threshold
        new_threshold: u32,
    },
}

/// Recovery initiation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryRequest {
    /// Unique recovery ceremony ID
    pub recovery_id: RecoveryId,
    /// Account authority being recovered
    pub account_authority: AuthorityId,
    /// Recovery operation type
    pub operation: RecoveryOperation,
    /// Justification for recovery
    pub justification: String,
    /// Guardian authorities to request approval from
    pub guardians: Vec<AuthorityId>,
    /// Required threshold of approvals
    pub threshold: u32,
    /// Request timestamp
    pub requested_at: u64,
    /// Optional expiration
    pub expires_at: Option<u64>,
}

/// Guardian approval for a recovery operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianApproval {
    /// Recovery ceremony ID being approved
    pub recovery_id: RecoveryId,
    /// Guardian authority providing approval
    pub guardian_id: AuthorityId,
    /// Guardian's signature over the recovery request
    pub signature: Vec<u8>,
    /// Optional encrypted share data
    pub share_data: Option<Vec<u8>>,
    /// Approval timestamp
    pub approved_at: u64,
}

/// Recovery operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryResult {
    /// Whether recovery succeeded
    pub success: bool,
    /// Recovery ceremony ID
    pub recovery_id: RecoveryId,
    /// Final state
    pub state: RecoveryState,
    /// Recovered key material (if applicable)
    pub key_material: Option<Vec<u8>>,
    /// Guardian approvals received
    pub approvals: Vec<GuardianApproval>,
    /// Error message if failed
    pub error: Option<String>,
}

/// Active recovery ceremony
#[derive(Debug, Clone)]
pub(crate) struct ActiveRecovery {
    pub(crate) request: RecoveryRequest,
    pub(crate) state: RecoveryState,
    pub(crate) approvals: Vec<GuardianApproval>,
}

/// Recovery handler
#[derive(Clone)]
pub struct RecoveryHandler {
    context: HandlerContext,
    /// Active recovery ceremonies
    recovery_manager: RecoveryManager,
}

impl RecoveryHandler {
    /// Create a new recovery handler
    pub fn new(authority: AuthorityContext) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;
        Ok(Self {
            context: HandlerContext::new(authority),
            recovery_manager: RecoveryManager::new(),
        })
    }

    /// Get the authority context
    pub fn authority_context(&self) -> &AuthorityContext {
        &self.context.authority
    }

    /// Get current recovery state
    pub async fn get_state(&self, recovery_id: &RecoveryId) -> Option<RecoveryState> {
        self.recovery_manager.get_state(recovery_id).await
    }

    /// Initiate a recovery ceremony
    pub async fn initiate(
        &self,
        effects: &AuraEffectSystem,
        operation: RecoveryOperation,
        guardians: Vec<AuthorityId>,
        threshold: u32,
        justification: String,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<RecoveryRequest> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // Validate threshold
        if threshold == 0 || threshold > guardians.len() as u32 {
            return Err(AgentError::config(format!(
                "Invalid threshold: {} of {} guardians",
                threshold,
                guardians.len()
            )));
        }

        // Enforce guard (unless testing)
        if !effects.is_testing() {
            let guard = create_send_guard(
                CapabilityId::from("recovery:initiate"),
                self.context.effect_context.context_id(),
                self.context.authority.authority_id(),
                FlowCost::new(100), // Higher cost for recovery operations
            );
            let result = guard
                .evaluate(effects)
                .await
                .map_err(|e| AgentError::effects(format!("guard evaluation failed: {e}")))?;
            if !result.authorized {
                return Err(AgentError::effects(
                    result
                        .denial_reason
                        .unwrap_or_else(|| "recovery initiate not authorized".to_string()),
                ));
            }
        }

        // Generate recovery ID
        let recovery_id =
            RecoveryId::new(format!("recovery-{}", effects.random_uuid().await.simple()));
        let current_time = effects.current_timestamp().await.unwrap_or(0);
        let expires_at = expires_in_ms.map(|ms| current_time + ms);

        let request = RecoveryRequest {
            recovery_id: recovery_id.clone(),
            account_authority: self.context.authority.authority_id(),
            operation,
            justification,
            guardians: guardians.clone(),
            threshold,
            requested_at: current_time,
            expires_at,
        };

        // Journal the recovery initiation fact
        HandlerUtilities::append_relational_fact(
            &self.context.authority,
            effects,
            self.context.effect_context.context_id(),
            "recovery_initiated",
            &serde_json::json!({
                "recovery_id": recovery_id,
                "account_authority": self.context.authority.authority_id(),
                "guardians": guardians,
                "threshold": threshold,
                "requested_at": current_time,
            }),
        )
        .await?;

        // Store active recovery
        let active_recovery = ActiveRecovery {
            request: request.clone(),
            state: RecoveryState::Initiated {
                recovery_id: recovery_id.clone(),
                threshold,
                collected: 0,
            },
            approvals: Vec::new(),
        };

        self.recovery_manager
            .insert(recovery_id, active_recovery)
            .await;

        Ok(request)
    }

    /// Submit a guardian approval
    pub async fn submit_approval(
        &self,
        effects: &AuraEffectSystem,
        approval: GuardianApproval,
    ) -> AgentResult<RecoveryState> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // Enforce guard (unless testing)
        if !effects.is_testing() {
            let guard = create_send_guard(
                CapabilityId::from("recovery:approve"),
                self.context.effect_context.context_id(),
                self.context.authority.authority_id(),
                FlowCost::new(50),
            );
            let result = guard
                .evaluate(effects)
                .await
                .map_err(|e| AgentError::effects(format!("guard evaluation failed: {e}")))?;
            if !result.authorized {
                return Err(AgentError::effects(
                    result
                        .denial_reason
                        .unwrap_or_else(|| "recovery approve not authorized".to_string()),
                ));
            }
        }

        let recovery = self
            .recovery_manager
            .get_recovery(&approval.recovery_id)
            .await
            .ok_or_else(|| {
                AgentError::runtime(format!(
                    "Recovery ceremony not found: {}",
                    approval.recovery_id
                ))
            })?;

        // Verify guardian is in the set
        if !recovery.request.guardians.contains(&approval.guardian_id) {
            return Err(AgentError::effects(format!(
                "Guardian {} not in recovery set",
                approval.guardian_id
            )));
        }

        // Check for duplicate approval
        if recovery
            .approvals
            .iter()
            .any(|a| a.guardian_id == approval.guardian_id)
        {
            return Err(AgentError::effects(format!(
                "Guardian {} already approved",
                approval.guardian_id
            )));
        }

        // Journal the approval
        HandlerUtilities::append_relational_fact(
            &self.context.authority,
            effects,
            self.context.effect_context.context_id(),
            "recovery_guardian_approved",
            &serde_json::json!({
                "recovery_id": approval.recovery_id,
                "guardian_id": approval.guardian_id,
                "approved_at": approval.approved_at,
            }),
        )
        .await?;

        let updated_state = self
            .recovery_manager
            .with_recovery_mut(&approval.recovery_id, |recovery| {
                // Re-check for duplicate approvals in case of concurrent updates.
                if recovery
                    .approvals
                    .iter()
                    .any(|a| a.guardian_id == approval.guardian_id)
                {
                    return Err(AgentError::effects(format!(
                        "Guardian {} already approved",
                        approval.guardian_id
                    )));
                }

                recovery.approvals.push(approval.clone());
                let collected = recovery.approvals.len() as u32;
                let threshold = recovery.request.threshold;

                if collected >= threshold {
                    recovery.state = RecoveryState::CollectingShares {
                        recovery_id: approval.recovery_id.clone(),
                        collected,
                        required: threshold,
                    };
                } else {
                    recovery.state = RecoveryState::Initiated {
                        recovery_id: approval.recovery_id.clone(),
                        threshold,
                        collected,
                    };
                }

                Ok(recovery.state.clone())
            })
            .await
            .ok_or_else(|| {
                AgentError::runtime(format!(
                    "Recovery ceremony not found: {}",
                    approval.recovery_id
                ))
            })??;

        Ok(updated_state)
    }

    /// Complete a recovery ceremony (called when threshold is met)
    pub async fn complete(
        &self,
        effects: &AuraEffectSystem,
        recovery_id: &RecoveryId,
    ) -> AgentResult<RecoveryResult> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        let policy =
            aura_core::threshold::policy_for(aura_core::threshold::CeremonyFlow::RecoveryExecution);
        if policy.allows_mode(aura_core::threshold::AgreementMode::ConsensusFinalized)
            && !effects.is_testing()
        {
            return Err(AgentError::effects(
                "Recovery execution requires consensus finalization".to_string(),
            ));
        }

        // Enforce guard (unless testing)
        if !effects.is_testing() {
            let guard = create_send_guard(
                CapabilityId::from("recovery:complete"),
                self.context.effect_context.context_id(),
                self.context.authority.authority_id(),
                FlowCost::new(100),
            );
            let result = guard
                .evaluate(effects)
                .await
                .map_err(|e| AgentError::effects(format!("guard evaluation failed: {e}")))?;
            if !result.authorized {
                return Err(AgentError::effects(
                    result
                        .denial_reason
                        .unwrap_or_else(|| "recovery complete not authorized".to_string()),
                ));
            }
        }

        let recovery = self
            .recovery_manager
            .get_recovery(recovery_id)
            .await
            .ok_or_else(|| {
                AgentError::runtime(format!("Recovery ceremony not found: {}", recovery_id))
            })?;

        // Check threshold is met
        let collected = recovery.approvals.len() as u32;
        if collected < recovery.request.threshold {
            return Err(AgentError::effects(format!(
                "Threshold not met: {} of {} required",
                collected, recovery.request.threshold
            )));
        }

        let current_time = effects.current_timestamp().await.unwrap_or(0);

        // Journal completion
        HandlerUtilities::append_relational_fact(
            &self.context.authority,
            effects,
            self.context.effect_context.context_id(),
            "recovery_completed",
            &serde_json::json!({
                "recovery_id": recovery_id,
                "approvals_count": collected,
                "completed_at": current_time,
            }),
        )
        .await?;

        let completed_state = RecoveryState::Complete {
            recovery_id: recovery_id.clone(),
            completed_at: current_time,
        };

        let result = RecoveryResult {
            success: true,
            recovery_id: recovery_id.clone(),
            state: completed_state.clone(),
            key_material: None, // Would be populated from actual key reconstruction
            approvals: recovery.approvals.clone(),
            error: None,
        };

        // Update state and remove from active recoveries
        let _ = self
            .recovery_manager
            .with_recovery_mut(recovery_id, |recovery| {
                recovery.state = completed_state;
            })
            .await;
        let _ = self.recovery_manager.remove(recovery_id).await;

        Ok(result)
    }

    /// Cancel a recovery ceremony
    pub async fn cancel(
        &self,
        effects: &AuraEffectSystem,
        recovery_id: &RecoveryId,
        reason: String,
    ) -> AgentResult<RecoveryResult> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // Enforce guard (unless testing)
        if !effects.is_testing() {
            let guard = create_send_guard(
                CapabilityId::from("recovery:cancel"),
                self.context.effect_context.context_id(),
                self.context.authority.authority_id(),
                FlowCost::new(30),
            );
            let result = guard
                .evaluate(effects)
                .await
                .map_err(|e| AgentError::effects(format!("guard evaluation failed: {e}")))?;
            if !result.authorized {
                return Err(AgentError::effects(
                    result
                        .denial_reason
                        .unwrap_or_else(|| "recovery cancel not authorized".to_string()),
                ));
            }
        }

        let recovery = self
            .recovery_manager
            .get_recovery(recovery_id)
            .await
            .ok_or_else(|| {
                AgentError::runtime(format!("Recovery ceremony not found: {}", recovery_id))
            })?;

        // Journal cancellation
        HandlerUtilities::append_relational_fact(
            &self.context.authority,
            effects,
            self.context.effect_context.context_id(),
            "recovery_cancelled",
            &serde_json::json!({
                "recovery_id": recovery_id,
                "reason": reason,
            }),
        )
        .await?;

        let failed_state = RecoveryState::Failed {
            recovery_id: recovery_id.clone(),
            reason: reason.clone(),
        };

        let result = RecoveryResult {
            success: false,
            recovery_id: recovery_id.clone(),
            state: failed_state.clone(),
            key_material: None,
            approvals: recovery.approvals.clone(),
            error: Some(reason),
        };

        // Update state and remove from active recoveries
        let _ = self
            .recovery_manager
            .with_recovery_mut(recovery_id, |recovery| {
                recovery.state = failed_state;
            })
            .await;
        let _ = self.recovery_manager.remove(recovery_id).await;

        Ok(result)
    }

    /// List active recovery ceremonies
    pub async fn list_active(&self) -> Vec<(RecoveryId, RecoveryState)> {
        self.recovery_manager.list_active().await
    }

    /// Cleanup expired recovery ceremonies.
    ///
    /// Removes recoveries that have passed their expiration time.
    /// Returns the number of recoveries removed.
    pub async fn cleanup_expired(&self, current_time: u64) -> usize {
        let removed = self.recovery_manager.cleanup_expired(current_time).await;
        if removed > 0 {
            tracing::debug!(removed, "Cleaned up expired recovery ceremonies");
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use crate::runtime::effects::AuraEffectSystem;
    use std::sync::Arc;

    fn create_test_authority(seed: u8) -> AuthorityContext {
        let authority_id = AuthorityId::new_from_entropy([seed; 32]);
        AuthorityContext::new(authority_id)
    }

    #[tokio::test]
    async fn recovery_can_be_initiated() {
        let authority_context = create_test_authority(130);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let handler = RecoveryHandler::new(authority_context).unwrap();

        let guardians = vec![
            AuthorityId::new_from_entropy([131u8; 32]),
            AuthorityId::new_from_entropy([132u8; 32]),
            AuthorityId::new_from_entropy([133u8; 32]),
        ];

        let request = handler
            .initiate(
                &effects,
                RecoveryOperation::AddDevice {
                    device_public_key: vec![0u8; 32],
                },
                guardians.clone(),
                2, // 2-of-3
                "Lost device".to_string(),
                Some(86400000), // 1 day
            )
            .await
            .unwrap();

        assert!(request.recovery_id.as_str().starts_with("recovery-"));
        assert_eq!(request.threshold, 2);
        assert_eq!(request.guardians.len(), 3);
    }

    #[tokio::test]
    async fn guardian_approvals_can_be_submitted() {
        let authority_context = create_test_authority(134);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let handler = RecoveryHandler::new(authority_context).unwrap();

        let guardians = vec![
            AuthorityId::new_from_entropy([135u8; 32]),
            AuthorityId::new_from_entropy([136u8; 32]),
        ];

        let request = handler
            .initiate(
                &effects,
                RecoveryOperation::RemoveDevice { leaf_index: 0 },
                guardians.clone(),
                2, // 2-of-2
                "Compromised device".to_string(),
                None,
            )
            .await
            .unwrap();

        // Submit first approval
        let approval1 = GuardianApproval {
            recovery_id: request.recovery_id.clone(),
            guardian_id: guardians[0],
            signature: vec![0u8; 64],
            share_data: None,
            approved_at: 12345,
        };
        let state = handler.submit_approval(&effects, approval1).await.unwrap();

        match state {
            RecoveryState::Initiated { collected, .. } => {
                assert_eq!(collected, 1);
            }
            _ => panic!("Expected Initiated state"),
        }

        // Submit second approval
        let approval2 = GuardianApproval {
            recovery_id: request.recovery_id.clone(),
            guardian_id: guardians[1],
            signature: vec![0u8; 64],
            share_data: None,
            approved_at: 12346,
        };
        let state = handler.submit_approval(&effects, approval2).await.unwrap();

        match state {
            RecoveryState::CollectingShares { collected, .. } => {
                assert_eq!(collected, 2);
            }
            _ => panic!("Expected CollectingShares state"),
        }
    }

    #[tokio::test]
    async fn recovery_can_be_completed() {
        let authority_context = create_test_authority(137);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let handler = RecoveryHandler::new(authority_context).unwrap();

        let guardians = vec![AuthorityId::new_from_entropy([138u8; 32])];

        let request = handler
            .initiate(
                &effects,
                RecoveryOperation::ReplaceTree {
                    new_public_key: vec![0u8; 32],
                },
                guardians.clone(),
                1, // 1-of-1
                "Full recovery".to_string(),
                None,
            )
            .await
            .unwrap();

        // Submit approval
        let approval = GuardianApproval {
            recovery_id: request.recovery_id.clone(),
            guardian_id: guardians[0],
            signature: vec![0u8; 64],
            share_data: Some(vec![1, 2, 3]),
            approved_at: 12345,
        };
        handler.submit_approval(&effects, approval).await.unwrap();

        // Complete recovery
        let result = handler
            .complete(&effects, &request.recovery_id)
            .await
            .unwrap();

        assert!(result.success);
        match result.state {
            RecoveryState::Complete { .. } => {}
            _ => panic!("Expected Complete state"),
        }
    }

    #[tokio::test]
    async fn recovery_can_be_cancelled() {
        let authority_context = create_test_authority(139);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let handler = RecoveryHandler::new(authority_context).unwrap();

        let guardians = vec![
            AuthorityId::new_from_entropy([140u8; 32]),
            AuthorityId::new_from_entropy([141u8; 32]),
        ];

        let request = handler
            .initiate(
                &effects,
                RecoveryOperation::UpdateGuardians {
                    new_guardians: vec![],
                    new_threshold: 1,
                },
                guardians,
                2,
                "Test".to_string(),
                None,
            )
            .await
            .unwrap();

        let result = handler
            .cancel(&effects, &request.recovery_id, "User cancelled".to_string())
            .await
            .unwrap();

        assert!(!result.success);
        match result.state {
            RecoveryState::Failed { reason, .. } => {
                assert_eq!(reason, "User cancelled");
            }
            _ => panic!("Expected Failed state"),
        }
    }

    #[tokio::test]
    async fn invalid_threshold_is_rejected() {
        let authority_context = create_test_authority(142);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let handler = RecoveryHandler::new(authority_context).unwrap();

        let guardians = vec![AuthorityId::new_from_entropy([143u8; 32])];

        let result = handler
            .initiate(
                &effects,
                RecoveryOperation::AddDevice {
                    device_public_key: vec![0u8; 32],
                },
                guardians,
                2, // 2-of-1 is invalid
                "Test".to_string(),
                None,
            )
            .await;

        assert!(result.is_err());
    }
}
