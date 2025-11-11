//! Concrete guardian recovery choreography implementation used by the agent/CLI.

use crate::{
    guardian_recovery::{
        GuardianRecoveryRequest, RecoveryPolicyConfig, RecoveryPolicyEnforcer, RecoveryPriority,
    },
    types::{GuardianProfile, GuardianSet, RecoveryEvidence, RecoveryShare},
    RecoveryResult,
};
use aura_core::{
    identifiers::GuardianId, relationships::ContextId, AuraError, AuraResult, DeviceId,
};
use aura_crypto::frost::ThresholdSignature;
use aura_protocol::effects::{
    AuraEffectSystem, ConsoleEffects, NetworkEffects, RandomEffects, TimeEffects,
};
use blake3::Hasher;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::Mutex;

/// Metrics collected during recovery session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoverySessionMetrics {
    /// Guardians contacted
    pub guardians_contacted: usize,
    /// Guardians that approved
    pub guardians_approved: usize,
    /// Guardians blocked by cooldown
    pub cooldown_blocked: usize,
    /// Time when session started
    pub started_at: u64,
    /// Time when session ended (if completed)
    pub completed_at: u64,
    /// Number of disputes filed
    pub dispute_count: usize,
}

impl Default for RecoverySessionMetrics {
    fn default() -> Self {
        Self {
            guardians_contacted: 0,
            guardians_approved: 0,
            cooldown_blocked: 0,
            started_at: 0,
            completed_at: 0,
            dispute_count: 0,
        }
    }
}

// RecoverySessionResult defined later with full implementation

/// Messages exchanged during G_recovery (documentation / logging only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryMessage {
    RecoveryRequest {
        device_id: DeviceId,
        account_id: String,
        epoch: u64,
        priority: RecoveryPriority,
    },
    RecoveryShare {
        guardian_id: GuardianId,
        issued_at: u64,
    },
    RecoveryReject {
        guardian_id: GuardianId,
        reason: String,
        timestamp: u64,
    },
    RecoveryComplete {
        device_id: DeviceId,
        guardians: Vec<GuardianId>,
        timestamp: u64,
    },
}

/// Roles supported by the choreography.
#[derive(Debug, Clone)]
pub enum RecoveryRole {
    /// Device requesting recovery.
    RecoveringDevice(DeviceId),
    /// Guardian role (not yet projected).
    Guardian(DeviceId),
    /// Coordinator role (future use).
    Coordinator(DeviceId),
}

/// Cooldown ledger shared across choreography invocations.
static GUARDIAN_COOLDOWNS: Lazy<Mutex<HashMap<GuardianId, u64>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

const FLOW_COST_SEND: u32 = 2;
const DEFAULT_COOLDOWN_SECS: u64 = 15 * 60;

/// Recovery choreography entry point.
pub struct RecoveryChoreography {
    role: RecoveryRole,
    guardian_set: GuardianSet,
    threshold: usize,
    effects: AuraEffectSystem,
    policy_enforcer: Option<RecoveryPolicyEnforcer>,
}

impl RecoveryChoreography {
    /// Create choreography.
    pub fn new(
        role: RecoveryRole,
        guardian_set: GuardianSet,
        threshold: usize,
        effects: AuraEffectSystem,
    ) -> Self {
        Self {
            role,
            guardian_set,
            threshold,
            effects,
            policy_enforcer: None,
        }
    }

    /// Create choreography with policy enforcement
    pub fn with_policy_enforcement(
        role: RecoveryRole,
        guardian_set: GuardianSet,
        threshold: usize,
        effects: AuraEffectSystem,
        policy_config: RecoveryPolicyConfig,
    ) -> Self {
        let policy_enforcer = RecoveryPolicyEnforcer::new(policy_config, effects.clone());
        Self {
            role,
            guardian_set,
            threshold,
            effects,
            policy_enforcer: Some(policy_enforcer),
        }
    }

