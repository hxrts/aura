//! Public request/response types for the guardian recovery choreography.

use crate::{
    choreography_impl::{RecoverySessionMetrics, RecoverySessionResult},
    types::{GuardianProfile, GuardianSet, RecoveryShare},
    RecoveryChoreography, RecoveryError, RecoveryResult, RecoveryRole,
};
use aura_authenticate::guardian_auth::{RecoveryContext, RecoveryOperationType};
use aura_core::{identifiers::GuardianId, AccountId, DeviceId};
use aura_protocol::effects::AuraEffectSystem;
use aura_verify::session::SessionTicket;
use serde::{Deserialize, Serialize};

pub const DEFAULT_DISPUTE_WINDOW_SECS: u64 = 48 * 60 * 60;
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Guardian recovery coordinator is a lightweight facade used by the agent/CLI.
#[derive(Clone)]
pub struct GuardianRecoveryCoordinator {
    effect_system: AuraEffectSystem,
}

impl GuardianRecoveryCoordinator {
    /// Create coordinator for the provided effect system.
    pub fn new(effect_system: AuraEffectSystem) -> Self {
        Self { effect_system }
    }

    /// Execute guardian recovery using the G_recovery choreography.
    pub async fn execute_recovery(
        &self,
        request: GuardianRecoveryRequest,
    ) -> RecoveryResult<GuardianRecoveryResponse> {
        if request.available_guardians.is_empty() {
            return Err(RecoveryError::invalid(
                "recovery requires at least one guardian".to_string(),
            ));
        }

        let mut choreography = RecoveryChoreography::new(
            RecoveryRole::RecoveringDevice(request.requesting_device),
            request.available_guardians.clone(),
            request.required_threshold,
            self.effect_system.clone(),
        );

        choreography
            .execute_recovery(request.clone())
            .await
            .map(|result| build_recovery_response(request, result))
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
