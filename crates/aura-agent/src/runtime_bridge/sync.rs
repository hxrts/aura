use super::{
    error_boundary::{bridge_internal, bridge_network, bridge_network_message, bridge_validation},
    harness_mode_enabled, harness_sync_backoff_ms, harness_sync_rounds, require_rendezvous_service,
    require_sync_service, service_unavailable, AgentRuntimeBridge,
};
use aura_app::runtime_bridge::{
    CeremonyProcessingCounts, CeremonyProcessingOutcome, ReachabilityRefreshOutcome, SyncStatus,
};
use aura_app::IntentError;
use aura_core::effects::{PhysicalTimeEffects, TransportEffects};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::{DeviceId, EffectContext};

const RUNTIME_BRIDGE_SYNC_STATUS_QUERY_CAPABILITY: &str = "runtime_bridge_sync_status_query";
const RUNTIME_BRIDGE_SYNC_PEER_ONLINE_QUERY_CAPABILITY: &str =
    "runtime_bridge_sync_peer_online_query";
const RUNTIME_BRIDGE_SYNC_PEER_QUERY_CAPABILITY: &str = "runtime_bridge_sync_peer_query";
const RUNTIME_BRIDGE_SYNC_TRIGGER_CAPABILITY: &str = "runtime_bridge_sync_trigger";
const RUNTIME_BRIDGE_SYNC_CEREMONY_PROCESSING_CAPABILITY: &str =
    "runtime_bridge_sync_ceremony_processing";
