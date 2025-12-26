//! Ceremony workflows (portable)
//!
//! Provides portable helpers for starting/polling/canceling Category C ceremonies.

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
        .map_err(|e| AuraError::agent(format!("Failed to start guardian ceremony: {}", e)))
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
        .map_err(|e| {
            AuraError::agent(format!("Failed to start device threshold ceremony: {}", e))
        })
}

/// Start a device enrollment ("add device") ceremony.
pub async fn start_device_enrollment_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    device_name: String,
) -> Result<crate::runtime_bridge::DeviceEnrollmentStart, AuraError> {
    let core = app_core.read().await;
    core.initiate_device_enrollment_ceremony(device_name)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to start device enrollment: {}", e)))
}
/// Start a device removal ("remove device") ceremony.
pub async fn start_device_removal_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    device_id: String,
) -> Result<String, AuraError> {
    let core = app_core.read().await;
    core.initiate_device_removal_ceremony(device_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to start device removal: {}", e)))
}

/// Get status of a key rotation ceremony (generic form).
pub async fn get_key_rotation_ceremony_status(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: &str,
) -> Result<KeyRotationCeremonyStatus, AuraError> {
    let core = app_core.read().await;
    core.get_key_rotation_ceremony_status(ceremony_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to get ceremony status: {}", e)))
}

/// Cancel a key rotation ceremony (best effort).
pub async fn cancel_key_rotation_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: &str,
) -> Result<(), AuraError> {
    let core = app_core.read().await;
    core.cancel_key_rotation_ceremony(ceremony_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to cancel ceremony: {}", e)))
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
    // Bounded polling to avoid infinite loops; UIs can re-invoke if desired.
    for _ in 0..60 {
        sleep_fn(poll_interval).await;

        let status = get_key_rotation_ceremony_status(app_core, &ceremony_id).await?;
        on_update(&status);

        if status.has_failed {
            // Best-effort rollback for rotations (until runtime owns this fully).
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
            return Ok(status);
        }

        if status.is_complete {
            // Best-effort: refresh settings so device list / counts update after a commit.
            let _ = crate::workflows::settings::refresh_settings_from_runtime(app_core).await;
            return Ok(status);
        }
    }

    // Timed out; return the latest status we can fetch.
    get_key_rotation_ceremony_status(app_core, &ceremony_id).await
}
