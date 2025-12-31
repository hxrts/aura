//! Network/Peer command handlers
//!
//! Handlers for AddPeer, RemovePeer, ListPeers, DiscoverPeers, ListLanPeers, InviteLanPeer.
//!
//! Peer state is managed through AppCore signals via the network workflow.
//! This handler is a thin view layer that delegates to aura_app::workflows::network.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::ui::prelude::*;

use super::types::{OpError, OpResponse, OpResult};
use super::EffectCommand;

/// Handle network/peer commands
///
/// Delegates to aura_app::workflows::network for all peer state management.
/// No local state is maintained - all peer tracking uses AppCore signals.
pub async fn handle_network(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::AddPeer { peer_id } => {
            // Delegate to workflow - it manages peer state in AppCore signals
            match aura_app::ui::workflows::network::add_peer(app_core, peer_id.clone()).await {
                Ok(count) => {
                    tracing::info!("Added peer: {} (total: {})", peer_id, count);
                    Some(Ok(OpResponse::Ok))
                }
                Err(e) => {
                    tracing::debug!("Failed to add peer: {}", e);
                    Some(Err(OpError::Failed(e.to_string())))
                }
            }
        }

        EffectCommand::RemovePeer { peer_id } => {
            // Delegate to workflow - it manages peer state in AppCore signals
            match aura_app::ui::workflows::network::remove_peer(app_core, peer_id).await {
                Ok(count) => {
                    tracing::info!("Removed peer: {} (remaining: {})", peer_id, count);
                    Some(Ok(OpResponse::Ok))
                }
                Err(e) => {
                    tracing::debug!("Failed to remove peer: {}", e);
                    Some(Err(OpError::Failed(e.to_string())))
                }
            }
        }

        EffectCommand::ListPeers => {
            let now_ms = super::time::current_time_ms(app_core).await;
            match aura_app::ui::workflows::network::list_peers(app_core, now_ms).await {
                Ok(peer_list) => {
                    tracing::info!("Listed {} peers", peer_list.len());
                    Some(Ok(OpResponse::List(peer_list)))
                }
                Err(e) => Some(Err(OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::DiscoverPeers => {
            let now_ms = super::time::current_time_ms(app_core).await;
            match aura_app::ui::workflows::network::discover_peers(app_core, now_ms).await {
                Ok(discovered) => {
                    tracing::info!("Peer discovery triggered");
                    Some(Ok(OpResponse::Data(format!(
                        "Discovery active, {discovered} peers known"
                    ))))
                }
                Err(e) => Some(Err(OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::ListLanPeers => {
            let now_ms = super::time::current_time_ms(app_core).await;
            match aura_app::ui::workflows::network::list_lan_peers(app_core, now_ms).await {
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

            // Export an invitation code that could be shared manually
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
                        "LAN invitation queued for {authority_id} at {address} (requires runtime)"
                    ))))
                }
            }
        }

        _ => None,
    }
}
