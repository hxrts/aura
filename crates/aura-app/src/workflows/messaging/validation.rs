use super::*;
use crate::workflows::runtime::timeout_runtime_call_with_budget;
use std::time::Duration;

const VALIDATION_RUNTIME_TIMEOUT: Duration = Duration::from_millis(5_000);

pub(super) fn is_invitation_capability_missing(error: &AuraError) -> bool {
    error.to_string().contains("invitation:capability-missing")
}

#[aura_macros::authoritative_source(kind = "runtime")]
pub(super) async fn authoritative_home_moderation_status(
    app_core: &Arc<RwLock<AppCore>>,
    context_id: ContextId,
    channel_id: ChannelId,
    authority_id: AuthorityId,
    timestamp_ms: u64,
    parent: &TimeoutBudget,
) -> Result<crate::runtime_bridge::AuthoritativeModerationStatus, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime().cloned()
    }
    .ok_or_else(|| {
        AuraError::permission_denied("authoritative moderation status requires runtime")
    })?;

    timeout_runtime_call_with_budget(
        &runtime,
        parent,
        "authoritative_home_moderation_status",
        "moderation_status",
        VALIDATION_RUNTIME_TIMEOUT,
        || runtime.moderation_status(context_id, channel_id, authority_id, timestamp_ms),
    )
    .await?
    .map_err(|error| {
        crate::workflows::error::native_runtime_call("authoritative moderation status", error)
            .into()
    })
}

pub(super) fn intent_error_is_not_found(error: &(impl std::error::Error + 'static)) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = source {
        if let Some(native) = error.downcast_ref::<crate::runtime_bridge::RuntimeBridgeError>() {
            if matches!(
                native.kind(),
                crate::runtime_bridge::RuntimeBridgeErrorKind::NotFound
                    | crate::runtime_bridge::RuntimeBridgeErrorKind::ContextNotFound
            ) {
                return true;
            }
        }
        if matches!(
            error.downcast_ref::<IntentError>(),
            Some(IntentError::ContextNotFound { .. })
        ) || matches!(
            error.downcast_ref::<AuraError>(),
            Some(AuraError::NotFound { .. })
        ) {
            return true;
        }
        source = error.source();
    }
    false
}

pub(super) async fn enforce_home_moderation_for_sender(
    app_core: &Arc<RwLock<AppCore>>,
    context_id: ContextId,
    channel_id: ChannelId,
    sender_id: AuthorityId,
    timestamp_ms: u64,
    parent: &TimeoutBudget,
) -> Result<(), AuraError> {
    let status = authoritative_home_moderation_status(
        app_core,
        context_id,
        channel_id,
        sender_id,
        timestamp_ms,
        parent,
    )
    .await?;

    if status.is_banned {
        return Err(crate::workflows::moderation::ModerationDenial::Banned {
            context: context_id,
            channel: channel_id,
            authority: sender_id,
        }
        .into());
    }

    if status.is_muted {
        return Err(crate::workflows::moderation::ModerationDenial::Muted {
            context: context_id,
            channel: channel_id,
            authority: sender_id,
        }
        .into());
    }

    if status.roster_known && !status.is_member {
        return Err(crate::workflows::moderation::ModerationDenial::NotMember {
            context: context_id,
            channel: channel_id,
            authority: sender_id,
        }
        .into());
    }

    // Resolve the home the way inbound gating does: by the channel's home id
    // or by its context (a home channel's id need not equal the home id).
    let homes = crate::workflows::observed_projection::homes_signal_snapshot(app_core).await?;
    let denied = homes.iter().any(|(id, home)| {
        (*id == channel_id || home.context_id == Some(context_id))
            && !home.allows_access_capability(&sender_id, "send_message")
    });
    if denied {
        return Err(
            crate::workflows::moderation::ModerationDenial::AccessRestricted {
                context: context_id,
                channel: channel_id,
                authority: sender_id,
                capability: "sending messages",
            }
            .into(),
        );
    }

    Ok(())
}

