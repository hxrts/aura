//! Ceremony workflows (portable)
//!
//! Provides portable helpers for starting/polling/canceling Category C ceremonies.

#![allow(missing_docs)] // Ceremony workflow types are self-documenting

use std::sync::Arc;

use async_lock::RwLock;

use crate::runtime_bridge::KeyRotationCeremonyStatus;
use crate::AppCore;
use aura_core::types::FrostThreshold;
use aura_core::AuraError;
use std::future::Future;
use std::time::Duration;

/// Start a guardian key-rotation ceremony.
pub async fn start_guardian_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    threshold_k: FrostThreshold,
    total_n: u16,
    guardian_ids: Vec<String>,
) -> Result<String, AuraError> {
    let core = app_core.read().await;
    core.initiate_guardian_ceremony(threshold_k, total_n, &guardian_ids)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to start guardian ceremony: {e}")))
}

/// Start a device threshold (multifactor) ceremony.
pub async fn start_device_threshold_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    threshold_k: FrostThreshold,
    total_n: u16,
    device_ids: Vec<String>,
) -> Result<String, AuraError> {
    let core = app_core.read().await;
    core.initiate_device_threshold_ceremony(threshold_k, total_n, &device_ids)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to start device threshold ceremony: {e}")))
}

/// Start a device enrollment ("add device") ceremony.
pub async fn start_device_enrollment_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    device_name: String,
) -> Result<crate::runtime_bridge::DeviceEnrollmentStart, AuraError> {
    let core = app_core.read().await;
    core.initiate_device_enrollment_ceremony(device_name)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to start device enrollment: {e}")))
}
/// Start a device removal ("remove device") ceremony.
pub async fn start_device_removal_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    device_id: String,
) -> Result<String, AuraError> {
    let core = app_core.read().await;
    core.initiate_device_removal_ceremony(device_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to start device removal: {e}")))
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
    ceremony_id: &str,
) -> Result<KeyRotationCeremonyStatus, AuraError> {
    let core = app_core.read().await;
    core.get_key_rotation_ceremony_status(ceremony_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to get ceremony status: {e}")))
}

/// Cancel a key rotation ceremony (best effort).
pub async fn cancel_key_rotation_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: &str,
) -> Result<(), AuraError> {
    let core = app_core.read().await;
    core.cancel_key_rotation_ceremony(ceremony_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to cancel ceremony: {e}")))
}

/// Poll a key rotation ceremony until completion or failure using a policy.
///
/// This is a portable (frontend-agnostic) helper for driving ceremony progress UIs.
/// Callers provide an `on_update` hook to receive intermediate statuses.
pub async fn monitor_key_rotation_ceremony_with_policy<SleepFn, SleepFut>(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: String,
    policy: CeremonyPollPolicy,
    mut on_update: impl FnMut(&KeyRotationCeremonyStatus) + Send,
    mut sleep_fn: SleepFn,
) -> Result<CeremonyLifecycle<KeyRotationCeremonyStatus>, AuraError>
where
    SleepFn: FnMut(Duration) -> SleepFut + Send,
    SleepFut: Future<Output = ()> + Send,
{
    // Bounded polling to avoid infinite loops; UIs can re-invoke if desired.
    for attempt in 1..=policy.max_attempts {
        sleep_fn(policy.interval).await;

        let status = get_key_rotation_ceremony_status(app_core, &ceremony_id).await?;
        on_update(&status);

        if status.has_failed {
            // Best-effort rollback for rotations (until runtime owns this fully).
            if policy.rollback_on_failure {
                if let Some(epoch) = status.pending_epoch {
                    if matches!(
                        status.kind,
                        crate::runtime_bridge::CeremonyKind::GuardianRotation
                            | crate::runtime_bridge::CeremonyKind::DeviceRotation
                    ) {
                        let core = app_core.read().await;
                        if let Err(e) = core.rollback_guardian_key_rotation(epoch).await {
                            let _ = e;
                        }
                    }
                }
            }
            return Ok(CeremonyLifecycle {
                state: CeremonyLifecycleState::Failed,
                status,
                attempts: attempt,
            });
        }

        if status.is_complete {
            // Best-effort: refresh settings so device list / counts update after a commit.
            if policy.refresh_settings_on_complete {
                let _ = crate::workflows::settings::refresh_settings_from_runtime(app_core).await;
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
    ceremony_id: String,
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
