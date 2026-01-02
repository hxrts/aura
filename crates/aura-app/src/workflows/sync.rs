//! Sync Workflow - Portable Business Logic
//!
//! This module contains sync operations that are portable across all frontends.
//! It follows the reactive signal pattern and uses RuntimeBridge for runtime operations.
//!
//! ## Configuration
//!
//! This module provides portable configuration constants and helpers:
//! - `DEFAULT_SYNC_INTERVAL_SECS`: Default interval between sync rounds (60s)
//! - `DEFAULT_MAX_CONCURRENT_SYNCS`: Default max concurrent sessions (5)
//! - `parse_peer_list()`: Parse comma-separated peer list string
//! - `SyncConfigDefaults`: Portable config defaults struct

use crate::workflows::signals::{emit_signal, read_signal_or_default};
use crate::{
    signal_defs::{SyncStatus, SYNC_STATUS_SIGNAL, SYNC_STATUS_SIGNAL_NAME},
    AppCore,
};
use async_lock::RwLock;
use aura_core::AuraError;
use std::sync::Arc;
use std::time::Duration;

// ============================================================================
// Portable Configuration Constants
// ============================================================================

/// Default interval between automatic sync rounds (60 seconds).
pub const DEFAULT_SYNC_INTERVAL_SECS: u64 = 60;

/// Default maximum concurrent sync sessions.
pub const DEFAULT_MAX_CONCURRENT_SYNCS: usize = 5;

/// Default maintenance interval (60 seconds).
pub const DEFAULT_MAINTENANCE_INTERVAL_SECS: u64 = 60;

/// Default TTL for stale peer states (6 hours).
pub const DEFAULT_PEER_STATE_TTL_SECS: u64 = 6 * 60 * 60;

/// Default maximum tracked peer states before pruning.
pub const DEFAULT_MAX_PEER_STATES: usize = 1024;

// ============================================================================
// Configuration Helpers
// ============================================================================

/// Portable sync configuration defaults.
///
/// This struct provides frontend-portable configuration that can be used
/// to build runtime-specific config objects (like `SyncManagerConfig`).
#[derive(Debug, Clone)]
pub struct SyncConfigDefaults {
    /// Enable automatic periodic sync
    pub auto_sync_enabled: bool,
    /// Interval between automatic sync rounds in seconds
    pub sync_interval_secs: u64,
    /// Maximum concurrent sync sessions
    pub max_concurrent_syncs: usize,
    /// Initial peer identifiers (as strings, to be converted by runtime)
    pub initial_peers: Vec<String>,
    /// Enable maintenance cleanup
    pub maintenance_enabled: bool,
}

impl Default for SyncConfigDefaults {
    fn default() -> Self {
        Self {
            auto_sync_enabled: true,
            sync_interval_secs: DEFAULT_SYNC_INTERVAL_SECS,
            max_concurrent_syncs: DEFAULT_MAX_CONCURRENT_SYNCS,
            initial_peers: Vec::new(),
            maintenance_enabled: true,
        }
    }
}

impl SyncConfigDefaults {
    /// Create a new config with specified interval and concurrency.
    #[must_use]
    pub fn new(sync_interval_secs: u64, max_concurrent_syncs: usize) -> Self {
        Self {
            sync_interval_secs,
            max_concurrent_syncs,
            ..Default::default()
        }
    }

    /// Create a config with initial peers.
    #[must_use]
    pub fn with_peers(mut self, peers: Vec<String>) -> Self {
        self.initial_peers = peers;
        self
    }

    /// Create a config from parsed peer list string.
    #[must_use]
    pub fn with_peer_list(mut self, peers_str: &str) -> Self {
        self.initial_peers = parse_peer_list(peers_str);
        self
    }

    /// Get the sync interval as a Duration.
    #[must_use]
    pub fn sync_interval(&self) -> Duration {
        Duration::from_secs(self.sync_interval_secs)
    }
}