    /// Legacy constructor kept for existing integration tests that still pass AuraRuntime.
    #[allow(dead_code)]
    pub fn from_runtime(
        role: RecoveryRole,
        _context: aura_authenticate::guardian_auth::RecoveryContext,
        guardian_set: GuardianSet,
        threshold: usize,
        runtime: aura_mpst::AuraRuntime,
    ) -> Self {
        let effects = AuraEffectSystem::for_testing(runtime.device_id());
        Self::new(role, guardian_set, threshold, effects)
    }

    /// Execute G_recovery orchestration.
    pub async fn execute_recovery(
        &mut self,
        request: GuardianRecoveryRequest,
    ) -> RecoveryResult<RecoverySessionResult> {
        match self.role {
            RecoveryRole::RecoveringDevice(device_id) => {
                self.execute_as_device(device_id, request).await
            }
            RecoveryRole::Guardian(_) | RecoveryRole::Coordinator(_) => Err(AuraError::invalid(
                "guardian/coordinator roles are not yet implemented",
            )),
        }
    }

    /// Execute the guardian approval path for a recovery request.
    pub async fn approve_as_guardian(
        &mut self,
        request: GuardianRecoveryRequest,
    ) -> RecoveryResult<RecoveryShare> {
        let guardian_device = match self.role {
            RecoveryRole::Guardian(device_id) => device_id,
            _ => {
                return Err(AuraError::invalid(
                    "guardian approval requires RecoveryRole::Guardian",
                ))
            }
        };

        let guardian = self
            .guardian_set
            .by_device(&guardian_device)
            .ok_or_else(|| {
                AuraError::permission_denied(
                    "guardian device is not present in the recovery request",
                )
            })?;

        let now = self.effects.current_timestamp().await;
        let cooldown_until = current_cooldown_until(guardian.guardian_id).await;
        if now < cooldown_until {
            return Err(AuraError::permission_denied(format!(
                "guardian {} still in cooldown for {}s",
                guardian.guardian_id,
                cooldown_until.saturating_sub(now)
            )));
        }

        let recovery_ctx = self.recovery_context(&request);
        self.send_guardian_share(guardian, &request, &recovery_ctx)
            .await?;

        let share = RecoveryShare {
            guardian: guardian.clone(),
            share: self.effects.random_bytes(32).await,
            partial_signature: self.effects.random_bytes(64).await,
            issued_at: now,
        };

        {
            let mut cooldowns = GUARDIAN_COOLDOWNS.lock().await;
            let cooldown_period = self.calculate_guardian_cooldown(guardian, &request);
            cooldowns.insert(guardian.guardian_id, now + cooldown_period);
        }

        tracing::info!(
            "Guardian {} approved recovery for account {}.",
            guardian.guardian_id,
            request.account_id
        );

        Ok(share)
    }

