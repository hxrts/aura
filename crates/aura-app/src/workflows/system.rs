//! System Workflow - Portable Business Logic
//!
//! This module contains system-level operations that are portable across all frontends.
//! These are mostly lightweight health-check and state refresh operations.
//!
//! ## OTA Upgrade Parsing
//!
//! This module provides portable parsing functions for OTA upgrades:
//! - `UpgradeKindValue`: Portable enum for upgrade types (soft/hard)
//! - `parse_upgrade_kind()`: Parse string to upgrade kind
//! - `parse_semantic_version()`: Parse "x.y.z" version strings

use crate::runtime_bridge::SyncStatus as RuntimeSyncStatus;
use crate::signal_defs::{
    ConnectionStatus, NetworkStatus, CONNECTION_STATUS_SIGNAL, CONNECTION_STATUS_SIGNAL_NAME,
    CONTACTS_SIGNAL, CONTACTS_SIGNAL_NAME, NETWORK_STATUS_SIGNAL, NETWORK_STATUS_SIGNAL_NAME,
    TRANSPORT_PEERS_SIGNAL, TRANSPORT_PEERS_SIGNAL_NAME,
};
use crate::workflows::signals::{emit_signal, read_signal};
use crate::workflows::snapshot_policy::contacts_snapshot;
use crate::AppCore;
use async_lock::RwLock;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::AuraError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// ============================================================================
// OTA Upgrade Parsing Types and Functions
// ============================================================================

/// Portable upgrade kind value.
///
/// This is a frontend-portable representation of upgrade types that can be
/// converted to runtime-specific types (like `aura_sync::UpgradeKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeKindValue {
    /// Soft fork (backward compatible upgrade)
    Soft,
    /// Hard fork (requires coordinated activation)
    Hard,
}

impl UpgradeKindValue {
    /// Get the canonical string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Hard => "hard",
        }
    }
}

impl std::fmt::Display for UpgradeKindValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Parse an upgrade kind string into a portable value.
///
/// Accepts "soft" or "hard" (case-insensitive).
///
/// # Examples
///
/// ```ignore
/// use aura_app::workflows::system::parse_upgrade_kind;
///
/// assert!(parse_upgrade_kind("soft").is_ok());
/// assert!(parse_upgrade_kind("HARD").is_ok());
/// assert!(parse_upgrade_kind("invalid").is_err());
/// ```
pub fn parse_upgrade_kind(s: &str) -> Result<UpgradeKindValue, AuraError> {
    match s.to_lowercase().as_str() {
        "soft" => Ok(UpgradeKindValue::Soft),
        "hard" => Ok(UpgradeKindValue::Hard),
        _ => Err(AuraError::invalid(format!(
            "Invalid upgrade kind: '{s}'. Use 'soft' or 'hard'"
        ))),
    }
}

/// Parse a semantic version string into (major, minor, patch) components.
///
/// Expects format "major.minor.patch" where each component is a u16.
///
/// # Examples
///
/// ```ignore
/// use aura_app::workflows::system::parse_semantic_version;
///
/// let (major, minor, patch) = parse_semantic_version("1.2.3")?;
/// assert_eq!((major, minor, patch), (1, 2, 3));
///
/// assert!(parse_semantic_version("1.2").is_err()); // Missing patch
/// assert!(parse_semantic_version("1.2.3.4").is_err()); // Too many parts
/// ```
pub fn parse_semantic_version(s: &str) -> Result<(u16, u16, u16), AuraError> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return Err(AuraError::invalid(
            "Invalid semantic version format. Expected: major.minor.patch",
        ));
    }

    let major: u16 = parts[0]
        .parse()
        .map_err(|e| AuraError::invalid(format!("Invalid major version '{}': {}", parts[0], e)))?;
    let minor: u16 = parts[1]
        .parse()
        .map_err(|e| AuraError::invalid(format!("Invalid minor version '{}': {}", parts[1], e)))?;
    let patch: u16 = parts[2]
        .parse()
        .map_err(|e| AuraError::invalid(format!("Invalid patch version '{}': {}", parts[2], e)))?;

    Ok((major, minor, patch))
}

/// Validate a semantic version string without parsing.
///
/// Returns the input string if valid, or an error if invalid.
pub fn validate_version_string(s: &str) -> Result<&str, AuraError> {
    // Parse to validate, then return original string
    parse_semantic_version(s)?;
    Ok(s)
}

// ============================================================================
// System Operations
// ============================================================================

