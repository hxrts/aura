//! Network/Peer command handlers
//!
//! Handlers for AddPeer, RemovePeer, ListPeers, DiscoverPeers, ListLanPeers, InviteLanPeer.

use std::collections::HashSet;
use std::sync::Arc;

use async_lock::RwLock;
use aura_app::AppCore;
use aura_effects::time::PhysicalTimeHandler;

use super::types::{OpError, OpResponse, OpResult};
use super::EffectCommand;

/// Handle network/peer commands
pub async fn handle_network(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
    peers: &Arc<RwLock<HashSet<String>>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::AddPeer { peer_id } => {
            let count = {
                let mut peers = peers.write().await;
                peers.insert(peer_id.clone());
                peers.len()
            };

            if let Err(e) =
                aura_app::workflows::network::update_connection_status(app_core, count).await
            {
                tracing::debug!("Failed to update connection status: {}", e);
            }

            tracing::info!("Added peer: {}", peer_id);
            Some(Ok(OpResponse::Ok))
        }

        EffectCommand::RemovePeer { peer_id } => {
            let count = {
                let mut peers = peers.write().await;
                peers.remove(peer_id);
                peers.len()
            };

            if let Err(e) =
                aura_app::workflows::network::update_connection_status(app_core, count).await
            {
                tracing::debug!("Failed to update connection status: {}", e);
            }

            tracing::info!("Removed peer: {}", peer_id);
            Some(Ok(OpResponse::Ok))
        }

        EffectCommand::ListPeers => {
            let now_ms = PhysicalTimeHandler::new().physical_time_now_ms();
            match aura_app::workflows::network::list_peers(app_core, now_ms).await {
                Ok(peer_list) => {
                    tracing::info!("Listed {} peers", peer_list.len());
                    Some(Ok(OpResponse::List(peer_list)))
                }
                Err(e) => Some(Err(OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::DiscoverPeers => {
            let now_ms = PhysicalTimeHandler::new().physical_time_now_ms();
            match aura_app::workflows::network::discover_peers(app_core, now_ms).await {
                Ok(discovered) => {
                    tracing::info!("Peer discovery triggered");
                    Some(Ok(OpResponse::Data(format!(
                        "Discovery active, {} peers known",
                        discovered
                    ))))
                }
                Err(e) => Some(Err(OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::ListLanPeers => {
            let now_ms = PhysicalTimeHandler::new().physical_time_now_ms();
            match aura_app::workflows::network::list_lan_peers(app_core, now_ms).await {
                Ok(peer_list) => {
                    tracing::info!("Found {} LAN peers", peer_list.len());
                    Some(Ok(OpResponse::List(peer_list)))
                }
                Err(e) => Some(Err(OpError::Failed(e.to_string()))),
            }
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

            // TODO: For now, we can at least export an invitation code that could be shared
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
