#![allow(missing_docs)]

use super::*;

async fn propagate_contact_acceptance_followup(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: AuthorityId,
) -> Result<(), AuraError> {
    let Ok(runtime) = require_runtime(app_core).await else {
        return Ok(());
    };
    let contact_peer_id = contact_id.to_string();
    let _ = wait_for_contact_link(app_core, &runtime, contact_id).await;
    for _ in 0..CONTACT_ACCEPT_PROPAGATION_ATTEMPTS {
        trigger_runtime_discovery_with_timeout(&runtime).await;
        let _ = crate::workflows::network::refresh_discovered_peers(app_core).await;
        let _ = timeout_runtime_call(
            &runtime,
            "accept_contact_invitation",
            "process_ceremony_messages",
            INVITATION_RUNTIME_OPERATION_TIMEOUT,
            || runtime.process_ceremony_messages(),
        )
        .await;
        let _ = timeout_runtime_call(
            &runtime,
            "accept_contact_invitation",
            "sync_with_peer",
            INVITATION_RUNTIME_OPERATION_TIMEOUT,
            || runtime.sync_with_peer(&contact_peer_id),
        )
        .await;
        let _ = timeout_runtime_call(
            &runtime,
            "accept_contact_invitation",
            "trigger_sync",
            INVITATION_RUNTIME_OPERATION_TIMEOUT,
            || runtime.trigger_sync(),
        )
        .await;
        converge_runtime(&runtime).await;
        let _ = crate::workflows::system::refresh_account(app_core).await;
        let _ = crate::workflows::network::refresh_discovered_peers(app_core).await;
        let _ = refresh_authoritative_contact_link_readiness(app_core).await;
        let linked = contacts_signal_snapshot(app_core)
            .await
            .map(|contacts| {
                contacts
                    .all_contacts()
                    .any(|contact| contact.id == contact_id)
            })
            .unwrap_or(false);
        let peer_online = timeout_runtime_call(
            &runtime,
            "accept_contact_invitation",
            "is_peer_online",
            INVITATION_RUNTIME_OPERATION_TIMEOUT,
            || runtime.is_peer_online(contact_id),
        )
        .await
        .unwrap_or(false);
        if linked && peer_online {
            break;
        }
    }
    Ok(())
}

pub async fn run_post_contact_accept_followups(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: AuthorityId,
) {
    let mut best_effort = workflow_best_effort();
    let _ = best_effort
        .capture(propagate_contact_acceptance_followup(app_core, contact_id))
        .await;
    let _ = best_effort.finish();
}