pub(super) async fn enforce_home_join_allowed(
    app_core: &Arc<RwLock<AppCore>>,
    context_id: ContextId,
    channel_id: ChannelId,
    authority_id: AuthorityId,
    parent: &TimeoutBudget,
) -> Result<(), AuraError> {
    let timestamp_ms = crate::workflows::time::current_time_ms(app_core)
        .await
        .map_err(AuraError::from)?;
    let status = authoritative_home_moderation_status(
        app_core,
        context_id,
        channel_id,
        authority_id,
        timestamp_ms,
        parent,
    )
    .await?;
    if status.is_banned {
        return Err(crate::workflows::moderation::ModerationDenial::Banned {
            context: context_id,
            channel: channel_id,
            authority: authority_id,
        }
        .into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppConfig, AppCore};

    #[tokio::test]
    async fn real_owned_moderation_status_yields_typed_denials_and_bound_entities() {
        use crate::runtime_bridge::AuthoritativeModerationStatus as S;
        use crate::workflows::moderation::ModerationDenial as D;
        use crate::workflows::strong_command::{
            classify_terminal_execution_error, CommandTerminalReasonCode as R,
        };
        let authority = AuthorityId::new_from_entropy([201; 32]);
        let target = AuthorityId::new_from_entropy([202; 32]);
        let context = ContextId::new_from_entropy([203; 32]);
        let channel = ChannelId::from_bytes([204; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(authority));
        let core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime.clone()).unwrap(),
        ));
        // The sender access check reads the homes signal.
        crate::signal_defs::register_app_signals(&*core.read().await)
            .await
            .unwrap();
        for (status, reason) in [
            (
                S {
                    is_banned: true,
                    is_muted: false,
                    roster_known: true,
                    is_member: true,
                },
                R::Banned,
            ),
            (
                S {
                    is_banned: false,
                    is_muted: true,
                    roster_known: true,
                    is_member: true,
                },
                R::Muted,
            ),
            (
                S {
                    is_banned: false,
                    is_muted: false,
                    roster_known: true,
                    is_member: false,
                },
                R::NotMember,
            ),
        ] {
            runtime.set_moderation_status(context, channel, target, status);
            let error = enforce_home_moderation_for_sender(
                &core,
                context,
                channel,
                target,
                1_000,
                &test_messaging_budget(&core).await,
            )
            .await
            .expect_err("owned authoritative status denies send");
            let denial = crate::workflows::moderation::denial_from_error(&error)
                .expect("typed denial source");
            match denial {
                D::NotMember {
                    context: c,
                    channel: h,
                    authority: a,
                }
                | D::Muted {
                    context: c,
                    channel: h,
                    authority: a,
                }
                | D::Banned {
                    context: c,
                    channel: h,
                    authority: a,
                } => assert_eq!((*c, *h, *a), (context, channel, target)),
                D::AccessRestricted { .. } => panic!("unexpected access denial: {denial}"),
            }
            assert_eq!(classify_terminal_execution_error(&error).reason, reason);
            let retained = aura_core::AuraError::from(crate::workflows::error::runtime_call(
                "context wrapper",
                error,
            ));
            assert_eq!(classify_terminal_execution_error(&retained).reason, reason);
        }
        runtime.set_moderation_status(
            context,
            channel,
            target,
            S {
                is_banned: true,
                is_muted: false,
                roster_known: true,
                is_member: false,
            },
        );
        let error = enforce_home_join_allowed(
            &core,
            context,
            channel,
            target,
            &test_messaging_budget(&core).await,
        )
        .await
        .expect_err("owned authoritative ban denies join");
        assert!(matches!(
            crate::workflows::moderation::denial_from_error(&error),
            Some(D::Banned { .. })
        ));
        runtime.set_moderation_status(
            context,
            channel,
            target,
            S {
                is_banned: false,
                is_muted: false,
                roster_known: true,
                is_member: true,
            },
        );
        enforce_home_moderation_for_sender(
            &core,
            context,
            channel,
            target,
            1_000,
            &test_messaging_budget(&core).await,
        )
        .await
        .expect("member allowed");

        // Task 121: a Limited access override refuses the send with the typed
        // permission denial, not an internal error.
        let mut home = crate::views::home::HomeState::new(channel, None, authority, 0, context);
        home.set_access_override(target, aura_social::AccessLevel::Limited);
        let mut homes = crate::views::home::HomesState::default();
        homes.add_home(home);
        crate::workflows::signals::emit_signal(
            &core,
            &*crate::signal_defs::HOMES_SIGNAL,
            homes,
            crate::signal_defs::HOMES_SIGNAL_NAME,
        )
        .await
        .unwrap();
        let error = enforce_home_moderation_for_sender(
            &core,
            context,
            channel,
            target,
            1_000,
            &test_messaging_budget(&core).await,
        )
        .await
        .expect_err("Limited access refuses send_message");
        assert!(matches!(
            crate::workflows::moderation::denial_from_error(&error),
            Some(D::AccessRestricted { context: c, channel: h, authority: a, .. })
                if (*c, *h, *a) == (context, channel, target)
        ));
        assert!(error.to_string().contains("access level"), "{error}");
        assert_eq!(
            classify_terminal_execution_error(&error).reason,
            R::PermissionDenied
        );
        let semantic =
            super::super::SendMessageError::ModerationDenied { source: error }.semantic_error();
        assert_eq!(
            semantic.code,
            crate::ui_contract::SemanticFailureCode::PermissionDenied
        );
    }

    #[tokio::test]
    async fn authoritative_home_moderation_status_reads_runtime_owned_status() {
        let authority = AuthorityId::new_from_entropy([91u8; 32]);
        let target = AuthorityId::new_from_entropy([92u8; 32]);
        let context_id = ContextId::new_from_entropy([93u8; 32]);
        let channel_id = ChannelId::from_bytes([94u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(authority));
        runtime.set_moderation_status(
            context_id,
            channel_id,
            target,
            crate::runtime_bridge::AuthoritativeModerationStatus {
                is_banned: true,
                is_muted: true,
                roster_known: true,
                is_member: false,
            },
        );
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime).unwrap(),
        ));

        let status = authoritative_home_moderation_status(
            &app_core,
            context_id,
            channel_id,
            target,
            1_000,
            &test_messaging_budget(&app_core).await,
        )
        .await
        .expect("runtime-backed moderation status should resolve");

        assert!(status.is_banned);
        assert!(status.is_muted);
        assert!(status.roster_known);
        assert!(!status.is_member);
    }
}
