use super::{service_unavailable, AgentRuntimeBridge};
use aura_app::runtime_bridge::{LanPeerInfo, RendezvousStatus};
use aura_app::IntentError;
use aura_core::identifiers::AuthorityId;

pub(super) async fn get_discovered_peers(bridge: &AgentRuntimeBridge) -> Vec<AuthorityId> {
    if let Some(rendezvous) = bridge.agent.runtime().rendezvous() {
        rendezvous.list_cached_peers().await
    } else {
        Vec::new()
    }
}

pub(super) async fn get_rendezvous_status(bridge: &AgentRuntimeBridge) -> RendezvousStatus {
    if let Some(rendezvous) = bridge.agent.runtime().rendezvous() {
        RendezvousStatus {
            is_running: rendezvous.is_running().await,
            cached_peers: rendezvous.list_cached_peers().await.len(),
        }
    } else {
        RendezvousStatus::default()
    }
}

pub(super) async fn trigger_discovery(bridge: &AgentRuntimeBridge) -> Result<(), IntentError> {
    if let Some(rendezvous) = bridge.agent.runtime().rendezvous() {
        rendezvous
            .trigger_discovery()
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to trigger discovery: {}", e)))
    } else {
        Err(service_unavailable("rendezvous_service"))
    }
}

pub(super) async fn get_lan_peers(bridge: &AgentRuntimeBridge) -> Vec<LanPeerInfo> {
    if let Some(rendezvous) = bridge.agent.runtime().rendezvous() {
        rendezvous
            .list_lan_discovered_peers()
            .await
            .into_iter()
            .map(|peer| LanPeerInfo {
                authority_id: peer.authority_id,
                address: peer.source_addr.clone(),
                discovered_at_ms: peer.discovered_at_ms,
                nickname_suggestion: peer.descriptor.nickname_suggestion.clone(),
            })
            .collect()
    } else {
        Vec::new()
    }
}

pub(super) async fn send_lan_invitation(
    bridge: &AgentRuntimeBridge,
    peer: &LanPeerInfo,
    invitation_code: &str,
) -> Result<(), IntentError> {
    if let Some(rendezvous) = bridge.agent.runtime().rendezvous() {
        rendezvous
            .send_lan_invitation(&peer.authority_id, &peer.address, invitation_code)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to send LAN invitation: {}", e))
            })
    } else {
        Err(service_unavailable("rendezvous_service"))
    }
}
