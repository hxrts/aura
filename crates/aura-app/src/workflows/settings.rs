//! Settings Workflow - Portable Business Logic
//!
//! This module contains settings operations that are portable across all frontends.
//! It follows the reactive signal pattern and emits SETTINGS_SIGNAL updates.

use crate::{
    signal_defs::{DeviceInfo, SettingsState, SETTINGS_SIGNAL},
    AppCore,
};
use aura_core::{effects::reactive::ReactiveEffects, AuraError};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Update MFA policy
///
/// **What it does**: Updates MFA policy and emits SETTINGS_SIGNAL
/// **Returns**: Unit result
/// **Signal pattern**: Emits SETTINGS_SIGNAL after update
///
/// **TODO**: Add RuntimeBridge method to persist MFA policy.
/// Currently emits signal for UI updates only.
pub async fn update_mfa_policy(
    app_core: &Arc<RwLock<AppCore>>,
    _require_mfa: bool,
) -> Result<(), AuraError> {
    // TODO: Persist via RuntimeBridge
    // For now, just emit signal for UI update
    emit_settings_signal(app_core).await?;

    Ok(())
}

/// Update nickname/display name
///
/// **What it does**: Updates display name and emits SETTINGS_SIGNAL
/// **Returns**: Unit result
/// **Signal pattern**: Emits SETTINGS_SIGNAL after update
///
/// **TODO**: Add RuntimeBridge method to persist display name.
/// Currently emits signal for UI updates only.
pub async fn update_nickname(
    app_core: &Arc<RwLock<AppCore>>,
    _name: String,
) -> Result<(), AuraError> {
    // TODO: Persist via RuntimeBridge
    // For now, just emit signal for UI update
    emit_settings_signal(app_core).await?;

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
pub async fn get_settings(app_core: &Arc<RwLock<AppCore>>) -> SettingsState {
    let core = app_core.read().await;

    match core.read(&*SETTINGS_SIGNAL).await {
        Ok(state) => state,
        Err(_) => SettingsState::default(),
    }
}

/// Emit settings signal with current state
///
/// **What it does**: Queries settings from runtime and emits SETTINGS_SIGNAL
/// **Returns**: Unit result
/// **Signal pattern**: Emits SETTINGS_SIGNAL
///
/// **TODO**: Query actual settings from RuntimeBridge.
/// Currently uses placeholder values.
async fn emit_settings_signal(app_core: &Arc<RwLock<AppCore>>) -> Result<(), AuraError> {
    let core = app_core.read().await;

    // Get current settings from runtime
    // For now, we use placeholder values - these would be queried from runtime in production
    let display_name = String::new(); // TODO: Query from runtime
    let threshold_k = 0; // TODO: Query from runtime
    let threshold_n = 0; // TODO: Query from runtime
    let mfa_policy = "SensitiveOnly".to_string(); // TODO: Query from runtime
    let devices: Vec<DeviceInfo> = Vec::new(); // TODO: Query device list from runtime
    let contact_count = 0; // TODO: Query contact count from runtime

    let state = SettingsState {
        display_name,
        threshold_k,
        threshold_n,
        mfa_policy,
        devices,
        contact_count,
    };

    // Emit the signal
    core.emit(&*SETTINGS_SIGNAL, state)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to emit settings signal: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[tokio::test]
    async fn test_get_settings_default() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let settings = get_settings(&app_core).await;
        assert_eq!(settings.threshold_k, 0);
        assert_eq!(settings.threshold_n, 0);
    }

    #[tokio::test]
    async fn test_update_mfa_policy() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let result = update_mfa_policy(&app_core, true).await;
        assert!(result.is_ok());
    }
}