/// Compute the unified network status from transport and sync state.
///
/// Precedence:
/// 1. No runtime → Disconnected
/// 2. No online contacts → NoPeers (can't sync with no one)
/// 3. Active sync sessions → Syncing
/// 4. Has last_sync_ms → Synced
/// 5. Fallback → Syncing (have peers but no sync yet)
fn compute_network_status(
    has_runtime: bool,
    online_contacts: usize,
    sync_status: &RuntimeSyncStatus,
) -> NetworkStatus {
    if !has_runtime {
        return NetworkStatus::Disconnected;
    }

    // No contacts online = no one to sync with. Note: transport_peers may be > 0
    // (raw network connections), but without contacts we can't sync meaningfully.
    if online_contacts == 0 {
        return NetworkStatus::NoPeers;
    }

    // Currently syncing if there are active sync sessions
    if sync_status.active_sessions > 0 {
        return NetworkStatus::Syncing;
    }

    // Synced if we have a last sync timestamp
    if let Some(last_sync_ms) = sync_status.last_sync_ms {
        return NetworkStatus::Synced { last_sync_ms };
    }

    // Have peers but never synced yet - show as syncing (conservative)
    NetworkStatus::Syncing
}

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
    // Refresh chat state (signals feature only)
    #[cfg(feature = "signals")]
    {
        let _ = super::messaging::get_chat_state(app_core).await;
    }

    // Refresh contacts state
    let _ = super::query::list_contacts(app_core).await;

    // Refresh invitations state
    let _ = super::invitation::list_invitations(app_core).await;

    // Refresh settings state
    let _ = super::settings::refresh_settings_from_runtime(app_core).await;

    // Refresh recovery state (signals feature only)
    #[cfg(feature = "signals")]
    {
        let _ = super::recovery::get_recovery_status(app_core).await;
    }

    // Refresh discovered peers
    let _ = super::network::get_discovered_peers(app_core).await;

    // Refresh connection and network status derived from contacts.
    let _ = refresh_connection_status_from_contacts(app_core).await;

    Ok(())
}

/// Refresh connection + network status derived from CONTACTS_SIGNAL.
pub async fn refresh_connection_status_from_contacts(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    // Refresh connection status + settings from runtime.
    //
    // ConnectionStatus is intended to represent "how many of my contacts are online",
    // not merely "how many peers are configured".
    let runtime = {
        let core = app_core.read().await;
        core.runtime().cloned()
    };
    let mut contacts_state = contacts_snapshot(app_core).await;
    if let Ok(state) = read_signal(app_core, &*CONTACTS_SIGNAL, CONTACTS_SIGNAL_NAME).await {
        contacts_state = state;
    }

    if let Some(runtime) = runtime {
        // Collect contact IDs for iteration (since we need to borrow mutably later)
        let contact_ids: Vec<_> = contacts_state.all_contacts().map(|c| c.id).collect();

        let mut online_contacts = 0usize;
        for contact_id in &contact_ids {
            let is_online = runtime.is_peer_online(*contact_id).await;
            if let Some(contact) = contacts_state.contact_mut(contact_id) {
                contact.is_online = is_online;
                if is_online {
                    online_contacts += 1;
                }
            }
        }

        // Get sync status and compute unified network status
        let sync_status = {
            let core = app_core.read().await;
            core.sync_status().await.unwrap_or_default()
        };
        if online_contacts == 0
            && !contacts_state.is_empty()
            && sync_status.connected_peers > 0
        {
            let fallback_online =
                std::cmp::min(contacts_state.contact_count(), sync_status.connected_peers);
            for (idx, contact_id) in contact_ids.iter().enumerate() {
                if let Some(contact) = contacts_state.contact_mut(contact_id) {
                    contact.is_online = idx < fallback_online;
                }
            }
            online_contacts = fallback_online;
        }

        let connection = if online_contacts > 0 {
            ConnectionStatus::Online {
                peer_count: online_contacts,
            }
        } else {
            ConnectionStatus::Offline
        };
        let network_status = compute_network_status(true, online_contacts, &sync_status);

        let _ = emit_signal(
            app_core,
            &*CONTACTS_SIGNAL,
            contacts_state,
            CONTACTS_SIGNAL_NAME,
        )
        .await;
        let _ = emit_signal(
            app_core,
            &*CONNECTION_STATUS_SIGNAL,
            connection,
            CONNECTION_STATUS_SIGNAL_NAME,
        )
        .await;
        let _ = emit_signal(
            app_core,
            &*NETWORK_STATUS_SIGNAL,
            network_status,
            NETWORK_STATUS_SIGNAL_NAME,
        )
        .await;
        let _ = emit_signal(
            app_core,
            &*TRANSPORT_PEERS_SIGNAL,
            sync_status.connected_peers,
            TRANSPORT_PEERS_SIGNAL_NAME,
        )
        .await;
    } else {
        // No runtime - emit disconnected status
        let _ = emit_signal(
            app_core,
            &*NETWORK_STATUS_SIGNAL,
            NetworkStatus::Disconnected,
            NETWORK_STATUS_SIGNAL_NAME,
        )
        .await;
        let _ = emit_signal(
            app_core,
            &*TRANSPORT_PEERS_SIGNAL,
            0usize,
            TRANSPORT_PEERS_SIGNAL_NAME,
        )
        .await;
    }

    Ok(())
}

