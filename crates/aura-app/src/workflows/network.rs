//! Network Workflow - Portable Business Logic
//!
//! This module contains network/peer operations that are portable across all frontends.
//! It follows the reactive signal pattern and uses RuntimeBridge for runtime operations.
//!
//! Peer state is managed through AppCore signals - terminals should not maintain local
//! peer state.

use crate::workflows::observed_projection::update_neighborhood_projection_observed;
use crate::workflows::runtime::timeout_runtime_call;
use crate::workflows::signals::emit_signal;
use crate::workflows::signals::read_signal_or_default;
use crate::{
    signal_defs::{
        ConnectionStatus, DiscoveredPeer, DiscoveredPeerMethod, DiscoveredPeersState,
        CONNECTION_STATUS_SIGNAL, CONNECTION_STATUS_SIGNAL_NAME, DISCOVERED_PEERS_SIGNAL,
        DISCOVERED_PEERS_SIGNAL_NAME,
    },
    AppCore,
};
use async_lock::RwLock;
use aura_core::types::identifiers::AuthorityId;
use aura_core::AuraError;
use std::{collections::HashSet, sync::Arc, time::Duration};

/// Period between automatic discovered-peer refreshes in interactive frontends.
pub const DISCOVERED_PEERS_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

// ============================================================================
// Peer Management (connected peer tracking)
// ============================================================================

/// Add a connected peer and update connection status.
///
/// **What it does**: Adds peer to NeighborhoodState and emits CONNECTION_STATUS_SIGNAL
/// **Returns**: Current peer count after adding
/// **Signal pattern**: Emits CONNECTION_STATUS_SIGNAL
///
/// Terminals should call this instead of maintaining local peer state.
pub async fn add_peer(
    app_core: &Arc<RwLock<AppCore>>,
    peer_id: AuthorityId,
) -> Result<usize, AuraError> {
    // OWNERSHIP: observed-display-update
    let count = update_neighborhood_projection_observed(app_core, |state| {
        state.add_connected_peer(peer_id);
        state.connected_peer_count()
    })
    .await?;

    // Update connection status signal
    update_connection_status(app_core, count).await?;

    Ok(count)
}

/// Remove a connected peer and update connection status.
///
/// **What it does**: Removes peer from NeighborhoodState and emits CONNECTION_STATUS_SIGNAL
/// **Returns**: Current peer count after removing
/// **Signal pattern**: Emits CONNECTION_STATUS_SIGNAL
///
/// Terminals should call this instead of maintaining local peer state.
pub async fn remove_peer(
    app_core: &Arc<RwLock<AppCore>>,
    peer_id: &AuthorityId,
) -> Result<usize, AuraError> {
    // OWNERSHIP: observed-display-update
    let count = update_neighborhood_projection_observed(app_core, |state| {
        state.remove_connected_peer(peer_id);
        state.connected_peer_count()
    })
    .await?;

    // Update connection status signal
    update_connection_status(app_core, count).await?;

    Ok(count)
}

/// Get the set of currently connected peers.
///
/// **What it does**: Returns the current connected peer set from NeighborhoodState
/// **Signal pattern**: Read-only operation (no emission)
pub async fn get_connected_peers(app_core: &Arc<RwLock<AppCore>>) -> HashSet<AuthorityId> {
    let state = read_signal_or_default(app_core, &*crate::signal_defs::NEIGHBORHOOD_SIGNAL).await;
    state.connected_peers().clone()
}

// ============================================================================
// Peer Discovery
// ============================================================================

/// List all known peers (sync and discovered)
///
/// **What it does**: Queries peers from RuntimeBridge and emits DISCOVERED_PEERS_SIGNAL
/// **Returns**: List of peer strings (sync:DeviceId, discovered:AuthorityId)
/// **Signal pattern**: Emits DISCOVERED_PEERS_SIGNAL after query
///
/// # Arguments
/// * `app_core` - The application core
/// * `timestamp_ms` - Current timestamp in milliseconds (caller provides via effect system)
pub async fn list_peers(
    app_core: &Arc<RwLock<AppCore>>,
    timestamp_ms: u64,
) -> Result<Vec<String>, AuraError> {
    let runtime = super::runtime::require_runtime(app_core).await?;

    // Get sync peers (DeviceIds)
    let sync_peers = timeout_runtime_call(
        &runtime,
        "list_peers",
        "try_get_sync_peers",
        Duration::from_millis(5_000),
        || runtime.try_get_sync_peers(),
    )
    .await?
    .map_err(|e| AuraError::from(super::error::runtime_call("query sync peers", e)))?;

    // Get discovered peers (AuthorityIds from rendezvous)
    let discovered_peers = timeout_runtime_call(
        &runtime,
        "list_peers",
        "try_get_discovered_peers",
        Duration::from_millis(5_000),
        || runtime.try_get_discovered_peers(),
    )
    .await?
    .map_err(|e| AuraError::from(super::error::runtime_call("query discovered peers", e)))?;

    // Combine into a list of strings
    let mut peer_list: Vec<String> = sync_peers.iter().map(|d| format!("sync:{d}")).collect();

    peer_list.extend(discovered_peers.iter().map(|a| format!("discovered:{a}")));

    // Emit discovered peers signal
    emit_discovered_peers_signal(app_core, timestamp_ms).await?;

    Ok(peer_list)
}

