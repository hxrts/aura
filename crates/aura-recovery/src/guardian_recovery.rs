//! Public request/response types for the guardian recovery choreography.

use crate::{
    choreography_impl::{RecoverySessionMetrics, RecoverySessionResult},
    types::{GuardianProfile, GuardianSet, RecoveryShare},
    RecoveryChoreography, RecoveryError, RecoveryResult, RecoveryRole,
};
use aura_authenticate::guardian_auth::{RecoveryContext, RecoveryOperationType};
use aura_core::{identifiers::GuardianId, AccountId, DeviceId};
use aura_protocol::effects::{AuraEffectSystem, TimeEffects, ConsoleEffects};
use aura_verify::session::SessionTicket;
use aura_wot::{CapabilitySet, TreePolicy};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DEFAULT_DISPUTE_WINDOW_SECS: u64 = 48 * 60 * 60;

// GuardianRecoveryCoordinator defined later with full implementation

/// Recovery status enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryStatus {
    /// Recovery is pending guardian approvals
    Pending,
    /// Recovery is active and processing
    Active,
    /// Recovery completed successfully
    Complete,
    /// Recovery was cancelled or failed
    Failed,
}

// GuardianRecoveryResponse defined later with full implementation

/// Guardian recovery request emitted by agents/CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianRecoveryRequest {
    /// Device requesting recovery
    pub requesting_device: DeviceId,
    /// Account being recovered
    pub account_id: AccountId,
    /// Recovery operation details
    pub recovery_context: RecoveryContext,
    /// Guardian threshold required
    pub required_threshold: usize,
    /// Guardians that can approve the flow
    pub available_guardians: GuardianSet,
    /// Recovery priority (normal/urgent/emergency)
    pub priority: RecoveryPriority,
    /// Dispute window in seconds (guardians can object before activation)
    pub dispute_window_secs: u64,
}

/// Recovery priority levels influence timeout/cooldown handling.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RecoveryPriority {
    /// Normal recovery (standard cooldown)
    Normal,
    /// Urgent recovery (shorter cooldown)
    Urgent,
    /// Emergency recovery (no cooldown overrides, but reduced waits)
    Emergency,
}

/// Guardian recovery response surfaced to higher layers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianRecoveryResponse {
    /// Recovery outcome payload
    pub recovery_outcome: RecoveryOutcome,
    /// Guardian approvals received
    pub guardian_approvals: Vec<RecoveryShare>,
    /// Recovery artifacts generated
    pub recovery_artifacts: Vec<RecoveryArtifact>,
    /// Success indicator
    pub success: bool,
    /// Error message if any
    pub error: Option<String>,
    /// Session level metrics (cooldown + timing + flow cost)
    pub metrics: RecoverySessionMetrics,
}

/// Recovery outcome details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryOutcome {
    /// Type of recovery completed
    pub operation_type: RecoveryOperationType,
    /// New session ticket (if applicable)
    pub session_ticket: Option<SessionTicket>,
    /// Recovered key material (if applicable)
    pub key_material: Option<Vec<u8>>,
    /// Account status changes
    pub account_changes: Vec<AccountStatusChange>,
    /// Evidence identifier recorded in the journal/log
    pub evidence_id: String,
}

/// Recovery artifacts generated during process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryArtifact {
    /// Artifact type
    pub artifact_type: ArtifactType,
    /// Artifact content
    pub content: Vec<u8>,
    /// Guardian signatures
    pub signatures: Vec<Vec<u8>>,
    /// Creation timestamp
    pub timestamp: u64,
}

/// Types of recovery artifacts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArtifactType {
    /// Recovery authorization certificate
    RecoveryAuthorization,
    /// Key recovery certificate
    KeyRecoveryCertificate,
    /// Account status change certificate
    AccountStatusCertificate,
    /// Emergency action certificate
    EmergencyActionCertificate,
}

