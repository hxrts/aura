//! Settings Workflow - Portable Business Logic
//!
//! This module contains settings operations that are portable across all frontends.
//! It follows the reactive signal pattern and emits SETTINGS_SIGNAL updates.

use crate::workflows::runtime::require_runtime;
use crate::workflows::signals::{emit_signal, read_signal};
use crate::{
    signal_defs::{DeviceInfo, SettingsState, SETTINGS_SIGNAL, SETTINGS_SIGNAL_NAME},
    AppCore,
};
use async_lock::RwLock;
use aura_core::AuraError;
use std::sync::Arc;

async fn refresh_settings_signal_from_runtime(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let (settings, devices) = {
        let core = app_core.read().await;
        match core.settings_snapshot().await {
            Some(snapshot) => snapshot,
            None => return Ok(()),
        }
    };
    let mut state = read_signal(app_core, &*SETTINGS_SIGNAL, SETTINGS_SIGNAL_NAME).await?;
    state.display_name = settings.display_name;
    state.mfa_policy = settings.mfa_policy;
    state.threshold_k = settings.threshold_k as u8;
    state.threshold_n = settings.threshold_n as u8;
    state.contact_count = settings.contact_count;
    state.devices = devices
        .into_iter()
        .map(|d| DeviceInfo {
            id: d.id,
            name: d.name,
            is_current: d.is_current,
            last_seen: d.last_seen,
        })
        .collect();

    emit_signal(app_core, &*SETTINGS_SIGNAL, state, SETTINGS_SIGNAL_NAME).await
}

/// Refresh SETTINGS_SIGNAL from the current RuntimeBridge settings.
///
/// This is used at startup (to seed UI state) and after settings writes.
pub async fn refresh_settings_from_runtime(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    refresh_settings_signal_from_runtime(app_core).await
}

/// Update MFA policy
///
/// **What it does**: Updates MFA policy and emits SETTINGS_SIGNAL
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn update_mfa_policy(
    app_core: &Arc<RwLock<AppCore>>,
    require_mfa: bool,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    let policy = if require_mfa {
        "AlwaysRequired"
    } else {
        "SensitiveOnly"
    };

    runtime
        .set_mfa_policy(policy)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to update MFA policy: {e}")))?;

    refresh_settings_from_runtime(app_core).await?;
    Ok(())
}

/// Update nickname/display name
///
/// **What it does**: Updates display name and emits SETTINGS_SIGNAL
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn update_nickname(
    app_core: &Arc<RwLock<AppCore>>,
    name: String,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    runtime
        .set_display_name(&name)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to update display name: {e}")))?;

    refresh_settings_from_runtime(app_core).await?;
    Ok(())
}

/// Set channel mode flags
///
/// **What it does**: Sets channel-specific mode flags
/// **Returns**: Unit result
/// **Signal pattern**: Read-only operation (no emission)
///
/// This operation updates local channel preferences (e.g., notifications).
/// The UI layer handles persistence to local storage.
pub async fn set_channel_mode(
    _app_core: &Arc<RwLock<AppCore>>,
    _channel_id: String,
    _flags: String,
) -> Result<(), AuraError> {
    // Channel mode is local UI preference, not persisted via RuntimeBridge
    // The UI layer will handle local storage
    Ok(())
}

/// Get current settings state
///
/// **What it does**: Reads settings from SETTINGS_SIGNAL
/// **Returns**: Current settings state
/// **Signal pattern**: Read-only operation (no emission)
pub async fn get_settings(app_core: &Arc<RwLock<AppCore>>) -> Result<SettingsState, AuraError> {
    read_signal(app_core, &SETTINGS_SIGNAL, SETTINGS_SIGNAL_NAME).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[tokio::test]
    async fn test_get_settings_default() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        // Workflows assume reactive signals are initialized.
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let settings = get_settings(&app_core).await.unwrap();
        assert_eq!(settings.threshold_k, 0);
        assert_eq!(settings.threshold_n, 0);
    }

    #[tokio::test]
    async fn test_update_mfa_policy_without_runtime() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        // Without a runtime bridge, updating MFA policy should fail
        let result = update_mfa_policy(&app_core, true).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Runtime bridge not available"));
    }
}
