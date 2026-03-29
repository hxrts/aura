//! Contacts domain callbacks.

use super::*;

/// All callbacks for the contacts screen
#[derive(Clone)]
pub struct ContactsCallbacks {
    pub(crate) on_update_nickname: UpdateNicknameCallback,
    pub(crate) on_start_chat: StartChatCallback,
    pub(crate) on_invite_to_channel: TwoStringContextHandoffCallback,
    pub on_invite_lan_peer: Arc<dyn Fn(String, String) + Send + Sync>,
    pub(crate) on_remove_contact: IdLocalOwnedCallback,
}

impl ContactsCallbacks {
    #[must_use]
    pub fn new(runtime: &CallbackFactoryRuntime) -> Self {
        let ctx = runtime.ctx();
        let tx = runtime.tx();
        Self {
            on_update_nickname: Self::make_update_nickname(ctx.clone(), tx.clone()),
            on_start_chat: Self::make_start_chat(ctx.clone(), tx.clone()),
            on_invite_to_channel: Self::make_invite_to_channel(ctx.clone(), tx.clone()),
            on_invite_lan_peer: Self::make_invite_lan_peer(ctx.clone(), tx.clone()),
            on_remove_contact: Self::make_remove_contact(ctx, tx),
        }
    }

    fn make_update_nickname(ctx: Arc<IoContext>, tx: UiUpdateSender) -> UpdateNicknameCallback {
        Arc::new(
            move |contact_id: String,
                  new_nickname: String,
                  operation: LocalTerminalOperationOwner| {
                let contact_id_clone = contact_id.clone();
                let nickname_clone = new_nickname.clone();
                spawn_local_terminal_result_callback(
                    ctx.clone(),
                    tx.clone(),
                    operation,
                    "UpdateContactNickname callback",
                    move |ctx| async move {
                        let app_core = ctx.app_core_raw().clone();
                        let timestamp_ms =
                            aura_app::ui::workflows::time::current_time_ms(&app_core)
                                .await
                                .map_err(aura_core::AuraError::from)
                                .map_err(crate::error::TerminalError::from)?;
                        aura_app::ui::workflows::contacts::update_contact_nickname(
                            &app_core,
                            &contact_id,
                            &new_nickname,
                            timestamp_ms,
                        )
                        .await
                        .map_err(Into::into)
                    },
                    move |tx, ()| async move {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::NicknameUpdated {
                                contact_id: contact_id_clone,
                                nickname: nickname_clone,
                            },
                        )
                        .await;
                    },
                    |tx, error| async move {
                        emit_error_toast(
                            &tx,
                            "contacts",
                            format!("Update nickname failed: {error}"),
                        )
                        .await;
                    },
                );
            },
        )
    }

    fn make_start_chat(ctx: Arc<IoContext>, tx: UiUpdateSender) -> StartChatCallback {
        Arc::new(
            move |contact_id: String, operation: LocalTerminalOperationOwner| {
                let success_contact_id = contact_id.clone();
                spawn_local_terminal_result_callback(
                    ctx.clone(),
                    tx.clone(),
                    operation,
                    "StartDirectChat callback",
                    move |ctx| async move {
                        let app_core = ctx.app_core_raw().clone();
                        let timestamp_ms =
                            aura_app::ui::workflows::time::current_time_ms(&app_core)
                                .await
                                .map_err(aura_core::AuraError::from)
                                .map_err(crate::error::TerminalError::from)?;
                        aura_app::ui::workflows::messaging::start_direct_chat(
                            &app_core,
                            &contact_id,
                            timestamp_ms,
                        )
                        .await
                        .map(|_| ())
                        .map_err(Into::into)
                    },
                    move |tx, ()| async move {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ChatStarted {
                                contact_id: success_contact_id,
                            },
                        )
                        .await;
                    },
                    |tx, error| async move {
                        emit_error_toast(&tx, "chat", format!("Start chat failed: {error}")).await;
                    },
                );
            },
        )
    }

    fn make_invite_to_channel(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> TwoStringContextHandoffCallback {
        Arc::new(
            move |contact_id: String,
                  channel: String,
                  context_id: Option<String>,
                  operation: WorkflowHandoffOperationOwner| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                spawn_handoff_workflow_callback(
                    ctx,
                    tx,
                    operation,
                    WorkflowHandoffSpec::new(
                        SemanticOperationTransferScope::InviteActorToChannel,
                        "invitation",
                        "Invite to channel failed",
                        "invite_to_channel callback",
                    ),
                    move |app_core, operation_instance_id| {
                        let contact_id = contact_id.clone();
                        let channel = channel.clone();
                        let parsed_context_id = context_id
                            .as_deref()
                            .and_then(|context_id| context_id.parse().ok());
                        async move {
                            aura_app::ui::workflows::messaging::invite_user_to_channel_with_context_terminal_status(
                                &app_core,
                                &contact_id,
                                &channel,
                                parsed_context_id,
                                operation_instance_id,
                                None,
                                None,
                            )
                            .await
                        }
                    },
                );
            },
        )
    }

    fn make_remove_contact(ctx: Arc<IoContext>, tx: UiUpdateSender) -> IdLocalOwnedCallback {
        Arc::new(
            move |contact_id: String, operation: LocalTerminalOperationOwner| {
                spawn_local_terminal_result_callback(
                    ctx.clone(),
                    tx.clone(),
                    operation,
                    "RemoveContact callback",
                    move |ctx| async move {
                        let app_core = ctx.app_core_raw().clone();
                        let timestamp_ms =
                            aura_app::ui::workflows::time::current_time_ms(&app_core)
                                .await
                                .map_err(aura_core::AuraError::from)
                                .map_err(crate::error::TerminalError::from)?;
                        aura_app::ui::workflows::contacts::remove_contact(
                            &app_core,
                            &contact_id,
                            timestamp_ms,
                        )
                        .await
                        .map_err(Into::into)
                    },
                    |_tx, ()| async {},
                    |tx, error| async move {
                        emit_error_toast(
                            &tx,
                            "contacts",
                            format!("Remove contact failed: {error}"),
                        )
                        .await;
                    },
                );
            },
        )
    }

    fn make_invite_lan_peer(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> Arc<dyn Fn(String, String) + Send + Sync> {
        Arc::new(move |authority_id: String, address: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let authority_id_clone = authority_id.clone();
            let parsed_authority_id = match authority_id.parse::<AuthorityId>() {
                Ok(id) => id,
                Err(error) => {
                    enqueue_invalid_lan_authority_toast(ctx, tx, authority_id, error.to_string());
                    return;
                }
            };
            spawn_observed_adaptation_dispatch_callback(
                ctx.clone(),
                tx,
                EffectCommand::InviteLanPeer {
                    authority_id: parsed_authority_id,
                    address,
                },
                move |tx| {
                    let ctx = ctx.clone();
                    async move {
                        ctx.mark_peer_invited(&authority_id_clone).await;
                        send_ui_update_required(
                            &tx,
                            UiUpdate::LanPeerInvited {
                                peer_id: authority_id_clone,
                            },
                        )
                        .await;
                    }
                },
                |error| async move {
                    tracing::debug!(error = %error, "dispatch error (surfaced via ERROR_SIGNAL)");
                },
            );
        })
    }
}