const RUNTIME_BRIDGE_SYNC_WITH_PEER_CAPABILITY: &str = "runtime_bridge_sync_with_peer";
const RUNTIME_BRIDGE_SYNC_PEER_CHANNEL_CAPABILITY: &str = "runtime_bridge_sync_peer_channel";

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "runtime_bridge_sync_status_query",
    family = "runtime_helper"
)]
pub(super) async fn get_sync_status(
    bridge: &AgentRuntimeBridge,
) -> Result<SyncStatus, IntentError> {
    let _ = RUNTIME_BRIDGE_SYNC_STATUS_QUERY_CAPABILITY;
    let sync = require_sync_service(bridge)?;

    let effects = bridge.agent.runtime().effects();
    let transport_stats = effects.get_transport_stats().await;

    let health = sync.sync_service_health().await;
    let is_running = sync.is_running().await;
    let active_sessions = health.as_ref().map(|h| h.active_sessions).unwrap_or(0);
    let last_sync_ms = health.and_then(|h| h.last_sync);

    Ok(SyncStatus {
        is_running,
        connected_peers: (transport_stats.active_channels as usize).max(active_sessions as usize),
        last_sync_ms,
        pending_facts: 0,
        active_sessions: active_sessions as usize,
    })
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "runtime_bridge_sync_peer_online_query",
    family = "runtime_helper"
)]
pub(super) async fn is_peer_online(bridge: &AgentRuntimeBridge, peer: AuthorityId) -> bool {
    let _ = RUNTIME_BRIDGE_SYNC_PEER_ONLINE_QUERY_CAPABILITY;
    let effects = bridge.agent.runtime().effects();
    let context = EffectContext::with_authority(bridge.agent.authority_id()).context_id();

    if effects.is_channel_established(context, peer).await {
        return true;
    }

    if let Some(rendezvous) = bridge.agent.runtime().rendezvous() {
        if rendezvous.get_descriptor(context, peer).await.is_some() {
            return true;
        }
    }

    false
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "runtime_bridge_sync_peer_query",
    family = "runtime_helper"
)]
pub(super) async fn get_sync_peers(
    bridge: &AgentRuntimeBridge,
) -> Result<Vec<DeviceId>, IntentError> {
    let _ = RUNTIME_BRIDGE_SYNC_PEER_QUERY_CAPABILITY;
    let sync = require_sync_service(bridge)?;
    Ok(sync.peers().await)
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "runtime_bridge_sync_trigger",
    family = "runtime_helper"
)]
pub(super) async fn trigger_sync(bridge: &AgentRuntimeBridge) -> Result<(), IntentError> {
    let _ = RUNTIME_BRIDGE_SYNC_TRIGGER_CAPABILITY;
    let sync = require_sync_service(bridge)?;

    let effects = bridge.agent.runtime().effects();
    let rounds = if harness_mode_enabled() {
        harness_sync_rounds()
    } else {
        1
    };
    let backoff_ms = harness_sync_backoff_ms();
    let mut last_sync_error: Option<IntentError> = None;

    for round in 0..rounds {
        if harness_mode_enabled() {
            let _ = super::rendezvous::trigger_discovery(bridge).await;
        }

        bridge.seed_sync_peers_from_rendezvous().await;

        let authority_peers: Vec<AuthorityId> =
            if let Some(rendezvous) = bridge.agent.runtime().rendezvous() {
                let mut peers = rendezvous.list_cached_peers().await;
                if peers.is_empty() {
                    peers = rendezvous
                        .list_lan_discovered_peers()
                        .await
                        .into_iter()
                        .map(|peer| peer.authority_id)
                        .collect();
                }
                peers.sort();
                peers.dedup();
                peers
            } else {
                Vec::new()
            };
        let peers = sync.peers().await;

        if peers.is_empty() && authority_peers.is_empty() {
            tracing::debug!(
                "trigger_sync skipped because no sync or authority peers are available"
            );
            return Ok(());
        }

        let sync_result = if peers.is_empty() {
            Ok(())
        } else {
            sync.sync_with_peers(&effects, peers)
                .await
                .map_err(|e| bridge_internal("Sync failed", e))
        };

        if !authority_peers.is_empty() {
            tracing::debug!(
                peer_count = authority_peers.len(),
                "skipping removed direct LAN relational fact pull; authenticated sync services own ingress"
            );
        }

        match sync_result {
            Ok(()) => {
                if last_sync_error.is_none() {
                    return Ok(());
                }
            }
            Err(error) => last_sync_error = Some(error),
        }

        if round + 1 < rounds {
            let effects = bridge.agent.runtime().effects();
            let _ = effects.sleep_ms(backoff_ms).await;
        }
    }

    match last_sync_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "runtime_bridge_sync_ceremony_processing",
    family = "runtime_helper"
)]
pub(super) async fn process_ceremony_messages(
    bridge: &AgentRuntimeBridge,
) -> Result<CeremonyProcessingOutcome, IntentError> {
    let _ = RUNTIME_BRIDGE_SYNC_CEREMONY_PROCESSING_CAPABILITY;
    let invitation_handler = crate::handlers::invitation::InvitationHandler::new(
        crate::core::AuthorityContext::new_with_device(
            bridge.agent.authority_id(),
            bridge.agent.runtime().device_id(),
        ),
    )
    .map_err(|e| bridge_internal("Create invitation handler for inbox processing failed", e))?;
    let processed_contact_messages = invitation_handler
        .process_contact_invitation_acceptances(bridge.agent.runtime().effects())
        .await
        .map_err(|e| bridge_internal("Process contact/chat envelopes failed", e))?;
    let processed_handshakes = if let Some(rendezvous_manager) = bridge.agent.runtime().rendezvous()
    {
        let authority = bridge.agent.context().clone();
        let handler = crate::handlers::rendezvous::RendezvousHandler::new(authority)
            .map_err(|e| {
                bridge_internal(
                    "Create rendezvous handler for handshake processing failed",
                    e,
                )
            })?
            .with_rendezvous_manager((*rendezvous_manager).clone());
        handler
            .process_handshake_envelopes(bridge.agent.runtime().effects())
            .await
            .map_err(|e| bridge_internal("Process rendezvous handshakes failed", e))?
    } else {
        0
    };

    let counts = CeremonyProcessingCounts {
        acceptances: 0,
        completions: 0,
        contact_messages: processed_contact_messages,
        handshakes: processed_handshakes,
    };

    if counts.total() == 0 {
        return Ok(CeremonyProcessingOutcome::NoProgress);
    }

    let reachability_refresh = match bridge
        .refresh_reachability_after_ceremony_processing()
        .await
    {
        Ok(()) => ReachabilityRefreshOutcome::Refreshed,
        Err(error) => ReachabilityRefreshOutcome::Degraded {
            reason: error.to_string(),
        },
    };

    Ok(CeremonyProcessingOutcome::Processed {
        counts,
        reachability_refresh,
    })
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "runtime_bridge_sync_with_peer",
    family = "runtime_helper"
)]
pub(super) async fn sync_with_peer(
    bridge: &AgentRuntimeBridge,
    peer_id: &str,
) -> Result<(), IntentError> {
    let _ = RUNTIME_BRIDGE_SYNC_WITH_PEER_CAPABILITY;
    let sync = require_sync_service(bridge)?;

    let device_id: DeviceId = peer_id
        .parse()
        .map_err(|e| bridge_validation("Invalid peer ID", e))?;
    let effects = bridge.agent.runtime().effects();
    sync.sync_with_peers(&effects, vec![device_id])
        .await
        .map_err(|e| bridge_internal("Sync failed", e))
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "runtime_bridge_sync_peer_channel",
    family = "runtime_helper"
)]
pub(super) async fn ensure_peer_channel(
    bridge: &AgentRuntimeBridge,
    context: ContextId,
    peer: AuthorityId,
) -> Result<(), IntentError> {
    let _ = RUNTIME_BRIDGE_SYNC_PEER_CHANNEL_CAPABILITY;
    let effects = bridge.agent.runtime().effects();
    let rendezvous_manager = require_rendezvous_service(bridge)
        .map_err(|_| service_unavailable("rendezvous_manager"))?;

    let authority = bridge.agent.context().clone();
    let handler = crate::handlers::rendezvous::RendezvousHandler::new(authority)
        .map_err(|e| bridge_internal("Create rendezvous handler for peer channel setup failed", e))?
        .with_rendezvous_manager((*rendezvous_manager).clone());

    let rounds = if harness_mode_enabled() {
        harness_sync_rounds()
    } else {
        1
    };
    let backoff_ms = if harness_mode_enabled() {
        harness_sync_backoff_ms()
    } else {
        0
    };

    if effects.is_channel_established(context, peer).await {
        bridge.seed_sync_peers_from_rendezvous().await;
        bridge.sync_seeded_peers().await?;
        return Ok(());
    }

    let _ = super::rendezvous::trigger_discovery(bridge).await;
    bridge.seed_sync_peers_from_rendezvous().await;
    let _ = bridge.sync_seeded_peers().await;
    let _ = process_ceremony_messages(bridge).await;

    let result = handler
        .initiate_channel(&effects, context, peer)
        .await
        .map_err(|e| {
            bridge_network(
                "Initiate peer channel failed",
                format!("{peer} in {context}: {e}"),
            )
        })?;

    if !result.success {
        return Err(bridge_network_message(result.error.unwrap_or_else(|| {
            "peer channel initiation was denied".to_string()
        })));
    }

    for round in 0..rounds {
        if effects.is_channel_established(context, peer).await {
            bridge.seed_sync_peers_from_rendezvous().await;
            bridge.sync_seeded_peers().await?;
            return Ok(());
        }

        if harness_mode_enabled() {
            let _ = super::rendezvous::trigger_discovery(bridge).await;
        }
        bridge.seed_sync_peers_from_rendezvous().await;
        bridge.sync_seeded_peers().await?;
        process_ceremony_messages(bridge).await?;

        if round + 1 < rounds && backoff_ms > 0 {
            let effects = bridge.agent.runtime().effects();
            let _ = effects.sleep_ms(backoff_ms).await;
        }
    }

    Err(bridge_network_message(format!(
        "peer channel for {peer} in {context} did not establish after bounded convergence"
    )))
}
