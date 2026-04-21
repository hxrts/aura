//! Ceremony workflows (portable)
//!
//! Provides portable helpers for starting/polling/canceling Category C ceremonies.

#![allow(missing_docs)] // Ceremony workflow types are self-documenting

use std::sync::Arc;

use async_lock::RwLock;

use super::error::{ceremony_op, WorkflowError};
use crate::core::IntentError;
use crate::runtime_bridge::KeyRotationCeremonyStatus;
use crate::ui_contract::{
    OperationId, OperationInstanceId, SemanticFailureCode, SemanticFailureDomain,
    SemanticOperationError, SemanticOperationKind, SemanticOperationPhase,
};
use crate::workflows::runtime::{timeout_runtime_call, workflow_retry_policy};
use crate::workflows::semantic_facts::{
    issue_device_enrollment_started_proof, SemanticWorkflowOwner,
};
use crate::AppCore;
use aura_core::types::identifiers::{AuthorityId, CeremonyId};
use aura_core::types::FrostThreshold;
use aura_core::{AttemptBudget, AuraError, OperationContext, TraceContext};
use std::future::Future;
use std::time::Duration;

const DEVICE_ENROLLMENT_START_TIMEOUT: Duration = Duration::from_millis(30_000);
const DEVICE_REMOVAL_START_TIMEOUT: Duration = Duration::from_millis(20_000);

fn ceremony_start_timeout(kind: crate::runtime_bridge::CeremonyKind) -> Duration {
    match kind {
        crate::runtime_bridge::CeremonyKind::DeviceEnrollment => DEVICE_ENROLLMENT_START_TIMEOUT,
        crate::runtime_bridge::CeremonyKind::DeviceRemoval => DEVICE_REMOVAL_START_TIMEOUT,
        crate::runtime_bridge::CeremonyKind::GuardianRotation
        | crate::runtime_bridge::CeremonyKind::DeviceRotation => Duration::from_secs(30),
        crate::runtime_bridge::CeremonyKind::Recovery
        | crate::runtime_bridge::CeremonyKind::OtaActivation => Duration::from_secs(45),
        crate::runtime_bridge::CeremonyKind::Invitation
        | crate::runtime_bridge::CeremonyKind::RendezvousSecureChannel => Duration::from_secs(15),
    }
}

fn ceremony_monitor_timeout(kind: crate::runtime_bridge::CeremonyKind) -> Duration {
    match kind {
        crate::runtime_bridge::CeremonyKind::GuardianRotation
        | crate::runtime_bridge::CeremonyKind::DeviceRotation => Duration::from_secs(60),
        crate::runtime_bridge::CeremonyKind::DeviceEnrollment
        | crate::runtime_bridge::CeremonyKind::DeviceRemoval => Duration::from_secs(45),
        crate::runtime_bridge::CeremonyKind::Recovery
        | crate::runtime_bridge::CeremonyKind::OtaActivation => Duration::from_secs(90),
        crate::runtime_bridge::CeremonyKind::Invitation
        | crate::runtime_bridge::CeremonyKind::RendezvousSecureChannel => Duration::from_secs(20),
    }
}

fn ceremony_monitor_attempts(kind: crate::runtime_bridge::CeremonyKind, interval: Duration) -> u32 {
    let interval_ms = interval.as_millis().max(1);
    let window_ms = ceremony_monitor_timeout(kind).as_millis();
    let attempts = window_ms.div_ceil(interval_ms).saturating_add(2);
    u32::try_from(attempts).unwrap_or(u32::MAX)
}

fn ceremony_start_retry_policy(
    kind: crate::runtime_bridge::CeremonyKind,
) -> Result<aura_core::RetryBudgetPolicy, AuraError> {
    let (attempts, initial_delay, max_delay) = match kind {
        crate::runtime_bridge::CeremonyKind::DeviceEnrollment => {
            (4, Duration::from_millis(250), Duration::from_secs(1))
        }
        crate::runtime_bridge::CeremonyKind::DeviceRemoval => {
            (3, Duration::from_millis(150), Duration::from_millis(750))
        }
        _ => (3, Duration::from_millis(200), Duration::from_millis(750)),
    };
    workflow_retry_policy(attempts, initial_delay, max_delay).map_err(AuraError::from)
}

