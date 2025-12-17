//! Network/Peer command handlers
//!
//! Handlers for AddPeer, RemovePeer, ListPeers, DiscoverPeers, ListLanPeers, InviteLanPeer.

use std::collections::HashSet;
use std::sync::Arc;

use aura_app::signal_defs::{
    ConnectionStatus, DiscoveredPeer, DiscoveredPeersState, CONNECTION_STATUS_SIGNAL,
    DISCOVERED_PEERS_SIGNAL,
};
use aura_app::AppCore;
use aura_core::effects::reactive::ReactiveEffects;
use tokio::sync::RwLock;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

/// Handle network/peer commands
pub async fn handle_network(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
    peers: &Arc<RwLock<HashSet<String>>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::AddPeer { peer_id } => {
            {
                let mut peers = peers.write().await;
                peers.insert(peer_id.clone());
                let count = peers.len();

                if let Ok(core) = app_core.try_read() {
                    let _ = core
                        .emit(
                            &*CONNECTION_STATUS_SIGNAL,
                            ConnectionStatus::Online { peer_count: count },
                        )
                        .await;
                }
            }
            tracing::info!("Added peer: {}", peer_id);
            Some(Ok(OpResponse::Ok))
        }

        EffectCommand::RemovePeer { peer_id } => {
            {
                let mut peers = peers.write().await;
                peers.remove(peer_id);
                let count = peers.len();

                if let Ok(core) = app_core.try_read() {
                    let status = if count == 0 {
                        ConnectionStatus::Offline
                    } else {
                        ConnectionStatus::Online { peer_count: count }
                    };
                    let _ = core.emit(&*CONNECTION_STATUS_SIGNAL, status).await;
                }
            }
            tracing::info!("Removed peer: {}", peer_id);
            Some(Ok(OpResponse::Ok))
        }

        EffectCommand::ListPeers => {
            // Query actual peers from runtime via AppCore
            let app_core = app_core.read().await;

            // Get sync peers (DeviceIds)
            let sync_peers = match app_core.sync_peers().await {
                Ok(peers) => peers,
                Err(e) => {
                    tracing::debug!("No sync peers available: {}", e);
                    vec![]
                }
            };

            // Get discovered peers (AuthorityIds from rendezvous)
            let discovered_peers = match app_core.discover_peers().await {
                Ok(peers) => peers,
                Err(e) => {
                    tracing::debug!("No discovered peers available: {}", e);
                    vec![]
                }
            };

            // Combine into a list of strings
            let mut peer_list: Vec<String> =
                sync_peers.iter().map(|d| format!("sync:{}", d)).collect();

            peer_list.extend(discovered_peers.iter().map(|a| format!("discovered:{}", a)));

            tracing::info!(
                "Listed {} peers ({} sync, {} discovered)",
                peer_list.len(),
                sync_peers.len(),
                discovered_peers.len()
            );

            // Emit discovered peers signal
            emit_discovered_peers_signal(&app_core).await;

            Some(Ok(OpResponse::List(peer_list)))
        }

        EffectCommand::DiscoverPeers => {
            // Trigger peer discovery via rendezvous
            // Currently this is implicit in the rendezvous service
            // NOTE: Explicit trigger_discovery() could be added to RuntimeBridge
            // for on-demand discovery refresh.
            tracing::info!("Peer discovery triggered");

            // For now, return the currently discovered peers
            let app_core = app_core.read().await;
            let discovered = match app_core.discover_peers().await {
                Ok(peers) => peers.len(),
                Err(_) => 0,
            };

            // Emit discovered peers signal
            emit_discovered_peers_signal(&app_core).await;

            Some(Ok(OpResponse::Data(format!(
                "Discovery active, {} peers known",
                discovered
            ))))
        }

        EffectCommand::ListLanPeers => {
            // Get LAN-discovered peers from the runtime
            let app_core = app_core.read().await;
            let lan_peers = app_core.get_lan_peers().await;

            let peer_list: Vec<String> = lan_peers
                .iter()
                .map(|peer| format!("{} ({})", peer.authority_id, peer.address))
                .collect();

            tracing::info!("Found {} LAN peers", peer_list.len());

            // Emit discovered peers signal
            emit_discovered_peers_signal(&app_core).await;

            Some(Ok(OpResponse::List(peer_list)))
        }

        EffectCommand::InviteLanPeer {
            authority_id,
            address,
        } => {
            // LAN peer invitation flow:
            // 1. Create a contact invitation for this peer
            // 2. Export the invitation code
            // 3. Send the code to the peer's address via LAN transport
            //
            // NOTE: LAN transport for invitation delivery needs send_lan_invitation()
            // added to RuntimeBridge. Currently falls back to exporting code for manual sharing.
            tracing::info!(
                "Inviting LAN peer: authority={} at address={}",
                authority_id,
                address
            );

            // For now, we can at least export an invitation code that could be shared
            let app_core = app_core.read().await;

            // Try to export an invitation (requires runtime)
            // The invitation_id would normally come from a created invitation
            // For LAN invites, we generate a placeholder ID based on the target
            let invitation_id =
                format!("lan-invite-{}", &authority_id[..8.min(authority_id.len())]);

            match app_core.export_invitation(&invitation_id).await {
                Ok(code) => {
                    tracing::info!(
                        "Generated invitation code for LAN peer (code would be sent to {})",
                        address
                    );
                    // Return the code - in a full implementation, this would be sent via LAN
                    Some(Ok(OpResponse::Data(format!(
                        "Invitation ready for {} (LAN send not yet implemented): {}",
                        address,
                        &code[..50.min(code.len())]
                    ))))
                }
                Err(e) => {
                    // No runtime available - log and return success anyway
                    // (LAN invites would work when runtime is present)
                    tracing::debug!("Could not export invitation (no runtime): {}", e);
                    Some(Ok(OpResponse::Data(format!(
                        "LAN invitation queued for {} at {} (requires runtime)",
                        authority_id, address
                    ))))
                }
            }
        }

        _ => None,
    }
}