/// Account status changes during recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountStatusChange {
    /// Type of status change
    pub change_type: AccountChangeType,
    /// Previous value
    pub previous_value: String,
    /// New value
    pub new_value: String,
    /// Effective timestamp
    pub effective_at: u64,
}

/// Types of account status changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccountChangeType {
    /// Account freeze status
    FreezeStatus,
    /// Guardian set modification
    GuardianSet,
    /// Recovery policy update
    RecoveryPolicy,
    /// Emergency contact update
    EmergencyContact,
}

/// Recovery policy enforcement configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPolicyConfig {
    /// Minimum guardian threshold for each priority level
    pub threshold_requirements: HashMap<RecoveryPriority, usize>,
    /// Required capabilities for recovery initiation
    pub initiation_capabilities: CapabilitySet,
    /// Required capabilities for guardian approval
    pub approval_capabilities: CapabilitySet,
    /// Maximum recovery attempts per device per epoch
    pub max_recovery_attempts: u32,
    /// Cooldown multipliers for repeated recoveries
    pub cooldown_multipliers: HashMap<RecoveryPriority, f64>,
    /// Trust policy for guardian selection
    pub guardian_trust_policy: TreePolicy,
    /// Emergency override capabilities
    pub emergency_override_capabilities: CapabilitySet,
}

impl Default for RecoveryPolicyConfig {
    fn default() -> Self {
        let mut threshold_requirements = HashMap::new();
        threshold_requirements.insert(RecoveryPriority::Normal, 2);
        threshold_requirements.insert(RecoveryPriority::Urgent, 3);
        threshold_requirements.insert(RecoveryPriority::Emergency, 2);

        let mut cooldown_multipliers = HashMap::new();
        cooldown_multipliers.insert(RecoveryPriority::Normal, 1.0);
        cooldown_multipliers.insert(RecoveryPriority::Urgent, 1.5);
        cooldown_multipliers.insert(RecoveryPriority::Emergency, 0.5);

        Self {
            threshold_requirements,
            initiation_capabilities: CapabilitySet::guardian_recovery_initiation(),
            approval_capabilities: CapabilitySet::guardian_approval(),
            max_recovery_attempts: 3,
            cooldown_multipliers,
            guardian_trust_policy: TreePolicy::default_recovery_trust(),
            emergency_override_capabilities: CapabilitySet::emergency_override(),
        }
    }
}

/// Recovery policy enforcement engine
#[derive(Debug, Clone)]
pub struct RecoveryPolicyEnforcer {
    config: RecoveryPolicyConfig,
    effect_system: AuraEffectSystem,
}

impl RecoveryPolicyEnforcer {
    /// Create new policy enforcer
    pub fn new(config: RecoveryPolicyConfig, effect_system: AuraEffectSystem) -> Self {
        Self {
            config,
            effect_system,
        }
    }

    /// Validate recovery request against policy
    pub async fn validate_recovery_request(
        &self,
        request: &GuardianRecoveryRequest,
    ) -> RecoveryResult<PolicyValidationResult> {
        let mut validation = PolicyValidationResult::new();

        // Check threshold requirements
        if let Some(&required_threshold) = self.config.threshold_requirements.get(&request.priority)
        {
            if request.required_threshold < required_threshold {
                validation.add_violation(PolicyViolation::InsufficientThreshold {
                    required: required_threshold,
                    provided: request.required_threshold,
                    priority: request.priority.clone(),
                });
            }
        }

        // Check recovery attempt limits
        let current_epoch = self.effect_system.current_timestamp_millis().await / (3600 * 1000); // Hour-based epochs
        let attempt_count = self
            .get_recovery_attempts(
                &request.requesting_device,
                &request.account_id,
                current_epoch,
            )
            .await?;

        if attempt_count >= self.config.max_recovery_attempts {
            validation.add_violation(PolicyViolation::TooManyAttempts {
                limit: self.config.max_recovery_attempts,
                current: attempt_count,
                device: request.requesting_device,
            });
        }

        // Check guardian trust policy compliance
        let trust_violations = self
            .validate_guardian_trust(&request.available_guardians)
            .await?;
        validation.extend_violations(trust_violations);

        // Check capabilities for initiation
        let device_capabilities = self
            .get_device_capabilities(&request.requesting_device)
            .await?;
        if !self.config.initiation_capabilities.is_subset_of(&device_capabilities) {
            validation.add_violation(PolicyViolation::MissingCapabilities {
                required: self.config.initiation_capabilities.clone(),
                available: device_capabilities,
                operation: "recovery_initiation".to_string(),
            });
        }

        Ok(validation)
    }