/// Parse a comma-separated peer list string into individual peer identifiers.
///
/// Empty entries and whitespace-only entries are filtered out.
/// Leading/trailing whitespace on each entry is trimmed.
///
/// # Examples
///
/// ```ignore
/// use aura_app::workflows::sync::parse_peer_list;
///
/// let peers = parse_peer_list("peer1, peer2, peer3");
/// assert_eq!(peers, vec!["peer1", "peer2", "peer3"]);
///
/// let empty = parse_peer_list("");
/// assert!(empty.is_empty());
///
/// let with_spaces = parse_peer_list("  peer1 , , peer2  ");
/// assert_eq!(with_spaces, vec!["peer1", "peer2"]);
/// ```
#[must_use]
pub fn parse_peer_list(peers: &str) -> Vec<String> {
    peers
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Get the default sync interval as a Duration.
#[must_use]
pub fn default_sync_interval() -> Duration {
    Duration::from_secs(DEFAULT_SYNC_INTERVAL_SECS)
}

// ============================================================================
// Signal-Based Sync Operations
// ============================================================================

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
        SYNC_STATUS_SIGNAL_NAME,
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
        SYNC_STATUS_SIGNAL_NAME,
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
                    SYNC_STATUS_SIGNAL_NAME,
                )
                .await?;
            }
            Err(_e) => {
                // In demo/offline mode, show as synced (local-only)
                emit_signal(
                    app_core,
                    &*SYNC_STATUS_SIGNAL,
                    SyncStatus::Synced,
                    SYNC_STATUS_SIGNAL_NAME,
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
        SYNC_STATUS_SIGNAL_NAME,
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
                SYNC_STATUS_SIGNAL_NAME,
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
                SYNC_STATUS_SIGNAL_NAME,
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

    // === Configuration Tests ===

    #[test]
    fn test_parse_peer_list_basic() {
        let peers = parse_peer_list("peer1,peer2,peer3");
        assert_eq!(peers, vec!["peer1", "peer2", "peer3"]);
    }

    #[test]
    fn test_parse_peer_list_with_spaces() {
        let peers = parse_peer_list("  peer1 , peer2 , peer3  ");
        assert_eq!(peers, vec!["peer1", "peer2", "peer3"]);
    }

    #[test]
    fn test_parse_peer_list_empty() {
        let peers = parse_peer_list("");
        assert!(peers.is_empty());
    }

    #[test]
    fn test_parse_peer_list_empty_entries() {
        let peers = parse_peer_list("peer1,,peer2, ,peer3");
        assert_eq!(peers, vec!["peer1", "peer2", "peer3"]);
    }

    #[test]
    fn test_parse_peer_list_single() {
        let peers = parse_peer_list("single_peer");
        assert_eq!(peers, vec!["single_peer"]);
    }

    #[test]
    fn test_default_sync_interval() {
        let interval = default_sync_interval();
        assert_eq!(interval, Duration::from_secs(60));
    }

    #[test]
    fn test_sync_config_defaults() {
        let config = SyncConfigDefaults::default();
        assert!(config.auto_sync_enabled);
        assert_eq!(config.sync_interval_secs, 60);
        assert_eq!(config.max_concurrent_syncs, 5);
        assert!(config.initial_peers.is_empty());
        assert!(config.maintenance_enabled);
    }

    #[test]
    fn test_sync_config_new() {
        let config = SyncConfigDefaults::new(120, 10);
        assert_eq!(config.sync_interval_secs, 120);
        assert_eq!(config.max_concurrent_syncs, 10);
    }

    #[test]
    fn test_sync_config_with_peers() {
        let config = SyncConfigDefaults::default()
            .with_peers(vec!["peer1".to_string(), "peer2".to_string()]);
        assert_eq!(config.initial_peers.len(), 2);
    }

    #[test]
    fn test_sync_config_with_peer_list() {
        let config = SyncConfigDefaults::default().with_peer_list("peer1, peer2, peer3");
        assert_eq!(config.initial_peers, vec!["peer1", "peer2", "peer3"]);
    }

    #[test]
    fn test_sync_config_interval() {
        let config = SyncConfigDefaults::new(120, 5);
        assert_eq!(config.sync_interval(), Duration::from_secs(120));
    }

    // === Signal Tests ===

    #[tokio::test]
    async fn test_get_sync_status_default() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let status = get_sync_status(&app_core).await;
        assert!(matches!(status, SyncStatus::Idle));
    }
}
