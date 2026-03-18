//! Ceremony workflows (portable)
//!
//! Provides portable helpers for starting/polling/canceling Category C ceremonies.

#![allow(missing_docs)] // Ceremony workflow types are self-documenting

use std::sync::Arc;

use async_lock::RwLock;

use super::error::{ceremony_op, WorkflowError};
use crate::runtime_bridge::KeyRotationCeremonyStatus;
use crate::ui_contract::{
    OperationId, SemanticFailureCode, SemanticFailureDomain, SemanticOperationError,
    SemanticOperationKind, SemanticOperationPhase,
};
use crate::workflows::semantic_facts::{
    publish_authoritative_operation_failure, publish_authoritative_operation_phase,
    semantic_lifecycle_publication_capability,
};
use crate::AppCore;
use aura_core::types::identifiers::{AuthorityId, CeremonyId};
use aura_core::types::FrostThreshold;
use aura_core::{AttemptBudget, AuraError};
use std::future::Future;
use std::time::Duration;

async fn fail_start_device_enrollment<T>(
    app_core: &Arc<RwLock<AppCore>>,
    detail: impl Into<String>,
) -> Result<T, AuraError> {
    let error = SemanticOperationError::new(
        SemanticFailureDomain::Internal,
        SemanticFailureCode::InternalError,
    )
    .with_detail(detail.into());
    publish_authoritative_operation_failure(
        app_core,
        semantic_lifecycle_publication_capability(),
        OperationId::device_enrollment(),
        SemanticOperationKind::StartDeviceEnrollment,
        error.clone(),
    )
    .await?;
    Err(AuraError::agent(error.detail.unwrap_or_else(|| {
        "start device enrollment failed".to_string()
    })))
}

/// Start a guardian key-rotation ceremony.
pub async fn start_guardian_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    threshold_k: FrostThreshold,
    total_n: u16,
    guardian_ids: Vec<AuthorityId>,
) -> Result<CeremonyId, AuraError> {
    let core = app_core.read().await;
    core.initiate_guardian_ceremony(threshold_k, total_n, &guardian_ids)
        .await
        .map_err(|e| ceremony_op("start guardian ceremony", e).into())
}

/// Start a device threshold (multifactor) ceremony.
pub async fn start_device_threshold_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    threshold_k: FrostThreshold,
    total_n: u16,
    device_ids: Vec<String>,
) -> Result<CeremonyId, AuraError> {
    let core = app_core.read().await;
    core.initiate_device_threshold_ceremony(threshold_k, total_n, &device_ids)
        .await
        .map_err(|e| ceremony_op("start device threshold ceremony", e).into())
}

/// Start a device enrollment ("add device") ceremony.
///
/// For the two-step exchange flow:
/// 1. The new device creates its own authority first
/// 2. The new device shares its authority_id with the initiator
/// 3. The initiator passes the invitee's authority_id here
/// 4. An addressed enrollment invitation is created
///
/// # Arguments
/// * `nickname_suggestion` - Suggested name for the device
/// * `invitee_authority_id` - The authority ID of the new device (if known)
pub async fn start_device_enrollment_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_suggestion: String,
    invitee_authority_id: Option<AuthorityId>,
) -> Result<crate::runtime_bridge::DeviceEnrollmentStart, AuraError> {
    publish_authoritative_operation_phase(
        app_core,
        semantic_lifecycle_publication_capability(),
        OperationId::device_enrollment(),
        SemanticOperationKind::StartDeviceEnrollment,
        SemanticOperationPhase::WorkflowDispatched,
    )
    .await?;
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .cloned()
            .ok_or_else(|| AuraError::from(WorkflowError::RuntimeUnavailable))?
    };
    let start = match runtime
        .initiate_device_enrollment_ceremony(nickname_suggestion, invitee_authority_id)
        .await
    {
        Ok(start) => start,
        Err(error) => {
            return fail_start_device_enrollment(
                app_core,
                ceremony_op("start device enrollment", error).to_string(),
            )
            .await;
        }
    };
    publish_authoritative_operation_phase(
        app_core,
        semantic_lifecycle_publication_capability(),
        OperationId::device_enrollment(),
        SemanticOperationKind::StartDeviceEnrollment,
        SemanticOperationPhase::Succeeded,
    )
    .await?;
    Ok(start)
}
/// Start a device removal ("remove device") ceremony.
pub async fn start_device_removal_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    device_id: String,
) -> Result<CeremonyId, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .cloned()
            .ok_or_else(|| AuraError::from(WorkflowError::RuntimeUnavailable))?
    };
    runtime
        .initiate_device_removal_ceremony(device_id)
        .await
        .map_err(|e| ceremony_op("start device removal", e).into())
}