fn retryable_ceremony_intent_error(error: &IntentError) -> bool {
    matches!(
        error,
        IntentError::NetworkError { .. } | IntentError::ServiceError { .. }
    )
}

async fn start_ceremony_with_retry<T, F, Fut>(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    kind: crate::runtime_bridge::CeremonyKind,
    operation: &'static str,
    stage: &'static str,
    mut call: F,
) -> Result<Result<T, IntentError>, AuraError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, IntentError>>,
{
    let policy = ceremony_start_retry_policy(kind)?;
    let mut attempts = AttemptBudget::new(policy.max_attempts());

    loop {
        let attempt = attempts.record_attempt().map_err(AuraError::from)?;
        match timeout_runtime_call(
            runtime,
            operation,
            stage,
            ceremony_start_timeout(kind),
            &mut call,
        )
        .await
        {
            Ok(Ok(value)) => return Ok(Ok(value)),
            Ok(Err(error)) if retryable_ceremony_intent_error(&error) && attempts.can_attempt() => {
                let delay_ms = u64::try_from(policy.delay_for_attempt(attempt).as_millis())
                    .unwrap_or(u64::MAX);
                runtime.sleep_ms(delay_ms).await;
            }
            Ok(Err(error)) => return Ok(Err(error)),
            Err(error) if error.is_retryable() && attempts.can_attempt() => {
                let delay_ms = u64::try_from(policy.delay_for_attempt(attempt).as_millis())
                    .unwrap_or(u64::MAX);
                runtime.sleep_ms(delay_ms).await;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn start_device_enrollment_from_runtime(
    runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    nickname_suggestion: String,
    invitee_authority_id: AuthorityId,
) -> Result<Result<crate::runtime_bridge::DeviceEnrollmentStart, IntentError>, AuraError> {
    let retry_runtime = runtime.clone();
    start_ceremony_with_retry(
        &runtime,
        crate::runtime_bridge::CeremonyKind::DeviceEnrollment,
        "start_device_enrollment_ceremony",
        "initiate_device_enrollment_ceremony",
        move || {
            let runtime = retry_runtime.clone();
            let nickname_suggestion = nickname_suggestion.clone();
            async move {
                runtime
                    .initiate_device_enrollment_ceremony(nickname_suggestion, invitee_authority_id)
                    .await
            }
        },
    )
    .await
}

async fn start_device_removal_from_runtime(
    runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    device_id: String,
) -> Result<Result<CeremonyId, IntentError>, AuraError> {
    let retry_runtime = runtime.clone();
    start_ceremony_with_retry(
        &runtime,
        crate::runtime_bridge::CeremonyKind::DeviceRemoval,
        "start_device_removal_ceremony",
        "initiate_device_removal_ceremony",
        move || {
            let runtime = retry_runtime.clone();
            let device_id = device_id.clone();
            async move { runtime.initiate_device_removal_ceremony(device_id).await }
        },
    )
    .await
}

async fn start_guardian_ceremony_from_runtime(
    runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    threshold_k: FrostThreshold,
    total_n: u16,
    guardian_ids: Vec<AuthorityId>,
) -> Result<Result<CeremonyId, IntentError>, AuraError> {
    let retry_runtime = runtime.clone();
    start_ceremony_with_retry(
        &runtime,
        crate::runtime_bridge::CeremonyKind::GuardianRotation,
        "start_guardian_ceremony",
        "initiate_guardian_ceremony",
        move || {
            let runtime = retry_runtime.clone();
            let guardian_ids = guardian_ids.clone();
            async move {
                runtime
                    .initiate_guardian_ceremony(threshold_k, total_n, &guardian_ids)
                    .await
            }
        },
    )
    .await
}

async fn start_device_threshold_ceremony_from_runtime(
    runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    threshold_k: FrostThreshold,
    total_n: u16,
    device_ids: Vec<String>,
) -> Result<Result<CeremonyId, IntentError>, AuraError> {
    let retry_runtime = runtime.clone();
    start_ceremony_with_retry(
        &runtime,
        crate::runtime_bridge::CeremonyKind::DeviceRotation,
        "start_device_threshold_ceremony",
        "initiate_device_threshold_ceremony",
        move || {
            let runtime = retry_runtime.clone();
            let device_ids = device_ids.clone();
            async move {
                runtime
                    .initiate_device_threshold_ceremony(threshold_k, total_n, &device_ids)
                    .await
            }
        },
    )
    .await
}

/// Move-owned ceremony handle.
///
/// This is the canonical owner token for parity-critical key rotation and
/// membership-change ceremonies. Cancellation consumes the handle so callers
/// cannot issue multiple cancels on the same owned ceremony instance.
#[aura_macros::strong_reference(domain = "ceremony")]
#[derive(Debug)]
pub struct CeremonyHandle {
    ceremony_id: CeremonyId,
    kind: crate::runtime_bridge::CeremonyKind,
}

#[derive(Debug, Clone)]
pub struct CeremonyStatusHandle {
    ceremony_id: CeremonyId,
    kind: crate::runtime_bridge::CeremonyKind,
}

impl CeremonyHandle {
    fn new(ceremony_id: CeremonyId, kind: crate::runtime_bridge::CeremonyKind) -> Self {
        Self { ceremony_id, kind }
    }

    pub fn ceremony_id(&self) -> &CeremonyId {
        &self.ceremony_id
    }

    pub fn kind(&self) -> crate::runtime_bridge::CeremonyKind {
        self.kind
    }

    pub fn status_handle(&self) -> CeremonyStatusHandle {
        CeremonyStatusHandle::new(self.ceremony_id.clone(), self.kind)
    }
}

impl CeremonyStatusHandle {
    fn new(ceremony_id: CeremonyId, kind: crate::runtime_bridge::CeremonyKind) -> Self {
        Self { ceremony_id, kind }
    }

    pub fn ceremony_id(&self) -> &CeremonyId {
        &self.ceremony_id
    }

    pub fn kind(&self) -> crate::runtime_bridge::CeremonyKind {
        self.kind
    }
}

#[derive(Debug)]
pub struct DeviceEnrollmentCeremonyStart {
    pub ceremony_id: CeremonyId,
    pub enrollment_code: String,
    pub pending_epoch: aura_core::types::Epoch,
    pub device_id: aura_core::types::identifiers::DeviceId,
    pub handle: CeremonyHandle,
    pub status_handle: CeremonyStatusHandle,
}

async fn fail_start_device_enrollment<T>(
    owner: &SemanticWorkflowOwner,
    detail: impl Into<String>,
) -> Result<T, AuraError> {
    let error = SemanticOperationError::new(
        SemanticFailureDomain::Internal,
        SemanticFailureCode::InternalError,
    )
    .with_detail(detail.into());
    owner.publish_failure(error.clone()).await?;
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
) -> Result<CeremonyHandle, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .cloned()
            .ok_or_else(|| AuraError::from(WorkflowError::RuntimeUnavailable))?
    };
    start_guardian_ceremony_from_runtime(runtime, threshold_k, total_n, guardian_ids)
        .await?
        .map(|ceremony_id| {
            CeremonyHandle::new(
                ceremony_id,
                crate::runtime_bridge::CeremonyKind::GuardianRotation,
            )
        })
        .map_err(|e| ceremony_op("start guardian ceremony", e).into())
}

/// Start a device threshold (multifactor) ceremony.
pub async fn start_device_threshold_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    threshold_k: FrostThreshold,
    total_n: u16,
    device_ids: Vec<String>,
) -> Result<CeremonyHandle, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .cloned()
            .ok_or_else(|| AuraError::from(WorkflowError::RuntimeUnavailable))?
    };
    start_device_threshold_ceremony_from_runtime(runtime, threshold_k, total_n, device_ids)
        .await?
        .map(|ceremony_id| {
            CeremonyHandle::new(
                ceremony_id,
                crate::runtime_bridge::CeremonyKind::DeviceRotation,
            )
        })
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
/// * `invitee_authority_id` - The authority ID of the new device
pub async fn start_device_enrollment_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_suggestion: String,
    invitee_authority_id: AuthorityId,
) -> Result<DeviceEnrollmentCeremonyStart, AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::device_enrollment(),
        None,
        SemanticOperationKind::StartDeviceEnrollment,
    );
    start_device_enrollment_ceremony_owned(
        app_core,
        nickname_suggestion,
        invitee_authority_id,
        &owner,
        None,
    )
    .await
}

