//! Network/Peer command handlers
//!
//! Handlers for AddPeer, RemovePeer, ListPeers, DiscoverPeers, ListLanPeers, InviteLanPeer.
//!
//! Peer state is managed through AppCore signals via the network workflow.
//! This handler is a thin view layer that delegates to aura_app::workflows::network.

use std::sync::Arc;
use std::time::Duration;

use super::types::{OpError, OpResponse, OpResult};
use super::EffectCommand;
use async_lock::RwLock;
use aura_app::ui::prelude::*;
use aura_app::ui::workflows::runtime as runtime_workflows;

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
            match aura_app::ui::workflows::network::add_peer(app_core, *peer_id).await {
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
                    Some(Ok(OpResponse::PeersListed { peers: peer_list }))
                }
                Err(e) => Some(Err(OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::DiscoverPeers => {
            let now_ms = super::time::current_time_ms(app_core).await;
            match aura_app::ui::workflows::network::discover_peers(app_core, now_ms).await {
                Ok(discovered) => {
                    tracing::info!("Peer discovery triggered");
                    Some(Ok(OpResponse::PeerDiscoveryTriggered {
                        known_peers: discovered,
                    }))
                }
                Err(e) => Some(Err(OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::ListLanPeers => {
            let now_ms = super::time::current_time_ms(app_core).await;
            match aura_app::ui::workflows::network::list_lan_peers(app_core, now_ms).await {
                Ok(peer_list) => {
                    tracing::info!("Found {} LAN peers", peer_list.len());
                    Some(Ok(OpResponse::LanPeersListed { peers: peer_list }))
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
            // 2. Export the invite code
            // 3. Send the code to the peer's address via LAN transport
            tracing::info!(
                "Inviting LAN peer: authority={} at address={}",
                authority_id,
                address
            );

            let app_core_guard = app_core.read().await;

            // Generate invitation ID from authority
            let authority_id_str = authority_id.to_string();
            let invitation_id = format!(
                "lan-invite-{}",
                &authority_id_str[..8.min(authority_id_str.len())]
            );

            // Export the invite code
            let code = match app_core_guard.export_invitation(&invitation_id).await {
                Ok(code) => code,
                Err(e) => {
                    tracing::debug!("Could not export invitation (no runtime): {}", e);
                    return Some(Ok(OpResponse::LanInvitationStatus {
                        authority_id: authority_id.to_string(),
                        address: address.clone(),
                        message: format!(
                            "LAN invitation queued for {authority_id} at {address} (requires runtime)"
                        ),
                    }));
                }
            };

            // Get the runtime bridge to send the invitation via LAN
            if let Some(runtime) = app_core_guard.runtime() {
                // Create LanPeerInfo for the bridge call (type from ui::prelude)
                let peer_info = LanPeerInfo {
                    authority_id: *authority_id,
                    address: address.clone(),
                    discovered_at_ms: 0,
                    nickname_suggestion: None,
                };

                match runtime_workflows::timeout_runtime_call(
                    runtime,
                    "terminal_invite_lan_peer",
                    "send_lan_invitation",
                    Duration::from_secs(5),
                    || runtime.send_lan_invitation(&peer_info, &code),
                )
                .await
                {
                    Ok(Ok(())) => {
                        tracing::info!("Sent LAN invitation to {}", address);
                        Some(Ok(OpResponse::LanInvitationStatus {
                            authority_id: authority_id.to_string(),
                            address: address.clone(),
                            message: format!("Invitation sent to {address} via LAN"),
                        }))
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("Failed to send LAN invitation: {}", e);
                        // Fall back to showing the code for manual sharing
                        Some(Ok(OpResponse::LanInvitationStatus {
                            authority_id: authority_id.to_string(),
                            address: address.clone(),
                            message: format!(
                                "LAN send failed ({}), share code manually: {}",
                                e,
                                &code[..50.min(code.len())]
                            ),
                        }))
                    }
                    Err(e) => {
                        tracing::warn!("Failed to send LAN invitation: {}", e);
                        // Fall back to showing the code for manual sharing
                        Some(Ok(OpResponse::LanInvitationStatus {
                            authority_id: authority_id.to_string(),
                            address: address.clone(),
                            message: format!(
                                "LAN send failed ({}), share code manually: {}",
                                e,
                                &code[..50.min(code.len())]
                            ),
                        }))
                    }
                }
            } else {
                // No runtime - show code for manual sharing
                Some(Ok(OpResponse::LanInvitationStatus {
                    authority_id: authority_id.to_string(),
                    address: address.clone(),
                    message: format!(
                        "No runtime available. Share invite code manually: {}",
                        &code[..50.min(code.len())]
                    ),
                }))
            }
        }

        _ => None,
    }
}
