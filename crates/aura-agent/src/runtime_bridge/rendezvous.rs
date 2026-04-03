use super::{require_rendezvous_service, service_unavailable, AgentRuntimeBridge};
use crate::runtime::services::bootstrap_broker::endpoint_is_loopback;
use crate::runtime::system::register_bootstrap_candidate_with;
use aura_app::runtime_bridge::{
    BootstrapCandidateInfo, BootstrapCandidateOrigin, DiscoveryTriggerOutcome, RendezvousStatus,
};
use aura_app::IntentError;
use aura_core::types::identifiers::AuthorityId;

fn broker_origin(bridge: &AgentRuntimeBridge) -> BootstrapCandidateOrigin {
    let Some(rendezvous) = bridge.agent.runtime().rendezvous() else {
        return BootstrapCandidateOrigin::LocalBroker;
    };
    let broker_config = &rendezvous.config().bootstrap_broker;
    let endpoint = broker_config
        .base_url
        .as_deref()
        .or(broker_config.bind_addr.as_deref());
    match endpoint {
        Some(raw) if endpoint_is_loopback(raw) => BootstrapCandidateOrigin::LocalBroker,
        Some(_) => BootstrapCandidateOrigin::LanBroker,
        None => BootstrapCandidateOrigin::LocalBroker,
    }
}

pub(super) async fn get_discovered_peers(
    bridge: &AgentRuntimeBridge,
) -> Result<Vec<AuthorityId>, IntentError> {
    let rendezvous = require_rendezvous_service(bridge)?;
    Ok(rendezvous.list_cached_peers().await)
}

pub(super) async fn get_rendezvous_status(
    bridge: &AgentRuntimeBridge,
) -> Result<RendezvousStatus, IntentError> {
    let rendezvous = require_rendezvous_service(bridge)?;
    Ok(RendezvousStatus {
        is_running: rendezvous.is_running().await,
        cached_peers: rendezvous.list_cached_peers().await.len(),
    })
}

pub(super) async fn trigger_discovery(
    bridge: &AgentRuntimeBridge,
) -> Result<DiscoveryTriggerOutcome, IntentError> {
    let rendezvous = require_rendezvous_service(bridge)?;
    rendezvous
        .trigger_discovery()
        .await
        .map_err(|e| IntentError::internal_error(format!("Failed to trigger discovery: {}", e)))
}

pub(super) async fn get_bootstrap_candidates(
    bridge: &AgentRuntimeBridge,
) -> Result<Vec<BootstrapCandidateInfo>, IntentError> {
    let rendezvous = require_rendezvous_service(bridge)?;
    let mut candidates: Vec<BootstrapCandidateInfo> = rendezvous
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
        .collect();
    let broker_candidates = rendezvous
        .list_bootstrap_broker_candidates()
        .await
        .map_err(|error| {
            IntentError::internal_error(format!(
                "Failed to list bootstrap broker candidates: {error}"
            ))
        })?;
    candidates.extend(broker_candidates.into_iter().filter_map(|candidate| {
        Some(BootstrapCandidateInfo {
            authority_id: candidate.authority_id()?,
            origin: broker_origin(bridge),
            address: candidate.address,
            discovered_at_ms: candidate.discovered_at_ms,
            nickname_suggestion: candidate.nickname_suggestion,
        })
    }));
    Ok(candidates)
}

pub(super) async fn send_bootstrap_invitation(
    bridge: &AgentRuntimeBridge,
    peer: &BootstrapCandidateInfo,
    invitation_code: &str,
) -> Result<(), IntentError> {
    let rendezvous = require_rendezvous_service(bridge)?;
    match peer.origin {
        BootstrapCandidateOrigin::Lan => rendezvous
            .send_lan_invitation(&peer.authority_id, &peer.address, invitation_code)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to send LAN invitation: {}", e))
            }),
        BootstrapCandidateOrigin::LocalBroker | BootstrapCandidateOrigin::LanBroker => rendezvous
            .send_bootstrap_broker_invitation(peer.authority_id, invitation_code)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!(
                    "Failed to send bootstrap broker invitation: {}",
                    e
                ))
            }),
    }
}

pub(super) async fn refresh_bootstrap_candidate_registration(
    bridge: &AgentRuntimeBridge,
) -> Result<(), IntentError> {
    let Some(rendezvous) = bridge.agent.runtime().rendezvous() else {
        return Err(service_unavailable("rendezvous_service"));
    };
    let Some(lan_transport) = bridge.agent.runtime().effects().lan_transport() else {
        return Err(service_unavailable("lan_transport_service"));
    };

    register_bootstrap_candidate_with(rendezvous, lan_transport.as_ref())
        .await
        .map_err(|error| {
            IntentError::internal_error(format!(
                "Failed to refresh bootstrap candidate registration: {error}"
            ))
        })
}