#[aura_macros::semantic_owner(
    owner = "start_device_enrollment_ceremony_owned",
    wrapper = "start_device_enrollment_ceremony",
    terminal = "publish_success_with",
    postcondition = "device_enrollment_started",
    proof = crate::workflows::semantic_facts::DeviceEnrollmentStartedProof,
    authoritative_inputs = "runtime,authoritative_source",
    depends_on = "runtime_device_enrollment_started",
    child_ops = "",
    category = "move_owned"
)]
async fn start_device_enrollment_ceremony_owned(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_suggestion: String,
    invitee_authority_id: AuthorityId,
    owner: &SemanticWorkflowOwner,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<DeviceEnrollmentCeremonyStart, AuraError> {
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .cloned()
            .ok_or_else(|| AuraError::from(WorkflowError::RuntimeUnavailable))?
    };
    let start = match start_device_enrollment_from_runtime(
        runtime,
        nickname_suggestion,
        invitee_authority_id,
    )
    .await
    {
        Ok(Ok(start)) => start,
        Ok(Err(error)) => {
            return fail_start_device_enrollment(
                owner,
                ceremony_op("start device enrollment", error).to_string(),
            )
            .await;
        }
        Err(error) => {
            return fail_start_device_enrollment(
                owner,
                ceremony_op("start device enrollment", error).to_string(),
            )
            .await;
        }
    };
    owner
        .publish_success_with(issue_device_enrollment_started_proof(
            start.ceremony_id.clone(),
        ))
        .await?;
    let handle = CeremonyHandle::new(
        start.ceremony_id.clone(),
        crate::runtime_bridge::CeremonyKind::DeviceEnrollment,
    );
    let status_handle = handle.status_handle();
    Ok(DeviceEnrollmentCeremonyStart {
        ceremony_id: start.ceremony_id,
        enrollment_code: start.enrollment_code,
        pending_epoch: start.pending_epoch,
        device_id: start.device_id,
        handle,
        status_handle,
    })
}