    /// Validate guardian approval against policy
    pub async fn validate_guardian_approval(
        &self,
        guardian_id: &DeviceId,
        request: &GuardianRecoveryRequest,
    ) -> RecoveryResult<PolicyValidationResult> {
        let mut validation = PolicyValidationResult::new();

        // Check guardian capabilities
        let guardian_capabilities = self.get_device_capabilities(guardian_id).await?;
        if !self.config.approval_capabilities.is_subset_of(&guardian_capabilities) {
            validation.add_violation(PolicyViolation::MissingCapabilities {
                required: self.config.approval_capabilities.clone(),
                available: guardian_capabilities.clone(),
                operation: "guardian_approval".to_string(),
            });
        }

        // Check emergency override if applicable
        if matches!(request.priority, RecoveryPriority::Emergency) {
            if !self.config.emergency_override_capabilities.is_subset_of(&guardian_capabilities) {
                validation.add_violation(PolicyViolation::EmergencyOverrideRequired {
                    guardian: *guardian_id,
                    required_capabilities: self.config.emergency_override_capabilities.clone(),
                });
            }
        }

        // Check cooldown multipliers
        let cooldown_multiplier = self
            .config
            .cooldown_multipliers
            .get(&request.priority)
            .copied()
            .unwrap_or(1.0);

        if cooldown_multiplier > 1.0 {
            validation.add_warning(PolicyWarning::CooldownMultiplier {
                guardian: *guardian_id,
                multiplier: cooldown_multiplier,
                priority: request.priority.clone(),
            });
        }

        Ok(validation)
    }

    /// Calculate policy-adjusted cooldown period
    pub fn calculate_cooldown_period(
        &self,
        base_cooldown: u64,
        priority: &RecoveryPriority,
    ) -> u64 {
        let multiplier = self
            .config
            .cooldown_multipliers
            .get(priority)
            .copied()
            .unwrap_or(1.0);
        (base_cooldown as f64 * multiplier) as u64
    }

    /// Get recovery attempt count for device/account in current epoch
    async fn get_recovery_attempts(
        &self,
        device_id: &DeviceId,
        account_id: &AccountId,
        epoch: u64,
    ) -> RecoveryResult<u32> {
        // In production, this would query the journal/ledger
        // For now, return mock data
        let key = format!("recovery_attempts:{}:{}:{}", device_id, account_id, epoch);
        // Mock implementation - would use actual storage
        Ok(0) // No attempts recorded
    }

    /// Get device capabilities for policy checking
    async fn get_device_capabilities(&self, device_id: &DeviceId) -> RecoveryResult<CapabilitySet> {
        // In production, this would query the WoT system
        // For now, return default capabilities
        Ok(CapabilitySet::default_device_capabilities())
    }

    /// Validate guardian set against trust policy
    async fn validate_guardian_trust(
        &self,
        guardian_set: &GuardianSet,
    ) -> RecoveryResult<Vec<PolicyViolation>> {
        let mut violations = Vec::new();

        // Check minimum trust level for each guardian
        for guardian in guardian_set.iter() {
            let trust_score = self.get_guardian_trust_score(&guardian.device_id).await?;
            if trust_score < self.config.guardian_trust_policy.minimum_trust_score() {
                violations.push(PolicyViolation::InsufficientTrust {
                    guardian: guardian.device_id,
                    required_score: self.config.guardian_trust_policy.minimum_trust_score(),
                    actual_score: trust_score,
                });
            }
        }

        Ok(violations)
    }