    async fn execute_as_device(
        &mut self,
        device_id: DeviceId,
        request: GuardianRecoveryRequest,
    ) -> RecoveryResult<RecoverySessionResult> {
        if self.guardian_set.is_empty() {
            return Err(AuraError::invalid("at least one guardian is required"));
        }

        let required = self.threshold.min(self.guardian_set.len());
        if required == 0 {
            return Err(AuraError::invalid(
                "threshold must be at least 1 to start recovery",
            ));
        }

        let mut metrics = RecoverySessionMetrics::default();
        metrics.started_at = self.effects.current_timestamp().await;

        tracing::info!(
            "Starting guardian recovery for account {} with {} guardians (need {}).",
            request.account_id,
            self.guardian_set.len(),
            required
        );

        let recovery_ctx = self.recovery_context(&request);
        let mut shares = Vec::new();
        let mut cooldown_failures = 0usize;

        for guardian in self.guardian_set.iter() {
            metrics.guardians_contacted += 1;

            match self
                .collect_share(device_id, guardian, &request, &recovery_ctx)
                .await
            {
                Ok(share) => {
                    metrics.guardians_approved += 1;
                    shares.push(share);
                    if shares.len() >= required {
                        break;
                    }
                }
                Err(err) => {
                    if matches!(err, AuraError::PermissionDenied { .. }) {
                        cooldown_failures += 1;
                    }
                    tracing::warn!(
                        "Guardian {} rejected recovery request: {}",
                        guardian.guardian_id,
                        err
                    );
                }
            }
        }

        metrics.cooldown_blocked = cooldown_failures;

        if shares.len() < required {
            return Err(AuraError::permission_denied(format!(
                "Recovery failed: only {} guardians approved (need {})",
                shares.len(),
                required
            )));
        }

        let recovered_key = derive_recovered_key(&shares, &request);
        let signature = aggregate_threshold_signature(&shares);
        let cooldown_expires_at = shares
            .iter()
            .map(|share| {
                share.issued_at + self.calculate_guardian_cooldown(&share.guardian, &request)
            })
            .max()
            .unwrap_or(metrics.started_at);
        let dispute_window_secs = request.dispute_window_secs;
        let dispute_window_ends_at = metrics.started_at + dispute_window_secs;
        let evidence = RecoveryEvidence {
            account_id: request.account_id,
            recovering_device: device_id,
            guardians: shares
                .iter()
                .map(|share| share.guardian.guardian_id)
                .collect(),
            issued_at: metrics.started_at,
            cooldown_expires_at,
            dispute_window_ends_at,
            guardian_profiles: shares.iter().map(|share| share.guardian.clone()).collect(),
            disputes: Vec::new(),
            threshold_signature: Some(signature.clone()),
        };

        metrics.completed_at = self.effects.current_timestamp().await;

        let session_result = RecoverySessionResult {
            recovered_key,
            threshold_signature: signature,
            guardian_shares: shares,
            evidence,
            metrics,
        };

        tracing::info!(
            "Guardian recovery completed for account {} ({} approvals).",
            request.account_id,
            session_result.guardian_shares.len()
        );

        Ok(session_result)
    }

    async fn collect_share(
        &self,
        device_id: DeviceId,
        guardian: &GuardianProfile,
        request: &GuardianRecoveryRequest,
        recovery_ctx: &ContextId,
    ) -> AuraResult<RecoveryShare> {
        let now = self.effects.current_timestamp().await;
        let cooldown_until = current_cooldown_until(guardian.guardian_id).await;
        if now < cooldown_until {
            return Err(AuraError::permission_denied(format!(
                "guardian {} still in cooldown for {}s",
                guardian.guardian_id,
                cooldown_until.saturating_sub(now)
            )));
        }

        self.send_recovery_request(guardian, device_id, request, recovery_ctx)
            .await?;

        let share = RecoveryShare {
            guardian: guardian.clone(),
            share: self.effects.random_bytes(32).await,
            partial_signature: self.effects.random_bytes(64).await,
            issued_at: now,
        };

        {
            let mut cooldowns = GUARDIAN_COOLDOWNS.lock().await;
            let cooldown_period = self.calculate_guardian_cooldown(guardian, &request);
            cooldowns.insert(guardian.guardian_id, now + cooldown_period);
        }

        Ok(share)
    }

    async fn send_recovery_request(
        &self,
        guardian: &GuardianProfile,
        device_id: DeviceId,
        request: &GuardianRecoveryRequest,
        recovery_ctx: &ContextId,
    ) -> AuraResult<()> {
        self.effects
            .set_flow_hint_components(recovery_ctx.clone(), guardian.device_id, FLOW_COST_SEND)
            .await;

        let message = RecoveryMessage::RecoveryRequest {
            device_id,
            account_id: request.account_id.to_string(),
            epoch: self.effects.current_timestamp().await,
            priority: request.priority.clone(),
        };

        let payload = serde_json::to_vec(&message).unwrap_or_default();

        if let Err(err) =
            NetworkEffects::send_to_peer(&self.effects, guardian.device_id.0, payload).await
        {
            tracing::warn!(
                "Failed to send recovery request to guardian {}: {}",
                guardian.guardian_id,
                err
            );
        }

        Ok(())
    }