pub async fn start_device_enrollment_ceremony_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_suggestion: String,
    invitee_authority_id: AuthorityId,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<DeviceEnrollmentCeremonyStart> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::device_enrollment(),
        instance_id,
        SemanticOperationKind::StartDeviceEnrollment,
    );
    let result = start_device_enrollment_ceremony_owned(
        app_core,
        nickname_suggestion,
        invitee_authority_id,
        &owner,
        None,
    )
    .await;
    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}
/// Start a device removal ("remove device") ceremony.
pub async fn start_device_removal_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    device_id: String,
) -> Result<CeremonyHandle, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .cloned()
            .ok_or_else(|| AuraError::from(WorkflowError::RuntimeUnavailable))?
    };
    start_device_removal_from_runtime(runtime, device_id)
        .await?
        .map(|ceremony_id| {
            CeremonyHandle::new(
                ceremony_id,
                crate::runtime_bridge::CeremonyKind::DeviceRemoval,
            )
        })
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

    pub fn for_kind(kind: crate::runtime_bridge::CeremonyKind, interval: Duration) -> Self {
        Self {
            interval,
            max_attempts: ceremony_monitor_attempts(kind, interval),
            rollback_on_failure: matches!(
                kind,
                crate::runtime_bridge::CeremonyKind::GuardianRotation
                    | crate::runtime_bridge::CeremonyKind::DeviceRotation
            ),
            refresh_settings_on_complete: true,
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
    handle: &CeremonyStatusHandle,
) -> Result<KeyRotationCeremonyStatus, AuraError> {
    let core = app_core.read().await;
    core.get_key_rotation_ceremony_status(handle.ceremony_id())
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
    handle: CeremonyHandle,
) -> Result<(), AuraError> {
    let core = app_core.read().await;
    core.cancel_key_rotation_ceremony(handle.ceremony_id())
        .await
        .map_err(|e| ceremony_op("cancel ceremony", e).into())
}

/// Cancel a key-rotation ceremony using a stored ceremony id.
pub async fn cancel_key_rotation_ceremony_by_id(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: CeremonyId,
) -> Result<(), AuraError> {
    let core = app_core.read().await;
    core.cancel_key_rotation_ceremony(&ceremony_id)
        .await
        .map_err(|e| ceremony_op("cancel ceremony", e).into())
}

/// Poll a key rotation ceremony until completion or failure using a policy.
///
/// This is a portable (frontend-agnostic) helper for driving ceremony progress UIs.
/// Callers provide an `on_update` hook to receive intermediate statuses.
pub async fn monitor_key_rotation_ceremony_with_policy<SleepFn, SleepFut>(
    app_core: &Arc<RwLock<AppCore>>,
    handle: &CeremonyStatusHandle,
    policy: CeremonyPollPolicy,
    mut on_update: impl FnMut(&KeyRotationCeremonyStatus),
    mut sleep_fn: SleepFn,
) -> Result<CeremonyLifecycle<KeyRotationCeremonyStatus>, AuraError>
where
    SleepFn: FnMut(Duration) -> SleepFut,
    SleepFut: Future<Output = ()>,
{
    // Bounded polling to avoid infinite loops; UIs can re-invoke if desired.
    let mut attempts = AttemptBudget::new(policy.max_attempts);
    while attempts.can_attempt() {
        let attempt = attempts
            .record_attempt()
            .map_err(AuraError::from)?
            .saturating_add(1);
        sleep_fn(policy.interval).await;

        let status = get_key_rotation_ceremony_status(app_core, handle).await?;
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
                                ceremony_id = %handle.ceremony_id(),
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
                        ceremony_id = %handle.ceremony_id(),
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
    let status = get_key_rotation_ceremony_status(app_core, handle).await?;
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
    handle: &CeremonyStatusHandle,
    poll_interval: Duration,
    mut on_update: impl FnMut(&KeyRotationCeremonyStatus),
    mut sleep_fn: SleepFn,
) -> Result<KeyRotationCeremonyStatus, AuraError>
where
    SleepFn: FnMut(Duration) -> SleepFut,
    SleepFut: Future<Output = ()>,
{
    let policy = CeremonyPollPolicy::for_kind(handle.kind(), poll_interval);
    let lifecycle = monitor_key_rotation_ceremony_with_policy(
        app_core,
        handle,
        policy,
        &mut on_update,
        &mut sleep_fn,
    )
    .await?;

    Ok(lifecycle.status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ceremony_monitor_policy_scales_by_kind() {
        let interval = Duration::from_millis(250);

        let enrollment = CeremonyPollPolicy::for_kind(
            crate::runtime_bridge::CeremonyKind::DeviceEnrollment,
            interval,
        );
        let recovery =
            CeremonyPollPolicy::for_kind(crate::runtime_bridge::CeremonyKind::Recovery, interval);

        assert_eq!(enrollment.max_attempts, 182);
        assert_eq!(recovery.max_attempts, 362);
        assert!(!enrollment.rollback_on_failure);
        assert!(!recovery.rollback_on_failure);
    }

    #[test]
    fn ceremony_start_timeout_is_kind_specific() {
        assert_eq!(
            ceremony_start_timeout(crate::runtime_bridge::CeremonyKind::DeviceEnrollment),
            Duration::from_millis(30_000)
        );
        assert_eq!(
            ceremony_start_timeout(crate::runtime_bridge::CeremonyKind::DeviceRemoval),
            Duration::from_millis(20_000)
        );
        assert_eq!(
            ceremony_monitor_timeout(crate::runtime_bridge::CeremonyKind::Recovery),
            Duration::from_secs(90)
        );
    }

    #[test]
    fn only_transient_intent_errors_retry_for_ceremony_start() {
        assert!(retryable_ceremony_intent_error(
            &IntentError::network_error("timeout")
        ));
        assert!(retryable_ceremony_intent_error(
            &IntentError::service_error("busy")
        ));
        assert!(!retryable_ceremony_intent_error(
            &IntentError::validation_failed("bad input")
        ));
    }
}
