use super::*;

/// Best-effort channel connectivity warming.
pub(crate) async fn warm_channel_connectivity(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
    channel: AuthoritativeChannelRef,
) -> bool {
    let context_id = channel.context_id();
    let policy =
        match workflow_retry_policy(8, Duration::from_millis(150), Duration::from_millis(750)) {
            Ok(policy) => policy,
            Err(_) => return false,
        };
    let result = execute_with_runtime_retry_budget(runtime, &policy, |_attempt| async {
        let recipients =
            authoritative_recipient_peers_for_channel(runtime, channel, runtime.authority_id())
                .await?;
        let mut any_peer_ready = recipients.is_empty();
        for peer in recipients {
            if timeout_runtime_call(
                runtime,
                "warm_channel_connectivity",
                "ensure_peer_channel",
                MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                || runtime.ensure_peer_channel(context_id, peer),
            )
            .await
            .is_ok()
            {
                any_peer_ready = true;
            }
        }
        let _ =
            refresh_authoritative_delivery_readiness_for_channel(app_core, runtime, channel).await;
        converge_runtime(runtime).await;
        let _ = crate::workflows::system::refresh_account(app_core).await;
        if any_peer_ready
            || ensure_runtime_peer_connectivity(runtime, "warm_channel_connectivity")
                .await
                .is_ok()
        {
            Ok(())
        } else {
            Err(AuraError::from(
                super::super::error::WorkflowError::Precondition(
                    "channel peer connectivity not yet warmed",
                ),
            ))
        }
    })
    .await;
    let warmed = result.is_ok();
    if !warmed {
        #[cfg(feature = "instrumented")]
        let channel_id = channel.channel_id();
        #[cfg(feature = "instrumented")]
        tracing::warn!(
            channel_id = %channel_id,
            context_id = %context_id,
            "channel connectivity warming exhausted retries"
        );
    }
    warmed
}

async fn propagate_channel_invitation_to_peer(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
    authoritative_channel: AuthoritativeChannelRef,
    receiver: AuthorityId,
) {
    let authority_context = authority_default_relational_context(receiver);
    for _ in 0..CHANNEL_INVITE_POST_CREATE_PROPAGATION_ATTEMPTS {
        let receiver_peer_id = receiver.to_string();
        let _ = timeout_runtime_call(
            runtime,
            "invite_authority_to_channel",
            "trigger_discovery",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.trigger_discovery(),
        )
        .await;
        let _ = crate::workflows::network::refresh_discovered_peers(app_core).await;
        let _ = timeout_runtime_call(
            runtime,
            "invite_authority_to_channel",
            "ensure_authority_peer_channel",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.ensure_peer_channel(authority_context, receiver),
        )
        .await;
        let _ = timeout_runtime_call(
            runtime,
            "invite_authority_to_channel",
            "ensure_peer_channel",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.ensure_peer_channel(authoritative_channel.context_id(), receiver),
        )
        .await;
        let _ = timeout_runtime_call(
            runtime,
            "invite_authority_to_channel",
            "process_ceremony_messages",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.process_ceremony_messages(),
        )
        .await;
        let _ = timeout_runtime_call(
            runtime,
            "invite_authority_to_channel",
            "sync_with_peer",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.sync_with_peer(&receiver_peer_id),
        )
        .await;
        let _ = timeout_runtime_call(
            runtime,
            "invite_authority_to_channel",
            "trigger_sync",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.trigger_sync(),
        )
        .await;
        converge_runtime(runtime).await;
        let _ = crate::workflows::system::refresh_account(app_core).await;
        let _ = crate::workflows::network::refresh_discovered_peers(app_core).await;
    }
}

pub async fn run_post_channel_invite_followups(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    authoritative_channel: AuthoritativeChannelRef,
) {
    let mut best_effort = workflow_best_effort();
    let _ = best_effort
        .capture(async {
            let runtime = require_runtime(app_core).await?;
            propagate_channel_invitation_to_peer(
                app_core,
                &runtime,
                authoritative_channel,
                receiver,
            )
            .await;
            let _ = crate::workflows::system::refresh_account(app_core).await;
            Ok::<(), AuraError>(())
        })
        .await;
    let _ = best_effort.finish();
}

async fn stabilize_authoritative_join_readiness(
    app_core: &Arc<RwLock<AppCore>>,
    authoritative_channel: AuthoritativeChannelRef,
    channel_name_hint: Option<&str>,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    let channel_id = authoritative_channel.channel_id();

    converge_runtime(&runtime).await;
    if let Err(_error) = ensure_runtime_peer_connectivity(&runtime, "join_channel_by_name").await {
        messaging_warn!(
            "Channel {} joined before connectivity fully warmed: {}",
            channel_id,
            _error
        );
    }

    let member_count = authoritative_recipient_peers_for_channel(
        &runtime,
        authoritative_channel,
        runtime.authority_id(),
    )
    .await
    .map(|recipients| (recipients.len() as u32).saturating_add(1))?;

    publish_authoritative_channel_membership_ready(
        app_core,
        channel_id,
        channel_name_hint,
        member_count,
    )
    .await?;
    refresh_authoritative_channel_membership_readiness(app_core).await?;
    refresh_authoritative_recipient_resolution_readiness(app_core).await
}

pub(in crate::workflows) async fn post_terminal_join_followups(
    app_core: &Arc<RwLock<AppCore>>,
    authoritative_channel: AuthoritativeChannelRef,
    channel_name_hint: Option<&str>,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    let channel_id = authoritative_channel.channel_id();
    let context_id = authoritative_channel.context_id();

    let _ = timeout_runtime_call(
        &runtime,
        "post_terminal_join_followups",
        "resend_channel_invitation_acceptance_notifications",
        MESSAGING_RUNTIME_OPERATION_TIMEOUT,
        || runtime.resend_channel_invitation_acceptance_notifications(context_id, channel_id),
    )
    .await
    .map_err(|error| {
        AuraError::from(super::super::error::runtime_call(
            "resend channel invitation acceptance notifications",
            error,
        ))
    })?
    .map_err(|error| {
        AuraError::from(super::super::error::runtime_call(
            "resend channel invitation acceptance notifications",
            error,
        ))
    });

    let _ = warm_channel_connectivity(app_core, &runtime, authoritative_channel).await;
    stabilize_authoritative_join_readiness(app_core, authoritative_channel, channel_name_hint).await
}