/// Discover peers via rendezvous
///
/// **What it does**: Triggers peer discovery and emits DISCOVERED_PEERS_SIGNAL
/// **Returns**: Number of discovered peers
/// **Signal pattern**: Emits DISCOVERED_PEERS_SIGNAL after discovery
///
/// # Arguments
/// * `app_core` - The application core
/// * `timestamp_ms` - Current timestamp in milliseconds (caller provides via effect system)
pub async fn discover_peers(
    app_core: &Arc<RwLock<AppCore>>,
    timestamp_ms: u64,
) -> Result<usize, AuraError> {
    let runtime = super::runtime::require_runtime(app_core).await?;
    let discovered_count = timeout_runtime_call(
        &runtime,
        "discover_peers",
        "try_get_discovered_peers",
        Duration::from_millis(5_000),
        || runtime.try_get_discovered_peers(),
    )
    .await?
    .map_err(|e| AuraError::from(super::error::runtime_call("discover peers", e)))?
    .len();

    // Emit discovered peers signal
    emit_discovered_peers_signal(app_core, timestamp_ms).await?;

    Ok(discovered_count)
}

/// List LAN-discovered peers
///
/// **What it does**: Queries LAN peers and emits DISCOVERED_PEERS_SIGNAL
/// **Returns**: List of peer descriptions (authority_id @ address)
/// **Signal pattern**: Emits DISCOVERED_PEERS_SIGNAL after query
///
/// # Arguments
/// * `app_core` - The application core
/// * `timestamp_ms` - Current timestamp in milliseconds (caller provides via effect system)
pub async fn list_lan_peers(
    app_core: &Arc<RwLock<AppCore>>,
    timestamp_ms: u64,
) -> Result<Vec<String>, AuraError> {
    let runtime = super::runtime::require_runtime(app_core).await?;
    let lan_peers = timeout_runtime_call(
        &runtime,
        "list_lan_peers",
        "try_get_lan_peers",
        Duration::from_millis(5_000),
        || runtime.try_get_lan_peers(),
    )
    .await?
    .map_err(|e| AuraError::from(super::error::runtime_call("list lan peers", e)))?;

    let peer_list: Vec<String> = lan_peers
        .iter()
        .map(|peer| format!("{} ({})", peer.authority_id, peer.address))
        .collect();

    // Emit discovered peers signal
    emit_discovered_peers_signal(app_core, timestamp_ms).await?;

    Ok(peer_list)
}

/// Set connection status directly.
///
/// **What it does**: Emits CONNECTION_STATUS_SIGNAL with provided status
/// **Signal pattern**: Emits CONNECTION_STATUS_SIGNAL
pub async fn set_connection_status(
    app_core: &Arc<RwLock<AppCore>>,
    status: ConnectionStatus,
) -> Result<(), AuraError> {
    emit_signal(
        app_core,
        &*CONNECTION_STATUS_SIGNAL,
        status,
        CONNECTION_STATUS_SIGNAL_NAME,
    )
    .await
}

/// Update connection status with peer count
///
/// **What it does**: Emits CONNECTION_STATUS_SIGNAL with online/offline status
/// **Returns**: Unit result
/// **Signal pattern**: Emits CONNECTION_STATUS_SIGNAL
pub async fn update_connection_status(
    app_core: &Arc<RwLock<AppCore>>,
    peer_count: usize,
) -> Result<(), AuraError> {
    let status = if peer_count == 0 {
        ConnectionStatus::Offline
    } else {
        ConnectionStatus::Online { peer_count }
    };

    emit_signal(
        app_core,
        &*CONNECTION_STATUS_SIGNAL,
        status,
        CONNECTION_STATUS_SIGNAL_NAME,
    )
    .await?;

    Ok(())
}

