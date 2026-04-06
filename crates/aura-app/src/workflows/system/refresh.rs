//! System refresh and health-check workflows.

use crate::runtime_bridge::SyncStatus as RuntimeSyncStatus;
use crate::signal_defs::{
    ConnectionStatus, NetworkStatus, CHAT_SIGNAL, CHAT_SIGNAL_NAME, CONNECTION_STATUS_SIGNAL,
    CONNECTION_STATUS_SIGNAL_NAME, CONTACTS_SIGNAL, CONTACTS_SIGNAL_NAME, NETWORK_STATUS_SIGNAL,
    NETWORK_STATUS_SIGNAL_NAME, TRANSPORT_PEERS_SIGNAL, TRANSPORT_PEERS_SIGNAL_NAME,
};
use crate::workflows::observed_snapshot::observed_contacts_snapshot;
use crate::workflows::runtime::{timeout_runtime_call, workflow_best_effort};
use crate::workflows::signals::{emit_signal, emit_signal_if_changed, read_signal};
use crate::AppCore;
use async_lock::RwLock;
use aura_core::AuraError;
use std::sync::Arc;
use std::time::Duration;

pub(super) const SYSTEM_RUNTIME_TIMEOUT: Duration = Duration::from_millis(5_000);

fn compute_network_status(
    has_runtime: bool,
    online_contacts: usize,
    sync_status: &RuntimeSyncStatus,
) -> NetworkStatus {
    if !has_runtime {
        return NetworkStatus::Disconnected;
    }
    if online_contacts == 0 {
        return NetworkStatus::NoPeers;
    }
    if sync_status.active_sessions > 0 {
        return NetworkStatus::Syncing;
    }
    if let Some(last_sync_ms) = sync_status.last_sync_ms {
        return NetworkStatus::Synced { last_sync_ms };
    }
    NetworkStatus::Syncing
}

pub async fn ping(_app_core: &Arc<RwLock<AppCore>>) -> Result<(), AuraError> {
    Ok(())
}

pub async fn refresh_account(app_core: &Arc<RwLock<AppCore>>) -> Result<(), AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime().cloned()
    };

    let mut best_effort = workflow_best_effort();

    #[cfg(feature = "signals")]
    {
        let _ = best_effort
            .capture(emit_chat_snapshot_signal(app_core))
            .await;
    }

    let _ = crate::workflows::query::list_contacts(app_core).await;
    let _ = crate::workflows::invitation::list_invitations(app_core).await;
    let _ = best_effort
        .capture(crate::workflows::invitation::refresh_authoritative_invitation_readiness(app_core))
        .await;
    let _ = best_effort
        .capture(
            crate::workflows::invitation::refresh_authoritative_contact_link_readiness(app_core),
        )
        .await;
    let _ = best_effort
        .capture(crate::workflows::settings::refresh_settings_from_runtime(
            app_core,
        ))
        .await;

    #[cfg(feature = "signals")]
    {
        let _ = best_effort
            .capture(crate::workflows::recovery::get_recovery_status(app_core))
            .await;
    }

    if let Some(runtime) = runtime {
        let _ = best_effort
            .capture(async {
                timeout_runtime_call(
                    &runtime,
                    "refresh_account",
                    "trigger_discovery",
                    SYSTEM_RUNTIME_TIMEOUT,
                    || runtime.trigger_discovery(),
                )
                .await?
                .map_err(|error| AuraError::agent(error.to_string()))
            })
            .await;
        let _ = best_effort
            .capture(async {
                timeout_runtime_call(
                    &runtime,
                    "refresh_account",
                    "trigger_sync",
                    SYSTEM_RUNTIME_TIMEOUT,
                    || runtime.trigger_sync(),
                )
                .await?
                .map_err(|error| AuraError::agent(error.to_string()))
            })
            .await;
    }

    #[cfg(feature = "signals")]
    {
        let _ = best_effort
            .capture(emit_chat_snapshot_signal(app_core))
            .await;
        let _ = best_effort
            .capture(
                crate::workflows::messaging::refresh_authoritative_channel_membership_readiness(
                    app_core,
                ),
            )
            .await;
    }

    let _ = best_effort
        .capture(crate::workflows::network::refresh_discovered_peers(
            app_core,
        ))
        .await;

    #[cfg(feature = "signals")]
    {
        let _ = best_effort
            .capture(
                crate::workflows::messaging::refresh_authoritative_recipient_resolution_readiness(
                    app_core,
                ),
            )
            .await;
    }

    let _ = best_effort
        .capture(refresh_connection_status_from_contacts(app_core))
        .await;

    best_effort.finish()
}

