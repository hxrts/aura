//! Network/Peer command handlers
//!
//! Handlers for AddPeer, RemovePeer, ListPeers, DiscoverPeers, ListLanPeers, InviteLanPeer.
//!
//! Peer state is managed through AppCore signals via the network workflow.
//! This handler is a thin view layer that delegates to aura_app::workflows::network.

use std::sync::Arc;
use std::time::Duration;

use super::types::{OpError, OpFailureCode, OpResponse, OpResult};
use super::EffectCommand;
use async_lock::RwLock;
use aura_app::ui::prelude::*;
use aura_app::ui::workflows::runtime as runtime_workflows;
use aura_core::effects::reactive::ReactiveEffects;

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
            match aura_app::ui::workflows::network::list_bootstrap_candidates(app_core, now_ms)
                .await
            {
                Ok(peer_list) => {
                    tracing::info!("Found {} bootstrap candidates", peer_list.len());
                    Some(Ok(OpResponse::LanPeersListed { peers: peer_list }))
                }
                Err(e) => Some(Err(OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::InviteLanPeer {
            authority_id,
            address,
        } => {
            // Bootstrap-candidate invitation flow:
            // 1. Create a contact invitation for this peer
            // 2. Export the invite code
            // 3. Send the code to the peer's discovered bootstrap address
            tracing::info!(
                "Inviting bootstrap candidate: authority={} at address={}",
                authority_id,
                address
            );

            // Name ourselves so the peer's request reads "from <nickname>".
            let sender_nickname = {
                let core = app_core.read().await;
                core.read(&*aura_app::ui::signals::SETTINGS_SIGNAL)
                    .await
                    .ok()
                    .map(|settings| settings.nickname_suggestion.trim().to_string())
                    .filter(|name| !name.is_empty())
            };
            let invitation = match aura_app::ui::workflows::invitation::create_contact_invitation(
                app_core,
                *authority_id,
                sender_nickname,
                None,
                None,
                None,
            )
            .await
            {
                Ok(invitation) => invitation,
                Err(error) => {
                    return Some(Err(OpError::typed(
                        OpFailureCode::CreateContactInvitation,
                        format!("Could not create an invitation for {authority_id}: {error}"),
                    )))
                }
            };
            let code = match aura_app::ui::workflows::invitation::export_invitation(
                app_core,
                invitation.invitation_id(),
            )
            .await
            {
                Ok(code) => code,
                Err(error) => {
                    return Some(Err(OpError::typed(
                        OpFailureCode::ExportInvitation,
                        format!("Could not export the invitation for {authority_id}: {error}"),
                    )))
                }
            };

            let runtime = {
                let app_core_guard = app_core.read().await;
                app_core_guard.runtime().cloned()
            };
            let Some(runtime) = runtime else {
                return Some(Err(OpError::Failed(
                    "No runtime available to send the invitation".to_string(),
                )));
            };
            let peer_info = BootstrapCandidateInfo {
                authority_id: *authority_id,
                origin: BootstrapCandidateOrigin::Lan,
                address: address.clone(),
                discovered_at_ms: 0,
                nickname_suggestion: None,
            };
            match runtime_workflows::timeout_runtime_call(
                &runtime,
                "terminal_invite_lan_peer",
                "send_bootstrap_invitation",
                Duration::from_secs(5),
                || runtime.send_bootstrap_invitation(&peer_info, &code),
            )
            .await
            {
                Ok(Ok(())) => {
                    tracing::info!("Sent bootstrap invitation to {}", address);
                    Some(Ok(OpResponse::LanInvitationStatus {
                        authority_id: authority_id.to_string(),
                        address: address.clone(),
                        message: format!("Invitation sent to {address}"),
                    }))
                }
                Ok(Err(error)) => Some(Err(OpError::typed(
                    OpFailureCode::SendBootstrapInvitation,
                    format!("Sending the invitation to {address} failed: {error}"),
                ))),
                Err(error) => Some(Err(OpError::typed(
                    OpFailureCode::SendBootstrapInvitation,
                    format!("Sending the invitation to {address} failed: {error}"),
                ))),
            }
        }

        _ => None,
    }
}