/// Polling policy for ceremonies.
#[derive(Debug, Clone)]
pub struct CeremonyPollPolicy {
    /// Delay between polls.
    pub interval: Duration,
    /// Max number of poll attempts.
    pub max_attempts: u32,
    /// Whether to attempt rollback on failure (key rotation only).
    pub rollback_on_failure: bool,
    /// Whether to refresh settings after completion.
    pub refresh_settings_on_complete: bool,
}

impl CeremonyPollPolicy {
    pub fn with_interval(interval: Duration) -> Self {
        Self {
            interval,
            ..Default::default()
        }
    }
}

impl Default for CeremonyPollPolicy {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(1),
            max_attempts: 60,
            rollback_on_failure: true,
            refresh_settings_on_complete: true,
        }
    }
}

/// Lifecycle outcome for a ceremony monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CeremonyLifecycleState {
    Completed,
    Failed,
    /// The ceremony failed and the best-effort rollback also failed.
    /// The account may be in a partially-committed state that requires
    /// manual intervention or a fresh ceremony to resolve.
    FailedRollbackIncomplete,
    TimedOut,
}

/// Lifecycle result for a ceremony monitor.
#[derive(Debug, Clone)]
pub struct CeremonyLifecycle<T> {
    pub state: CeremonyLifecycleState,
    pub status: T,
    pub attempts: u32,
}

/// Common interface for ceremony status values.
pub trait CeremonyStatusLike {
    fn is_complete(&self) -> bool;
    fn has_failed(&self) -> bool;
}

impl CeremonyStatusLike for KeyRotationCeremonyStatus {
    fn is_complete(&self) -> bool {
        self.is_complete
    }

    fn has_failed(&self) -> bool {
        self.has_failed
    }
}

/// Get status of a key rotation ceremony (generic form).
pub async fn get_key_rotation_ceremony_status(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: &CeremonyId,
) -> Result<KeyRotationCeremonyStatus, AuraError> {
    let core = app_core.read().await;
    core.get_key_rotation_ceremony_status(ceremony_id)
        .await
        .map_err(|e| ceremony_op("get ceremony status", e).into())
}

/// Cancel a key rotation ceremony (best effort).
///
/// # Ownership contract
///
/// Today this accepts a bare `CeremonyId`, which means multiple callers can
/// race cancel against poll or status queries.  The target ownership model
/// requires a `MoveOwned` ceremony handle returned by the start function that
/// is consumed on cancel — preventing concurrent cancel/poll races by
/// construction.  Until that migration is complete, callers must coordinate
/// externally to avoid issuing cancel and poll concurrently on the same
/// ceremony.
pub async fn cancel_key_rotation_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: &CeremonyId,
) -> Result<(), AuraError> {
    let core = app_core.read().await;
    core.cancel_key_rotation_ceremony(ceremony_id)
        .await
        .map_err(|e| ceremony_op("cancel ceremony", e).into())
}