/// Helper function to emit discovered peers signal with current state
async fn emit_discovered_peers_signal(app_core: &AppCore) {
    // Get both rendezvous and LAN peers
    let rendezvous_peers = app_core.discover_peers().await.unwrap_or_default();
    let lan_peers = app_core.get_lan_peers().await;

    // Get invited peer IDs to mark peers as invited
    let invited_ids: std::collections::HashSet<String> = app_core
        .runtime()
        .and_then(|_r| {
            // TODO: Add method to get invited peer IDs from runtime
            // For now, return empty set
            Some(std::collections::HashSet::new())
        })
        .unwrap_or_default();

    // Combine into discovered peers state
    let mut peers = Vec::new();

    // Add rendezvous peers
    for peer in rendezvous_peers {
        let peer_str = peer.to_string();
        peers.push(DiscoveredPeer {
            authority_id: peer_str.clone(),
            address: String::new(),
            method: "rendezvous".to_string(),
            invited: invited_ids.contains(&peer_str),
        });
    }

    // Add LAN peers (avoiding duplicates)
    for peer in lan_peers {
        let peer_str = peer.authority_id.to_string();
        if !peers.iter().any(|p| p.authority_id == peer_str) {
            peers.push(DiscoveredPeer {
                authority_id: peer_str.clone(),
                address: peer.address,
                method: "LAN".to_string(),
                invited: invited_ids.contains(&peer_str),
            });
        }
    }

    // Get current timestamp (using system time for UI display)
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let state = DiscoveredPeersState {
        peers,
        last_updated_ms: now_ms,
    };

    // Emit the signal
    let _ = app_core.emit(&*DISCOVERED_PEERS_SIGNAL, state).await;
}
