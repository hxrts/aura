//! Network Workflow - Portable Business Logic
//!
//! This module contains network/peer operations that are portable across all frontends.
//! It follows the reactive signal pattern and uses RuntimeBridge for runtime operations.

use crate::{
    signal_defs::{
        ConnectionStatus, DiscoveredPeer, DiscoveredPeerMethod, DiscoveredPeersState,
        CONNECTION_STATUS_SIGNAL, DISCOVERED_PEERS_SIGNAL,
    },
    AppCore,
};
use async_lock::RwLock;
use aura_core::{identifiers::AuthorityId, AuraError};
use std::{collections::HashSet, sync::Arc};
use crate::workflows::signals::emit_signal;
use crate::workflows::signals::read_signal_or_default;

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
    // Get sync peers (DeviceIds)
    let app_core_guard = app_core.read().await;
    let sync_peers = app_core_guard
        .sync_peers()
        .await
        .unwrap_or_else(|_e| vec![]);

    // Get discovered peers (AuthorityIds from rendezvous)
    let discovered_peers = app_core_guard
        .discover_peers()
        .await
        .unwrap_or_else(|_e| vec![]);

    // Combine into a list of strings
    let mut peer_list: Vec<String> = sync_peers.iter().map(|d| format!("sync:{}", d)).collect();

    peer_list.extend(discovered_peers.iter().map(|a| format!("discovered:{}", a)));

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
    let app_core_guard = app_core.read().await;
    let discovered_count = app_core_guard
        .discover_peers()
        .await
        .map(|peers| peers.len())
        .unwrap_or(0);

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
    let app_core_guard = app_core.read().await;
    let lan_peers = app_core_guard.get_lan_peers().await;

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
        "CONNECTION_STATUS_SIGNAL",
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
        "CONNECTION_STATUS_SIGNAL",
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
    let app_core_guard = app_core.read().await;
    // Get both rendezvous and LAN peers
    let rendezvous_peers = app_core_guard.discover_peers().await.unwrap_or_default();
    let lan_peers = app_core_guard.get_lan_peers().await;

    // Get invited peer IDs to mark peers as invited
    let invited_ids: HashSet<AuthorityId> = if let Some(runtime) = app_core_guard.runtime() {
        runtime
            .get_invited_peer_ids()
            .await
            .into_iter()
            .filter_map(|id| id.parse::<AuthorityId>().ok())
            .collect()
    } else {
        HashSet::new()
    };

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
        "DISCOVERED_PEERS_SIGNAL",
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
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let state = get_discovered_peers(&app_core).await;
        assert!(state.peers.is_empty());
        assert_eq!(state.last_updated_ms, 0);
    }
}