pub(super) async fn emit_chat_snapshot_signal(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let (runtime_present, snapshot_chat) = {
        let core = app_core.read().await;
        (core.runtime().is_some(), core.snapshot().chat)
    };
    let chat = if runtime_present {
        read_signal(app_core, &*CHAT_SIGNAL, CHAT_SIGNAL_NAME)
            .await
            .unwrap_or(snapshot_chat)
    } else {
        snapshot_chat
    };
    emit_signal(app_core, &*CHAT_SIGNAL, chat, CHAT_SIGNAL_NAME).await
}

pub(super) async fn publish_connection_status_bundle(
    app_core: &Arc<RwLock<AppCore>>,
    contacts_state: crate::views::ContactsState,
    connection: ConnectionStatus,
    network_status: Option<NetworkStatus>,
    transport_peers: Option<usize>,
) -> Result<(), AuraError> {
    let mut best_effort = workflow_best_effort();
    let _ = best_effort
        .capture(emit_signal_if_changed(
            app_core,
            &*CONTACTS_SIGNAL,
            contacts_state,
            CONTACTS_SIGNAL_NAME,
        ))
        .await;
    let _ = best_effort
        .capture(emit_signal_if_changed(
            app_core,
            &*CONNECTION_STATUS_SIGNAL,
            connection,
            CONNECTION_STATUS_SIGNAL_NAME,
        ))
        .await;
    if let Some(network_status) = network_status {
        let _ = best_effort
            .capture(emit_signal_if_changed(
                app_core,
                &*NETWORK_STATUS_SIGNAL,
                network_status,
                NETWORK_STATUS_SIGNAL_NAME,
            ))
            .await;
    }
    if let Some(transport_peers) = transport_peers {
        let _ = best_effort
            .capture(emit_signal_if_changed(
                app_core,
                &*TRANSPORT_PEERS_SIGNAL,
                transport_peers,
                TRANSPORT_PEERS_SIGNAL_NAME,
            ))
            .await;
    }
    best_effort.finish()
}

pub async fn refresh_connection_status_from_contacts(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime().cloned()
    };
    let mut contacts_state = observed_contacts_snapshot(app_core).await;
    if let Ok(state) = read_signal(app_core, &*CONTACTS_SIGNAL, CONTACTS_SIGNAL_NAME).await {
        contacts_state = state;
    }

    if let Some(runtime) = runtime {
        let mut online_contacts = 0usize;
        for contact in contacts_state.all_contacts_mut() {
            contact.is_online = timeout_runtime_call(
                &runtime,
                "refresh_connection_status_from_contacts",
                "is_peer_online",
                SYSTEM_RUNTIME_TIMEOUT,
                || runtime.is_peer_online(contact.id),
            )
            .await
            .unwrap_or(false);
            if contact.is_online {
                online_contacts += 1;
            }
        }

        let sync_status = {
            let core = app_core.read().await;
            core.sync_status().await.map_err(|error| {
                AuraError::from(crate::workflows::error::runtime_call(
                    "get sync status",
                    error,
                ))
            })?
        };
        if online_contacts == 0 && !contacts_state.is_empty() {
            if let Some(sync_status) = sync_status.as_ref() {
                if sync_status.connected_peers > 0 {
                    let inferred_online =
                        std::cmp::min(contacts_state.contact_count(), sync_status.connected_peers);
                    for (index, contact) in contacts_state.all_contacts_mut().enumerate() {
                        contact.is_online = index < inferred_online;
                    }
                    online_contacts = inferred_online;
                }
            }
        }

        let connection = if online_contacts > 0 {
            ConnectionStatus::Online {
                peer_count: online_contacts,
            }
        } else {
            ConnectionStatus::Offline
        };
        let network_status = sync_status
            .as_ref()
            .map(|status| compute_network_status(true, online_contacts, status));

        publish_connection_status_bundle(
            app_core,
            contacts_state,
            connection,
            network_status,
            sync_status.as_ref().map(|status| status.connected_peers),
        )
        .await?;
    } else {
        publish_connection_status_bundle(
            app_core,
            contacts_state,
            ConnectionStatus::Offline,
            Some(NetworkStatus::Disconnected),
            Some(0),
        )
        .await?;
    }

    Ok(())
}

pub async fn is_available(app_core: &Arc<RwLock<AppCore>>) -> bool {
    app_core.try_read().is_some()
}
