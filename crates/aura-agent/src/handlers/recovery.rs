//! Guardian recovery operations exposed by the agent.

use crate::errors::{AuraError, Result};
use aura_core::DeviceId;
use aura_protocol::effects::{AuraEffectSystem, TimeEffects};
use aura_recovery::{
    guardian_recovery::{
        build_recovery_response, GuardianRecoveryRequest, GuardianRecoveryResponse,
        DEFAULT_DISPUTE_WINDOW_SECS, RecoveryPolicyConfig, RecoveryPolicyEnforcer,
    },
    RecoveryChoreography, RecoveryDispute, RecoveryEvidence, RecoveryRole, RecoverySessionResult,
    RecoveryShare,
};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

static GLOBAL_RECOVERY_LEDGER: Lazy<Mutex<HashMap<String, RecoveryEvidence>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Simple status snapshot for operator UX.
#[derive(Debug, Clone)]
pub struct RecoveryStatus {
    /// How many recovery sessions are currently executing.
    pub pending_sessions: usize,
    /// Most recent recovery evidence emitted by the choreography.
    pub latest_evidence: Option<RecoveryEvidence>,
    /// Identifier of the latest evidence.
    pub latest_evidence_id: Option<String>,
    /// When the current guardian cooldown expires (if any).
    pub cooldown_expires_at: Option<u64>,
    /// Remaining seconds until cooldown lifts (if any).
    pub cooldown_remaining: Option<u64>,
    /// Dispute window deadline (if any).
    pub dispute_window_ends_at: Option<u64>,
    /// Whether the latest evidence has been disputed.
    pub disputed: bool,
}

/// Recovery operations handler with policy enforcement.
pub struct RecoveryOperations {
    effects: Arc<RwLock<AuraEffectSystem>>,
    device_id: DeviceId,
    pending_sessions: Arc<Mutex<usize>>,
    evidence_log: Arc<Mutex<Vec<(String, RecoveryEvidence)>>>,
    policy_enforcer: RecoveryPolicyEnforcer,
}

impl RecoveryOperations {
    /// Create a new recovery operations handler with default policy.
    pub fn new(effects: Arc<RwLock<AuraEffectSystem>>, device_id: DeviceId) -> Self {
        let policy_config = RecoveryPolicyConfig::default();
        Self::with_policy_config(effects, device_id, policy_config)
    }

    /// Create a new recovery operations handler with custom policy configuration.
    pub fn with_policy_config(
        effects: Arc<RwLock<AuraEffectSystem>>, 
        device_id: DeviceId, 
        policy_config: RecoveryPolicyConfig
    ) -> Self {
        let effect_system = {
            let guard = effects.blocking_read();
            guard.clone()
        };
        let policy_enforcer = RecoveryPolicyEnforcer::new(policy_config, effect_system);
        
        Self {
            effects,
            device_id,
            pending_sessions: Arc::new(Mutex::new(0)),
            evidence_log: Arc::new(Mutex::new(Vec::new())),
            policy_enforcer,
        }
    }

    /// Start a guardian recovery session (recovering device role) with policy enforcement.
    pub async fn start_guardian_recovery(
        &self,
        request: GuardianRecoveryRequest,
    ) -> Result<GuardianRecoveryResponse> {
        // Validate request against recovery policy
        let validation = self.policy_enforcer.validate_recovery_request(&request).await
            .map_err(|e| AuraError::internal(e.to_string()))?;
        
        if !validation.is_valid {
            let effects = self.effects.read().await;
            effects.log_error(
                &format!("Recovery policy validation failed: {} violations", validation.violations.len()),
                &[],
            );
            
            for violation in &validation.violations {
                effects.log_error(
                    &format!("Policy violation: {:?}", violation),
                    &[],
                );
            }
            
            return Err(AuraError::permission_denied(format!(
                "Recovery request rejected due to policy violations: {}",
                validation.violations.len()
            )));
        }

        // Log policy warnings
        if !validation.warnings.is_empty() {
            let effects = self.effects.read().await;
            for warning in &validation.warnings {
                effects.log_warn(
                    &format!("Policy warning: {:?}", warning),
                    &[],
                );
            }
        }

        {
            let mut pending = self.pending_sessions.lock().await;
            *pending += 1;
        }

        let session_result = self.execute_recovery_session(request.clone()).await;

        {
            let mut pending = self.pending_sessions.lock().await;
            *pending = pending.saturating_sub(1);
        }

        let session = session_result?;
        let evidence_id = format!(
            "{}:{}",
            session.evidence.account_id, session.evidence.issued_at
        );
        {
            let mut log = self.evidence_log.lock().await;
            log.push((evidence_id.clone(), session.evidence.clone()));
        }
        {
            let mut ledger = GLOBAL_RECOVERY_LEDGER.lock().await;
            ledger.insert(evidence_id, session.evidence.clone());
        }

        Ok(build_recovery_response(request, session))
    }