    /// Get trust score for guardian
    async fn get_guardian_trust_score(&self, guardian_id: &DeviceId) -> RecoveryResult<f64> {
        // In production, this would query the WoT system
        // For now, return a default trust score
        Ok(0.8) // 80% trust score
    }
}

/// Policy validation result
#[derive(Debug, Clone)]
pub struct PolicyValidationResult {
    pub violations: Vec<PolicyViolation>,
    pub warnings: Vec<PolicyWarning>,
    pub is_valid: bool,
}

impl PolicyValidationResult {
    fn new() -> Self {
        Self {
            violations: Vec::new(),
            warnings: Vec::new(),
            is_valid: true,
        }
    }

    fn add_violation(&mut self, violation: PolicyViolation) {
        self.violations.push(violation);
        self.is_valid = false;
    }

    fn add_warning(&mut self, warning: PolicyWarning) {
        self.warnings.push(warning);
    }

    fn extend_violations(&mut self, violations: Vec<PolicyViolation>) {
        if !violations.is_empty() {
            self.is_valid = false;
            self.violations.extend(violations);
        }
    }
}

/// Policy violation types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PolicyViolation {
    InsufficientThreshold {
        required: usize,
        provided: usize,
        priority: RecoveryPriority,
    },
    TooManyAttempts {
        limit: u32,
        current: u32,
        device: DeviceId,
    },
    MissingCapabilities {
        required: CapabilitySet,
        available: CapabilitySet,
        operation: String,
    },
    InsufficientTrust {
        guardian: DeviceId,
        required_score: f64,
        actual_score: f64,
    },
    EmergencyOverrideRequired {
        guardian: DeviceId,
        required_capabilities: CapabilitySet,
    },
}

/// Policy warning types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PolicyWarning {
    CooldownMultiplier {
        guardian: DeviceId,
        multiplier: f64,
        priority: RecoveryPriority,
    },
}

/// Guardian recovery coordinator is a lightweight facade used by the agent/CLI.
#[derive(Clone)]
pub struct GuardianRecoveryCoordinator {
    effect_system: AuraEffectSystem,
    policy_enforcer: RecoveryPolicyEnforcer,
}

impl GuardianRecoveryCoordinator {
    /// Create coordinator for the provided effect system.
    pub fn new(effect_system: AuraEffectSystem) -> Self {
        let policy_config = RecoveryPolicyConfig::default();
        let policy_enforcer = RecoveryPolicyEnforcer::new(policy_config, effect_system.clone());

        Self {
            effect_system,
            policy_enforcer,
        }
    }

    /// Create coordinator with custom policy configuration
    pub fn with_policy_config(
        effect_system: AuraEffectSystem,
        policy_config: RecoveryPolicyConfig,
    ) -> Self {
        let policy_enforcer = RecoveryPolicyEnforcer::new(policy_config, effect_system.clone());

        Self {
            effect_system,
            policy_enforcer,
        }
    }

