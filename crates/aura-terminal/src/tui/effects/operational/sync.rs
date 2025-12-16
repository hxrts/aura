//! Sync command handlers
//!
//! Handlers for ForceSync, RequestState.

use std::sync::Arc;

use aura_app::signal_defs::{SyncStatus, SYNC_STATUS_SIGNAL};
use aura_app::AppCore;
use aura_core::effects::reactive::ReactiveEffects;
use tokio::sync::RwLock;

use super::types::{OpError, OpResponse, OpResult};
use super::EffectCommand;

/// Handle sync commands
pub async fn handle_sync(command: &EffectCommand, app_core: &Arc<RwLock<AppCore>>) -> Option<OpResult> {
    match command {
        EffectCommand::ForceSync => {
            // Update sync status signal to show syncing
            if let Ok(core) = app_core.try_read() {
                let _ = core
                    .emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Syncing { progress: 0 })
                    .await;
            }

            // Trigger sync through effect injection (RuntimeBridge)
            let result = if let Ok(core) = app_core.try_read() {
                core.trigger_sync().await
            } else {
                Err(aura_app::core::IntentError::internal_error(
                    "AppCore unavailable",
                ))
            };

            // Update status based on result
            if let Ok(core) = app_core.try_read() {
                match &result {
                    Ok(()) => {
                        let _ = core.emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Synced).await;
                    }
                    Err(e) => {
                        tracing::warn!("Sync trigger failed: {}", e);
                        // In demo/offline mode, show as synced (local-only)
                        let _ = core.emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Synced).await;
                    }
                }
            }

            Some(Ok(OpResponse::Ok))
        }

        EffectCommand::RequestState { peer_id } => {
            // Request state from a specific peer - triggers targeted sync
            // Update sync status signal to show syncing
            if let Ok(core) = app_core.try_read() {
                let _ = core
                    .emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Syncing { progress: 0 })
                    .await;
            }

            // Trigger sync through AppCore (RuntimeBridge handles peer targeting)
            // For now, we trigger a general sync - peer-targeted sync requires
            // additional infrastructure in the sync engine
            let result = if let Ok(core) = app_core.try_read() {
                core.trigger_sync().await
            } else {
                Err(aura_app::core::IntentError::internal_error(
                    "AppCore unavailable",
                ))
            };

            // Update status based on result
            if let Ok(core) = app_core.try_read() {
                match &result {
                    Ok(_) => {
                        let _ = core.emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Synced).await;
                    }
                    Err(e) => {
                        let _ = core
                            .emit(
                                &*SYNC_STATUS_SIGNAL,
                                SyncStatus::Failed {
                                    message: e.to_string(),
                                },
                            )
                            .await;
                    }
                }
            }

            match result {
                Ok(_) => Some(Ok(OpResponse::Data(format!(
                    "Sync requested from peer: {}",
                    peer_id
                )))),
                Err(e) => Some(Err(OpError::Failed(format!(
                    "Failed to sync from peer {}: {}",
                    peer_id, e
                )))),
            }
        }

        _ => None,
    }
}