/// Install a background hook that refreshes derived account state when contacts change.
///
/// Frontends should not manage the "contacts → refresh account" coupling themselves.
/// This hook centralizes the derivation so UI layers can stay thin.
pub async fn install_contacts_refresh_hook(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let (reactive, spawner, should_install) = {
        let mut core = app_core.write().await;
        let should_install = core.mark_contacts_refresh_hook_installed();
        let reactive = core.reactive().clone();
        let spawner = core.runtime().and_then(|runtime| runtime.task_spawner());
        (reactive, spawner, should_install)
    };

    if !should_install {
        return Ok(());
    }

    let Some(spawner) = spawner else {
        return Ok(());
    };

    let app_core = Arc::clone(app_core);
    let refresh_in_flight = Arc::new(AtomicBool::new(false));
    let refresh_pending = Arc::new(AtomicBool::new(false));
    let refresh_spawner = spawner.clone();

    spawner.spawn_cancellable(
        Box::pin(async move {
            let mut stream = reactive.subscribe(&*CONTACTS_SIGNAL);
            loop {
                let Ok(_) = stream.recv().await else {
                    break;
                };

                if refresh_in_flight.swap(true, Ordering::SeqCst) {
                    refresh_pending.store(true, Ordering::SeqCst);
                    continue;
                }

                let refresh_app_core = app_core.clone();
                let refresh_in_flight = refresh_in_flight.clone();
                let refresh_pending = refresh_pending.clone();
                refresh_spawner.spawn(Box::pin(async move {
                    loop {
                        let _ = refresh_connection_status_from_contacts(&refresh_app_core).await;

                        if refresh_pending.swap(false, Ordering::SeqCst) {
                            continue;
                        }

                        refresh_in_flight.store(false, Ordering::SeqCst);
                        break;
                    }
                }));
            }
        }),
        spawner.cancellation_token(),
    );

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

    // === OTA Parsing Tests ===

    #[test]
    fn test_parse_upgrade_kind_soft() {
        let result = parse_upgrade_kind("soft");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), UpgradeKindValue::Soft);
    }

    #[test]
    fn test_parse_upgrade_kind_hard() {
        let result = parse_upgrade_kind("hard");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), UpgradeKindValue::Hard);
    }

    #[test]
    fn test_parse_upgrade_kind_case_insensitive() {
        assert!(parse_upgrade_kind("SOFT").is_ok());
        assert!(parse_upgrade_kind("Hard").is_ok());
        assert!(parse_upgrade_kind("HARD").is_ok());
    }

    #[test]
    fn test_parse_upgrade_kind_invalid() {
        let result = parse_upgrade_kind("invalid");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("soft"));
    }

    #[test]
    fn test_upgrade_kind_as_str() {
        assert_eq!(UpgradeKindValue::Soft.as_str(), "soft");
        assert_eq!(UpgradeKindValue::Hard.as_str(), "hard");
    }

    #[test]
    fn test_upgrade_kind_display() {
        assert_eq!(format!("{}", UpgradeKindValue::Soft), "soft");
        assert_eq!(format!("{}", UpgradeKindValue::Hard), "hard");
    }

    #[test]
    fn test_parse_semantic_version_valid() {
        let result = parse_semantic_version("1.2.3");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), (1, 2, 3));
    }

    #[test]
    fn test_parse_semantic_version_zeros() {
        let result = parse_semantic_version("0.0.0");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), (0, 0, 0));
    }

    #[test]
    fn test_parse_semantic_version_large_numbers() {
        let result = parse_semantic_version("100.200.300");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), (100, 200, 300));
    }

    #[test]
    fn test_parse_semantic_version_too_few_parts() {
        let result = parse_semantic_version("1.2");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("major.minor.patch"));
    }

    #[test]
    fn test_parse_semantic_version_too_many_parts() {
        let result = parse_semantic_version("1.2.3.4");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_semantic_version_non_numeric() {
        let result = parse_semantic_version("1.x.3");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("minor"));
    }

    #[test]
    fn test_parse_semantic_version_overflow() {
        // u16 max is 65535
        let result = parse_semantic_version("70000.0.0");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_version_string_valid() {
        let result = validate_version_string("1.2.3");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "1.2.3");
    }

    #[test]
    fn test_validate_version_string_invalid() {
        let result = validate_version_string("invalid");
        assert!(result.is_err());
    }

    // === System Operation Tests ===

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