    /// Execute guardian recovery using the G_recovery choreography with policy enforcement.
    pub async fn execute_recovery(
        &self,
        request: GuardianRecoveryRequest,
    ) -> RecoveryResult<GuardianRecoveryResponse> {
        if request.available_guardians.is_empty() {
            return Err(RecoveryError::invalid(
                "recovery requires at least one guardian".to_string(),
            ));
        }

        // Enforce recovery policy before proceeding
        let validation = self
            .policy_enforcer
            .validate_recovery_request(&request)
            .await?;
        if !validation.is_valid {
            tracing::error!(
                "Recovery policy validation failed: {} violations",
                validation.violations.len()
            );

            for violation in &validation.violations {
                tracing::error!("Policy violation: {:?}", violation);
            }

            return Err(RecoveryError::permission_denied(format!(
                "Recovery request rejected due to policy violations: {}",
                validation.violations.len()
            )));
        }

        // Log policy warnings
        for warning in &validation.warnings {
            tracing::warn!("Policy warning: {:?}", warning);
        }

        let mut choreography = RecoveryChoreography::new(
            RecoveryRole::RecoveringDevice(request.requesting_device),
            request.available_guardians.clone(),
            request.required_threshold,
            self.effect_system.clone(),
        );

        // Execute recovery with policy-aware cooldown calculation
        let mut policy_aware_request = request.clone();
        self.apply_policy_adjustments(&mut policy_aware_request)
            .await?;

        choreography
            .execute_recovery(policy_aware_request.clone())
            .await
            .map(|result| build_recovery_response(policy_aware_request, result))
    }

    /// Apply policy-based adjustments to recovery request
    async fn apply_policy_adjustments(
        &self,
        request: &mut GuardianRecoveryRequest,
    ) -> RecoveryResult<()> {
        // Adjust dispute window based on priority
        match request.priority {
            RecoveryPriority::Emergency => {
                // Emergency recoveries get shorter dispute window
                request.dispute_window_secs = request.dispute_window_secs.min(24 * 60 * 60);
                // Max 24 hours
            }
            RecoveryPriority::Urgent => {
                // Urgent recoveries get standard window
                // No adjustment needed
            }
            RecoveryPriority::Normal => {
                // Normal recoveries get extended window for additional review
                request.dispute_window_secs = request.dispute_window_secs.max(48 * 60 * 60);
                // Min 48 hours
            }
        }

        let _ = self.effect_system.log_info(
            &format!(
                "Applied policy adjustments: dispute_window={}s for {:?} priority",
                request.dispute_window_secs, request.priority
            ),
        ).await;

        Ok(())
    }

    /// Validate guardian approval with policy enforcement
    pub async fn validate_guardian_approval(
        &self,
        guardian_id: &DeviceId,
        request: &GuardianRecoveryRequest,
    ) -> RecoveryResult<PolicyValidationResult> {
        self.policy_enforcer
            .validate_guardian_approval(guardian_id, request)
            .await
    }

    /// Get policy configuration
    pub fn policy_config(&self) -> &RecoveryPolicyConfig {
        &self.policy_enforcer.config
    }

    /// Update policy configuration
    pub fn update_policy_config(&mut self, config: RecoveryPolicyConfig) {
        self.policy_enforcer.config = config;
    }
}

/// Convert a low-level session result into a transport-friendly response.
pub fn build_recovery_response(
    request: GuardianRecoveryRequest,
    result: RecoverySessionResult,
) -> GuardianRecoveryResponse {
    let evidence_id = format!(
        "{}:{}",
        result.evidence.account_id, result.evidence.issued_at
    );

    GuardianRecoveryResponse {
        recovery_outcome: RecoveryOutcome {
            operation_type: request.recovery_context.operation_type,
            session_ticket: None,
            key_material: Some(result.recovered_key.clone()),
            account_changes: Vec::new(),
            evidence_id,
        },
        guardian_approvals: result.guardian_shares,
        recovery_artifacts: vec![RecoveryArtifact {
            artifact_type: ArtifactType::RecoveryAuthorization,
            content: result.threshold_signature.as_bytes().to_vec(),
            signatures: vec![result.threshold_signature.as_bytes().to_vec()],
            timestamp: result.metrics.completed_at,
        }],
        success: true,
        error: None,
        metrics: result.metrics,
    }
}

/// Helper for building guardian profiles directly from CLI/agent input.
pub fn guardian_from_device(device_id: DeviceId, label: impl Into<String>) -> GuardianProfile {
    GuardianProfile::new(GuardianId::new(), device_id, label)
}
