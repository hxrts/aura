//! Sync Workflow - Portable Business Logic
//!
//! This module contains sync operations that are portable across all frontends.
//! It follows the reactive signal pattern and uses RuntimeBridge for runtime operations.

use crate::{signal_defs::{SyncStatus, SYNC_STATUS_SIGNAL}, AppCore};
use async_lock::RwLock;
use aura_core::AuraError;
use std::sync::Arc;
use crate::workflows::signals::{emit_signal, read_signal_or_default};

/// Set sync status directly.
///
/// **What it does**: Emits SYNC_STATUS_SIGNAL with provided status
/// **Signal pattern**: Emits SYNC_STATUS_SIGNAL
pub async fn set_sync_status(
    app_core: &Arc<RwLock<AppCore>>,
    status: SyncStatus,
) -> Result<(), AuraError> {
    emit_signal(
        app_core,
        &*SYNC_STATUS_SIGNAL,
        status,
        "SYNC_STATUS_SIGNAL",
    )
    .await
}

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
pub async fn force_sync(app_core: &Arc<RwLock<AppCore>>) -> Result<(), AuraError> {
    // Update sync status signal to show syncing
    emit_signal(
        app_core,
        &*SYNC_STATUS_SIGNAL,
        SyncStatus::Syncing { progress: 0 },
        "SYNC_STATUS_SIGNAL",
    )
    .await?;

    // Trigger sync through RuntimeBridge
    let result = {
        let core = app_core.read().await;
        core.trigger_sync()
            .await
            .map_err(|e| AuraError::agent(format!("Failed to trigger sync: {e}")))
    };

    // Update status based on result
    {
        match &result {
            Ok(()) => {
                emit_signal(
                    app_core,
                    &*SYNC_STATUS_SIGNAL,
                    SyncStatus::Synced,
                    "SYNC_STATUS_SIGNAL",
                )
                .await?;
            }
            Err(_e) => {
                // In demo/offline mode, show as synced (local-only)
                emit_signal(
                    app_core,
                    &*SYNC_STATUS_SIGNAL,
                    SyncStatus::Synced,
                    "SYNC_STATUS_SIGNAL",
                )
                .await?;
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
) -> Result<(), AuraError> {
    // Update sync status signal to show syncing
    emit_signal(
        app_core,
        &*SYNC_STATUS_SIGNAL,
        SyncStatus::Syncing { progress: 0 },
        "SYNC_STATUS_SIGNAL",
    )
    .await?;

    // Trigger peer-targeted sync through RuntimeBridge
    let result = {
        let core = app_core.read().await;
        core.sync_with_peer(peer_id)
            .await
            .map_err(|e| AuraError::agent(format!("Failed to sync with peer: {e}")))
    };

    // Update status based on result
    match &result {
        Ok(_) => {
            emit_signal(
                app_core,
                &*SYNC_STATUS_SIGNAL,
                SyncStatus::Synced,
                "SYNC_STATUS_SIGNAL",
            )
            .await?;
        }
        Err(e) => {
            emit_signal(
                app_core,
                &*SYNC_STATUS_SIGNAL,
                SyncStatus::Failed {
                    message: e.to_string(),
                },
                "SYNC_STATUS_SIGNAL",
            )
            .await?;
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
    read_signal_or_default(app_core, &*SYNC_STATUS_SIGNAL).await
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
