use super::{service_unavailable, AgentRuntimeBridge};
use aura_app::runtime_bridge::{
    BootstrapCandidateInfo, BootstrapCandidateOrigin, DiscoveryTriggerOutcome, RendezvousStatus,
};
use aura_app::IntentError;
use aura_core::types::identifiers::AuthorityId;

pub(super) async fn get_discovered_peers(
    bridge: &AgentRuntimeBridge,
) -> Result<Vec<AuthorityId>, IntentError> {
    if let Some(rendezvous) = bridge.agent.runtime().rendezvous() {
        Ok(rendezvous.list_cached_peers().await)
    } else {
        Err(service_unavailable("rendezvous_service"))
    }
}

pub(super) async fn get_rendezvous_status(
    bridge: &AgentRuntimeBridge,
) -> Result<RendezvousStatus, IntentError> {
    if let Some(rendezvous) = bridge.agent.runtime().rendezvous() {
        Ok(RendezvousStatus {
            is_running: rendezvous.is_running().await,
            cached_peers: rendezvous.list_cached_peers().await.len(),
        })
    } else {
        Err(service_unavailable("rendezvous_service"))
    }
}

pub(super) async fn trigger_discovery(
    bridge: &AgentRuntimeBridge,
) -> Result<DiscoveryTriggerOutcome, IntentError> {
    if let Some(rendezvous) = bridge.agent.runtime().rendezvous() {
        rendezvous
            .trigger_discovery()
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to trigger discovery: {}", e)))
    } else {
        Err(service_unavailable("rendezvous_service"))
    }
}

pub(super) async fn get_bootstrap_candidates(
    bridge: &AgentRuntimeBridge,
) -> Result<Vec<BootstrapCandidateInfo>, IntentError> {
    if let Some(rendezvous) = bridge.agent.runtime().rendezvous() {
        Ok(rendezvous
            .list_lan_discovered_peers()
            .await
            .into_iter()
            .map(|peer| BootstrapCandidateInfo {
                authority_id: peer.authority_id,
                origin: BootstrapCandidateOrigin::Lan,
                address: peer.source_addr.clone(),
                discovered_at_ms: peer.discovered_at_ms,
                nickname_suggestion: peer.descriptor.nickname_suggestion.clone(),
            })
            .collect())
    } else {
        Err(service_unavailable("rendezvous_service"))
    }
}

pub(super) async fn send_bootstrap_invitation(
    bridge: &AgentRuntimeBridge,
    peer: &BootstrapCandidateInfo,
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
