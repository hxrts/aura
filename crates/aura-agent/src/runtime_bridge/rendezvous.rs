use super::error_boundary::bridge_internal;
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

pub(super) async fn get_selected_peer_paths(
    bridge: &AgentRuntimeBridge,
) -> Result<Vec<aura_core::service::SelectedPeerPath>, IntentError> {
    let rendezvous = require_rendezvous_service(bridge)?;
    Ok(rendezvous.list_selected_peer_paths().await)
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
        .map_err(|e| bridge_internal("Trigger discovery failed", e))
}

pub(super) async fn get_bootstrap_candidates(
    bridge: &AgentRuntimeBridge,
) -> Result<Vec<BootstrapCandidateInfo>, aura_app::runtime_bridge::RuntimeBridgeError> {
    let rendezvous = require_rendezvous_service(bridge)?;
    let mut candidates: Vec<BootstrapCandidateInfo> = rendezvous
        .list_lan_discovered_peers()
        .await
        .into_iter()
        .map(|peer| BootstrapCandidateInfo {
            authority_id: peer.authority_id,
            device_id: peer.descriptor.device_id,
            origin: BootstrapCandidateOrigin::Lan,
            address: peer.source_addr.clone(),
            discovered_at_ms: peer.discovered_at_ms,
            nickname_suggestion: peer.descriptor.nickname_suggestion.clone(),
        })
        .collect();
    let broker_candidates = rendezvous
        .list_bootstrap_broker_candidates(bridge.agent.runtime().device_id())
        .await
        .map_err(|error| {
            super::error_boundary::bridge_runtime_internal(
                "List bootstrap broker candidates failed",
                error,
            )
        })?;
    candidates.extend(broker_candidates.into_iter().filter_map(|candidate| {
        Some(BootstrapCandidateInfo {
            authority_id: candidate.authority_id()?,
            device_id: Some(candidate.device_id),
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
) -> Result<(), aura_app::runtime_bridge::RuntimeBridgeError> {
    let rendezvous = require_rendezvous_service(bridge)?;
    let observed = aura_invitation::shareable::ShareableInvitation::from_code(invitation_code)
        .map_err(|source| {
            bridge_internal(
                "Decode bootstrap invitation purpose",
                aura_core::AuraError::Invalid {
                    message: "invalid bootstrap invitation code".into(),
                    source: Some(std::sync::Arc::new(source)),
                },
            )
        })?;
    match peer.origin {
        BootstrapCandidateOrigin::Lan => rendezvous
            .send_lan_invitation(&peer.authority_id, &peer.address, invitation_code)
            .await
            .map_err(|e| {
                super::error_boundary::bridge_runtime_internal("Send LAN invitation failed", e)
            }),
        BootstrapCandidateOrigin::LocalBroker | BootstrapCandidateOrigin::LanBroker => rendezvous
            .send_bootstrap_broker_invitation(
                peer.authority_id,
                peer.device_id.ok_or_else(|| {
                    bridge_internal(
                        "Broker recipient device is required",
                        aura_core::AuraError::invalid("missing broker recipient device"),
                    )
                })?,
                observed.invitation_type.purpose(),
                invitation_code,
            )
            .await
            .map_err(|e| {
                super::error_boundary::bridge_runtime_internal(
                    "Send bootstrap broker invitation failed",
                    e,
                )
            }),
    }
}

pub(super) async fn refresh_bootstrap_candidate_registration(
    bridge: &AgentRuntimeBridge,
    original: &aura_core::TimeoutBudget,
) -> Result<(), aura_app::runtime_bridge::RuntimeBridgeError> {
    let Some(rendezvous) = bridge.agent.runtime().rendezvous() else {
        return Err(service_unavailable("rendezvous_service").into());
    };
    let Some(lan_transport) = bridge.agent.runtime().effects().lan_transport() else {
        return Err(service_unavailable("lan_transport_service").into());
    };

    let effects = bridge.agent.runtime().effects();
    aura_core::execute_with_timeout_budget(effects.as_ref(), original, || {
        register_bootstrap_candidate_with(rendezvous, lan_transport.as_ref(), effects.device_id())
    })
    .await
    .map_err(|error| {
        super::error_boundary::bridge_runtime_internal(
            "Refresh bootstrap candidate registration failed",
            error,
        )
    })
}