/// Poll a key rotation ceremony until completion or failure using a policy.
///
/// This is a portable (frontend-agnostic) helper for driving ceremony progress UIs.
/// Callers provide an `on_update` hook to receive intermediate statuses.
pub async fn monitor_key_rotation_ceremony_with_policy<SleepFn, SleepFut>(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: CeremonyId,
    policy: CeremonyPollPolicy,
    mut on_update: impl FnMut(&KeyRotationCeremonyStatus) + Send,
    mut sleep_fn: SleepFn,
) -> Result<CeremonyLifecycle<KeyRotationCeremonyStatus>, AuraError>
where
    SleepFn: FnMut(Duration) -> SleepFut + Send,
    SleepFut: Future<Output = ()> + Send,
{
    // Bounded polling to avoid infinite loops; UIs can re-invoke if desired.
    let mut attempts = AttemptBudget::new(policy.max_attempts);
    while attempts.can_attempt() {
        let attempt = attempts
            .record_attempt()
            .map_err(AuraError::from)?
            .saturating_add(1);
        sleep_fn(policy.interval).await;

        let status = get_key_rotation_ceremony_status(app_core, &ceremony_id).await?;
        on_update(&status);

        if status.has_failed {
            // Best-effort rollback for rotations (until runtime owns this fully).
            let mut rollback_failed = false;
            if policy.rollback_on_failure {
                if let Some(epoch) = status.pending_epoch {
                    if matches!(
                        status.kind,
                        crate::runtime_bridge::CeremonyKind::GuardianRotation
                            | crate::runtime_bridge::CeremonyKind::DeviceRotation
                    ) {
                        let core = app_core.read().await;
                        if let Err(e) = core.rollback_guardian_key_rotation(epoch).await {
                            #[cfg(feature = "instrumented")]
                            tracing::error!(
                                error = %e,
                                ceremony_id = %ceremony_id,
                                epoch = ?epoch,
                                "ceremony rollback failed — account may be in partially-committed state"
                            );
                            let _ = &e;
                            rollback_failed = true;
                        }
                    }
                }
            }
            return Ok(CeremonyLifecycle {
                state: if rollback_failed {
                    CeremonyLifecycleState::FailedRollbackIncomplete
                } else {
                    CeremonyLifecycleState::Failed
                },
                status,
                attempts: attempt,
            });
        }

        if status.is_complete {
            // Best-effort: refresh settings so device list / counts update after a commit.
            if policy.refresh_settings_on_complete {
                if let Err(_e) =
                    crate::workflows::settings::refresh_settings_from_runtime(app_core).await
                {
                    #[cfg(feature = "instrumented")]
                    tracing::warn!(
                        error = %_e,
                        ceremony_id = %ceremony_id,
                        "settings refresh failed after ceremony completion — UI may show stale device counts"
                    );
                }
            }
            return Ok(CeremonyLifecycle {
                state: CeremonyLifecycleState::Completed,
                status,
                attempts: attempt,
            });
        }
    }

    // Timed out; return the latest status we can fetch.
    let status = get_key_rotation_ceremony_status(app_core, &ceremony_id).await?;
    Ok(CeremonyLifecycle {
        state: CeremonyLifecycleState::TimedOut,
        status,
        attempts: policy.max_attempts,
    })
}

/// Poll a key rotation ceremony until completion or failure.
///
/// This is a portable (frontend-agnostic) helper for driving ceremony progress UIs.
/// Callers provide an `on_update` hook to receive intermediate statuses.
pub async fn monitor_key_rotation_ceremony<SleepFn, SleepFut>(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: CeremonyId,
    poll_interval: Duration,
    mut on_update: impl FnMut(&KeyRotationCeremonyStatus) + Send,
    mut sleep_fn: SleepFn,
) -> Result<KeyRotationCeremonyStatus, AuraError>
where
    SleepFn: FnMut(Duration) -> SleepFut + Send,
    SleepFut: Future<Output = ()> + Send,
{
    let policy = CeremonyPollPolicy::with_interval(poll_interval);
    let lifecycle = monitor_key_rotation_ceremony_with_policy(
        app_core,
        ceremony_id,
        policy,
        &mut on_update,
        &mut sleep_fn,
    )
    .await?;

    Ok(lifecycle.status)
}