    async fn execute_recovery_session(
        &self,
        request: GuardianRecoveryRequest,
    ) -> Result<RecoverySessionResult> {
        let effects = self.effects.read().await;
        let mut choreography = RecoveryChoreography::new(
            RecoveryRole::RecoveringDevice(self.device_id),
            request.available_guardians.clone(),
            request.required_threshold,
            effects.clone(),
        );

        choreography
            .execute_recovery(request)
            .await
            .map_err(|err| AuraError::internal(err.to_string()))
    }

    /// Approve a guardian recovery request from the guardian device with policy enforcement.
    pub async fn approve_guardian_recovery(
        &self,
        request: GuardianRecoveryRequest,
    ) -> Result<RecoveryShare> {
        // Validate guardian approval against policy
        let validation = self.policy_enforcer.validate_guardian_approval(&self.device_id, &request).await
            .map_err(|e| AuraError::internal(e.to_string()))?;
        
        if !validation.is_valid {
            let effects = self.effects.read().await;
            effects.log_error(
                &format!("Guardian approval policy validation failed: {} violations", validation.violations.len()),
                &[],
            );
            
            for violation in &validation.violations {
                effects.log_error(
                    &format!("Policy violation: {:?}", violation),
                    &[],
                );
            }
            
            return Err(AuraError::permission_denied(format!(
                "Guardian approval rejected due to policy violations: {}",
                validation.violations.len()
            )));
        }

        let effects = self.effects.read().await;
        let mut choreography = RecoveryChoreography::new(
            RecoveryRole::Guardian(self.device_id),
            request.available_guardians.clone(),
            request.required_threshold,
            effects.clone(),
        );

        choreography
            .approve_as_guardian(request)
            .await
            .map_err(|err| AuraError::internal(err.to_string()))
    }

    /// Return lightweight status snapshot for CLI/UX.
    pub async fn recovery_status(&self) -> Result<RecoveryStatus> {
        let pending = *self.pending_sessions.lock().await;
        let latest_entry = self.evidence_log.lock().await.last().cloned();
        let current_effects = {
            let guard = self.effects.read().await;
            guard.clone()
        };

        let (latest_evidence, latest_id) = match latest_entry {
            Some((id, evidence)) => (Some(evidence), Some(id)),
            None => (None, None),
        };

        let (cooldown_expires_at, cooldown_remaining, dispute_window_ends_at, disputed) =
            if let Some(evidence) = &latest_evidence {
                let now = current_effects.current_timestamp().await;
                if now < evidence.cooldown_expires_at {
                    (
                        Some(evidence.cooldown_expires_at),
                        Some(evidence.cooldown_expires_at - now),
                        Some(evidence.dispute_window_ends_at),
                        !evidence.disputes.is_empty(),
                    )
                } else {
                    (
                        Some(evidence.cooldown_expires_at),
                        None,
                        Some(evidence.dispute_window_ends_at),
                        !evidence.disputes.is_empty(),
                    )
                }
            } else {
                (None, None, None, false)
            };

        Ok(RecoveryStatus {
            pending_sessions: pending,
            latest_evidence,
            latest_evidence_id: latest_id,
            cooldown_expires_at,
            cooldown_remaining,
            dispute_window_ends_at,
            disputed,
        })
    }

