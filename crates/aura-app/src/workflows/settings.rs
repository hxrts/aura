//! Settings Workflow - Portable Business Logic
//!
//! This module contains settings operations that are portable across all frontends.
//! It follows the reactive signal pattern and emits SETTINGS_SIGNAL updates.

use crate::{
    signal_defs::{SettingsState, SETTINGS_SIGNAL},
    AppCore,
};
use async_lock::RwLock;
use aura_core::{effects::reactive::ReactiveEffects, AuraError};
use std::sync::Arc;

/// Update MFA policy
///
/// **What it does**: Updates MFA policy and emits SETTINGS_SIGNAL
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn update_mfa_policy(
    app_core: &Arc<RwLock<AppCore>>,
    require_mfa: bool,
) -> Result<(), AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    let policy = if require_mfa {
        "AlwaysRequired"
    } else {
        "SensitiveOnly"
    };

    runtime
        .set_mfa_policy(policy)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to update MFA policy: {}", e)))?;

    {
        let core = app_core.read().await;
        if let Some(runtime) = core.runtime() {
            let settings = runtime.get_settings().await;
            let mut state = core.read(&*SETTINGS_SIGNAL).await.unwrap_or_default();
            state.display_name = settings.display_name;
            state.mfa_policy = settings.mfa_policy;
            state.threshold_k = settings.threshold_k as u8;
            state.threshold_n = settings.threshold_n as u8;
            state.contact_count = settings.contact_count;
            let _ = core.emit(&*SETTINGS_SIGNAL, state).await;
        }
    }

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
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    runtime
        .set_display_name(&name)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to update display name: {}", e)))?;

    {
        let core = app_core.read().await;
        if let Some(runtime) = core.runtime() {
            let settings = runtime.get_settings().await;
            let mut state = core.read(&*SETTINGS_SIGNAL).await.unwrap_or_default();
            state.display_name = settings.display_name;
            state.mfa_policy = settings.mfa_policy;
            state.threshold_k = settings.threshold_k as u8;
            state.threshold_n = settings.threshold_n as u8;
            state.contact_count = settings.contact_count;
            let _ = core.emit(&*SETTINGS_SIGNAL, state).await;
        }
    }

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

    core.read(&*SETTINGS_SIGNAL).await.unwrap_or_default()
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