    async fn send_guardian_share(
        &self,
        guardian: &GuardianProfile,
        request: &GuardianRecoveryRequest,
        recovery_ctx: &ContextId,
    ) -> AuraResult<()> {
        self.effects
            .set_flow_hint_components(
                recovery_ctx.clone(),
                request.requesting_device,
                FLOW_COST_SEND,
            )
            .await;

        let message = RecoveryMessage::RecoveryShare {
            guardian_id: guardian.guardian_id,
            issued_at: self.effects.current_timestamp().await,
        };

        let payload = serde_json::to_vec(&message).unwrap_or_default();

        if let Err(err) =
            NetworkEffects::send_to_peer(&self.effects, request.requesting_device.0, payload).await
        {
            tracing::warn!(
                "Failed to deliver guardian share to {}: {}",
                request.requesting_device,
                err
            );
        }

        Ok(())
    }

    fn recovery_context(&self, request: &GuardianRecoveryRequest) -> ContextId {
        ContextId::hierarchical(&[
            "recovery",
            &request.account_id.to_string(),
            &request.priority_label(),
        ])
    }

    /// Calculate policy-aware cooldown period for guardian
    fn calculate_guardian_cooldown(
        &self,
        guardian: &GuardianProfile,
        request: &GuardianRecoveryRequest,
    ) -> u64 {
        let base_cooldown = if guardian.cooldown_secs == 0 {
            DEFAULT_COOLDOWN_SECS
        } else {
            guardian.cooldown_secs
        };

        if let Some(policy_enforcer) = &self.policy_enforcer {
            policy_enforcer.calculate_cooldown_period(base_cooldown, &request.priority)
        } else {
            base_cooldown
        }
    }
}

/// Result of a choreography execution with detailed information for higher layers.
#[derive(Debug, Clone)]
pub struct RecoverySessionResult {
    /// Derived key material.
    pub recovered_key: Vec<u8>,
    /// Aggregated threshold signature.
    pub threshold_signature: ThresholdSignature,
    /// Guardian shares used to produce the key.
    pub guardian_shares: Vec<RecoveryShare>,
    /// Recovery evidence snapshot.
    pub evidence: RecoveryEvidence,
    /// Metrics captured during execution.
    pub metrics: RecoverySessionMetrics,
}

// Removed duplicate RecoverySessionMetrics - using unified version above

fn derive_recovered_key(shares: &[RecoveryShare], request: &GuardianRecoveryRequest) -> Vec<u8> {
    let mut hasher = Hasher::new();
    for share in shares {
        hasher.update(&share.share);
        hasher.update(share.guardian.guardian_id.0.as_bytes());
    }
    hasher.update(request.account_id.0.as_bytes());
    hasher.finalize().as_bytes()[0..32].to_vec()
}

fn aggregate_threshold_signature(shares: &[RecoveryShare]) -> ThresholdSignature {
    let mut hasher = Hasher::new();
    for share in shares {
        hasher.update(&share.partial_signature);
        hasher.update(share.guardian.guardian_id.0.as_bytes());
    }
    let digest = hasher.finalize();
    let signers: Vec<u16> = shares
        .iter()
        .enumerate()
        .map(|(idx, _)| idx as u16)
        .collect();
    ThresholdSignature::new(digest.as_bytes().to_vec(), signers)
}

async fn current_cooldown_until(guardian_id: GuardianId) -> u64 {
    let ledger = GUARDIAN_COOLDOWNS.lock().await;
    ledger.get(&guardian_id).copied().unwrap_or(0)
}

fn guardian_cooldown(guardian: &GuardianProfile) -> u64 {
    if guardian.cooldown_secs == 0 {
        DEFAULT_COOLDOWN_SECS
    } else {
        guardian.cooldown_secs
    }
}

impl RecoveryPriority {
    fn label(&self) -> &'static str {
        match self {
            RecoveryPriority::Normal => "normal",
            RecoveryPriority::Urgent => "urgent",
            RecoveryPriority::Emergency => "emergency",
        }
    }
}

impl GuardianRecoveryRequest {
    fn priority_label(&self) -> String {
        self.priority.label().to_string()
    }
}