    /// Get the device ID for this recovery operations handler
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// File a dispute against an existing recovery evidence entry.
    pub async fn dispute_guardian_recovery(&self, evidence_id: &str, reason: &str) -> Result<()> {
        let mut ledger = GLOBAL_RECOVERY_LEDGER.lock().await;
        let evidence = ledger
            .get_mut(evidence_id)
            .ok_or_else(|| AuraError::not_found(format!("unknown evidence {}", evidence_id)))?;

        let effects = self.effects.read().await;
        let timestamp = effects.current_timestamp().await;
        drop(effects);

        if timestamp > evidence.dispute_window_ends_at {
            return Err(AuraError::invalid(
                "dispute window has already closed for this recovery",
            ));
        }

        let guardian = evidence
            .guardian_profiles
            .iter()
            .find(|profile| profile.device_id == self.device_id)
            .ok_or_else(|| {
                AuraError::permission_denied(
                    "this device did not participate in the recovery ceremony",
                )
            })?;

        if evidence
            .disputes
            .iter()
            .any(|dispute| dispute.guardian_id == guardian.guardian_id)
        {
            return Err(AuraError::invalid(
                "guardian has already filed a dispute for this recovery",
            ));
        }

        let dispute = RecoveryDispute {
            guardian_id: guardian.guardian_id,
            reason: reason.to_string(),
            filed_at: timestamp,
        };
        evidence.disputes.push(dispute.clone());

        drop(ledger);

        let mut log = self.evidence_log.lock().await;
        if let Some(entry) = log.iter_mut().find(|(id, _)| id == evidence_id) {
            entry.1.disputes.push(dispute);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_authenticate::guardian_auth::{RecoveryContext, RecoveryOperationType};
    use aura_recovery::{
        guardian_recovery::{guardian_from_device, RecoveryPriority},
        GuardianSet,
    };

    fn guardian_set(devices: Vec<DeviceId>) -> GuardianSet {
        GuardianSet::new(
            devices
                .into_iter()
                .map(|device| guardian_from_device(device, "test-guardian"))
                .collect(),
        )
    }

    #[tokio::test]
    async fn guardian_can_dispute_within_window() {
        let recovering_device = DeviceId::new();
        let guardian_device = DeviceId::new();
        let other_guardian = DeviceId::new();

        {
            let mut ledger = GLOBAL_RECOVERY_LEDGER.lock().await;
            ledger.clear();
        }

        let effects = Arc::new(RwLock::new(AuraEffectSystem::for_testing(
            recovering_device,
        )));
        let operations = RecoveryOperations::new(effects.clone(), recovering_device);

        let request = GuardianRecoveryRequest {
            requesting_device: recovering_device,
            account_id: aura_core::AccountId::new(),
            recovery_context: RecoveryContext {
                operation_type: RecoveryOperationType::DeviceKeyRecovery,
                justification: "test".into(),
                is_emergency: false,
                timestamp: 0,
            },
            required_threshold: 1,
            available_guardians: guardian_set(vec![guardian_device, other_guardian]),
            priority: RecoveryPriority::Normal,
            dispute_window_secs: 60,
        };

        operations
            .start_guardian_recovery(request)
            .await
            .expect("recovery should succeed");

        let status = operations
            .recovery_status()
            .await
            .expect("status should be available");
        let evidence_id = status
            .latest_evidence_id
            .clone()
            .expect("evidence id should exist");

        let guardian_effects =
            Arc::new(RwLock::new(AuraEffectSystem::for_testing(guardian_device)));
        let guardian_ops = RecoveryOperations::new(guardian_effects, guardian_device);
        guardian_ops
            .dispute_guardian_recovery(&evidence_id, "suspicious request")
            .await
            .expect("guardian should file dispute");

        let ledger = GLOBAL_RECOVERY_LEDGER.lock().await;
        let evidence = ledger.get(&evidence_id).expect("evidence present");
        assert!(
            evidence
                .disputes
                .iter()
                .any(|entry| entry.reason == "suspicious request"),
            "expected dispute to be recorded"
        );
    }
}