/// Get current discovered peers state
///
/// **What it does**: Reads discovered peers from DISCOVERED_PEERS_SIGNAL
/// **Returns**: Current discovered peers state
/// **Signal pattern**: Read-only operation (no emission)
pub async fn get_discovered_peers(app_core: &Arc<RwLock<AppCore>>) -> DiscoveredPeersState {
    read_signal_or_default(app_core, &*DISCOVERED_PEERS_SIGNAL).await
}

/// Re-query discovered peers from runtime and emit the signal.
///
/// **What it does**: Queries rendezvous + LAN peers and emits DISCOVERED_PEERS_SIGNAL
/// **Signal pattern**: Emits DISCOVERED_PEERS_SIGNAL
///
/// Use this instead of `get_discovered_peers()` when you need to refresh from
/// the runtime (e.g. after contact acceptance changes the invited set).
pub async fn refresh_discovered_peers(app_core: &Arc<RwLock<AppCore>>) -> Result<(), AuraError> {
    let timestamp_ms = super::time::current_time_ms(app_core).await?;
    emit_discovered_peers_signal(app_core, timestamp_ms).await
}

/// Emit discovered peers signal with current state
///
/// **What it does**: Queries peers and emits DISCOVERED_PEERS_SIGNAL
/// **Returns**: Unit result
/// **Signal pattern**: Emits DISCOVERED_PEERS_SIGNAL
///
/// This is a helper function that combines rendezvous and LAN peers into a single signal.
///
/// # Arguments
/// * `app_core` - The application core
/// * `timestamp_ms` - Current timestamp in milliseconds (caller provides via effect system)
async fn emit_discovered_peers_signal(
    app_core: &Arc<RwLock<AppCore>>,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = super::runtime::require_runtime(app_core).await?;
    // Get both rendezvous and LAN peers without holding the app-core lock across awaits.
    let rendezvous_peers = timeout_runtime_call(
        &runtime,
        "emit_discovered_peers_signal",
        "try_get_discovered_peers",
        Duration::from_millis(5_000),
        || runtime.try_get_discovered_peers(),
    )
    .await?
    .map_err(|e| AuraError::from(super::error::runtime_call("refresh discovered peers", e)))?;
    let lan_peers = timeout_runtime_call(
        &runtime,
        "emit_discovered_peers_signal",
        "try_get_lan_peers",
        Duration::from_millis(5_000),
        || runtime.try_get_lan_peers(),
    )
    .await?
    .map_err(|e| AuraError::from(super::error::runtime_call("refresh lan peers", e)))?;

    // Get invited peer IDs to mark peers as invited
    let invited_ids: HashSet<AuthorityId> = timeout_runtime_call(
        &runtime,
        "emit_discovered_peers_signal",
        "try_get_invited_peer_ids",
        Duration::from_millis(5_000),
        || runtime.try_get_invited_peer_ids(),
    )
    .await?
    .map_err(|e| AuraError::from(super::error::runtime_call("get invited peers", e)))?
    .into_iter()
    .collect();

    // Combine into discovered peers state
    let mut peers = Vec::new();

    // Add rendezvous peers
    for peer in rendezvous_peers {
        peers.push(DiscoveredPeer {
            authority_id: peer,
            address: String::new(),
            method: DiscoveredPeerMethod::Rendezvous,
            invited: invited_ids.contains(&peer),
        });
    }

    // Add LAN peers (avoiding duplicates)
    for peer in lan_peers {
        if !peers.iter().any(|p| p.authority_id == peer.authority_id) {
            peers.push(DiscoveredPeer {
                authority_id: peer.authority_id,
                address: peer.address,
                method: DiscoveredPeerMethod::Lan,
                invited: invited_ids.contains(&peer.authority_id),
            });
        }
    }

    let state = DiscoveredPeersState {
        peers,
        last_updated_ms: timestamp_ms,
    };

    // Emit the signal
    emit_signal(
        app_core,
        &*DISCOVERED_PEERS_SIGNAL,
        state,
        DISCOVERED_PEERS_SIGNAL_NAME,
    )
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[tokio::test]
    async fn test_get_discovered_peers_default() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);

        let state = get_discovered_peers(&app_core).await;
        assert!(state.peers.is_empty());
        assert_eq!(state.last_updated_ms, 0);
    }
}
