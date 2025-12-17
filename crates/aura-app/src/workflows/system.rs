//! System Workflow - Portable Business Logic
//!
//! This module contains system-level operations that are portable across all frontends.
//! These are mostly lightweight health-check and state refresh operations.

use crate::signal_defs::{
    ConnectionStatus, SettingsState, CONNECTION_STATUS_SIGNAL, SETTINGS_SIGNAL,
};
use crate::AppCore;
use async_lock::RwLock;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::AuraError;
use std::sync::Arc;

/// Ping operation for health check
///
/// **What it does**: Simple health check operation
/// **Returns**: Unit result
/// **Signal pattern**: Read-only operation (no emission)
///
/// This is a no-op that verifies the workflow layer is responsive.
pub async fn ping(_app_core: &Arc<RwLock<AppCore>>) -> Result<(), AuraError> {
    Ok(())
}

/// Refresh account state
///
/// **What it does**: Triggers state refresh across all signals
/// **Returns**: Unit result
/// **Signal pattern**: Re-emits all major signals
///
/// This operation triggers a state refresh by calling domain-specific
/// workflows that re-read and emit their respective signals.
pub async fn refresh_account(app_core: &Arc<RwLock<AppCore>>) -> Result<(), AuraError> {
    // Refresh chat state
    let _ = super::messaging::get_chat_state(app_core).await;

    // Refresh contacts state
    let _ = super::query::list_contacts(app_core).await;

    // Refresh invitations state
    let _ = super::invitation::list_invitations(app_core).await;

    // Refresh settings state
    let _ = super::settings::get_settings(app_core).await;

    // Refresh recovery state
    let _ = super::recovery::get_recovery_status(app_core).await;

    // Refresh discovered peers
    let _ = super::network::get_discovered_peers(app_core).await;

    // Refresh connection status from runtime
    let core = app_core.read().await;
    if let Some(runtime) = core.runtime() {
        let status = runtime.get_sync_status().await;
        let connection = if status.connected_peers > 0 {
            ConnectionStatus::Online {
                peer_count: status.connected_peers,
            }
        } else {
            ConnectionStatus::Offline
        };
        let _ = core.emit(&*CONNECTION_STATUS_SIGNAL, connection).await;

        // Refresh settings from runtime
        let settings = runtime.get_settings().await;
        let settings_state = SettingsState {
            display_name: settings.display_name,
            mfa_policy: settings.mfa_policy,
            threshold_k: settings.threshold_k as u8,
            threshold_n: settings.threshold_n as u8,
            devices: Vec::new(), // Devices list populated elsewhere
            contact_count: settings.contact_count,
        };
        let _ = core.emit(&*SETTINGS_SIGNAL, settings_state).await;
    }

    Ok(())
}

/// Check if app core is accessible
///
/// **What it does**: Verifies AppCore can be accessed
/// **Returns**: Boolean indicating accessibility
/// **Signal pattern**: Read-only operation (no emission)
pub async fn is_available(app_core: &Arc<RwLock<AppCore>>) -> bool {
    app_core.try_read().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[tokio::test]
    async fn test_ping() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let result = ping(&app_core).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_is_available() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let available = is_available(&app_core).await;
        assert!(available);
    }

    #[tokio::test]
    async fn test_refresh_account() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let result = refresh_account(&app_core).await;
        assert!(result.is_ok());
    }
}
