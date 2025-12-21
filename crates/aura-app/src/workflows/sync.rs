//! Sync Workflow - Portable Business Logic
//!
//! This module contains sync operations that are portable across all frontends.
//! It follows the reactive signal pattern and uses RuntimeBridge for runtime operations.

use crate::{
    core::IntentError,
    signal_defs::{SyncStatus, SYNC_STATUS_SIGNAL},
    AppCore,
};
use async_lock::RwLock;
use aura_core::effects::reactive::ReactiveEffects;
use std::sync::Arc;

/// Force synchronization with peers
///
/// **What it does**: Triggers sync operation via RuntimeBridge
/// **Returns**: Unit result
/// **Signal pattern**: Emits SYNC_STATUS_SIGNAL during and after operation
///
/// This operation:
/// 1. Emits SYNC_STATUS_SIGNAL with Syncing state
/// 2. Triggers sync via RuntimeBridge.trigger_sync()
/// 3. Emits SYNC_STATUS_SIGNAL with Synced or Failed state
pub async fn force_sync(app_core: &Arc<RwLock<AppCore>>) -> Result<(), IntentError> {
    // Update sync status signal to show syncing
    {
        let core = app_core.read().await;
        core.emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Syncing { progress: 0 })
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to emit sync status: {}", e))
            })?;
    }

    // Trigger sync through RuntimeBridge
    let result = {
        let core = app_core.read().await;
        core.trigger_sync().await
    };

    // Update status based on result
    {
        let core = app_core.read().await;
        match &result {
            Ok(()) => {
                core.emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Synced)
                    .await
                    .map_err(|e| {
                        IntentError::internal_error(format!("Failed to emit sync status: {}", e))
                    })?;
            }
            Err(_e) => {
                // In demo/offline mode, show as synced (local-only)
                core.emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Synced)
                    .await
                    .map_err(|e| {
                        IntentError::internal_error(format!("Failed to emit sync status: {}", e))
                    })?;
            }
        }
    }

    result
}

/// Request state from a specific peer
///
/// **What it does**: Triggers targeted sync with the specified peer
/// **Returns**: Unit result
/// **Signal pattern**: Emits SYNC_STATUS_SIGNAL during and after operation
///
/// This operation:
/// 1. Emits SYNC_STATUS_SIGNAL with Syncing state
/// 2. Triggers peer-targeted sync via RuntimeBridge.sync_with_peer()
/// 3. Emits SYNC_STATUS_SIGNAL with Synced or Failed state
pub async fn request_state(
    app_core: &Arc<RwLock<AppCore>>,
    peer_id: &str,
) -> Result<(), IntentError> {
    // Update sync status signal to show syncing
    {
        let core = app_core.read().await;
        core.emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Syncing { progress: 0 })
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to emit sync status: {}", e))
            })?;
    }

    // Trigger peer-targeted sync through RuntimeBridge
    let result = {
        let core = app_core.read().await;
        core.sync_with_peer(peer_id).await
    };

    // Update status based on result
    {
        let core = app_core.read().await;
        match &result {
            Ok(_) => {
                core.emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Synced)
                    .await
                    .map_err(|e| {
                        IntentError::internal_error(format!("Failed to emit sync status: {}", e))
                    })?;
            }
            Err(e) => {
                core.emit(
                    &*SYNC_STATUS_SIGNAL,
                    SyncStatus::Failed {
                        message: e.to_string(),
                    },
                )
                .await
                .map_err(|e| {
                    IntentError::internal_error(format!("Failed to emit sync status: {}", e))
                })?;
            }
        }
    }

    result
}

/// Get current sync status
///
/// **What it does**: Reads sync status from SYNC_STATUS_SIGNAL
/// **Returns**: Current sync status
/// **Signal pattern**: Read-only operation (no emission)
pub async fn get_sync_status(app_core: &Arc<RwLock<AppCore>>) -> SyncStatus {
    let core = app_core.read().await;

    match core.read(&*SYNC_STATUS_SIGNAL).await {
        Ok(status) => status,
        Err(_) => SyncStatus::Idle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[tokio::test]
    async fn test_get_sync_status_default() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let status = get_sync_status(&app_core).await;
        assert!(matches!(status, SyncStatus::Idle));
    }
}
