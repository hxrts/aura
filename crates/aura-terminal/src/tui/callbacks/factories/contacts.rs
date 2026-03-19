//! Contacts domain callbacks.

use super::*;

/// All callbacks for the contacts screen
#[derive(Clone)]
pub struct ContactsCallbacks {
    pub on_update_nickname: UpdateNicknameCallback,
    pub on_start_chat: StartChatCallback,
    pub(crate) on_invite_to_channel: TwoStringContextOwnedCallback,
    pub on_invite_lan_peer: Arc<dyn Fn(String, String) + Send + Sync>,
    pub on_remove_contact: IdCallback,
}

impl ContactsCallbacks {
    #[must_use]
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_update_nickname: Self::make_update_nickname(ctx.clone(), tx.clone()),
            on_start_chat: Self::make_start_chat(ctx.clone(), tx.clone()),
            on_invite_to_channel: Self::make_invite_to_channel(ctx.clone(), tx.clone()),
            on_invite_lan_peer: Self::make_invite_lan_peer(ctx.clone(), tx.clone()),
            on_remove_contact: Self::make_remove_contact(ctx, tx),
        }
    }

    fn make_update_nickname(ctx: Arc<IoContext>, tx: UiUpdateSender) -> UpdateNicknameCallback {
        Arc::new(move |contact_id: String, new_nickname: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let contact_id_clone = contact_id.clone();
            let nickname_clone = new_nickname.clone();
            let cmd = EffectCommand::UpdateContactNickname {
                contact_id,
                nickname: new_nickname,
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::NicknameUpdated {
                                contact_id: contact_id_clone,
                                nickname: nickname_clone,
                            },
                        )
                        .await;
                    }
                    Err(_e) => {
                        tracing::debug!(error = %_e, "dispatch error (surfaced via ERROR_SIGNAL)");
                    }
                }
            });
        })
    }

    fn make_start_chat(ctx: Arc<IoContext>, tx: UiUpdateSender) -> StartChatCallback {
        Arc::new(move |contact_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let contact_id_clone = contact_id.clone();
            let cmd = EffectCommand::StartDirectChat { contact_id };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ChatStarted {
                                contact_id: contact_id_clone,
                            },
                        )
                        .await;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, contact_id = %contact_id_clone, "StartDirectChat dispatch failed");
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
                    }
                }
            });
        })
    }

    fn make_invite_to_channel(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> TwoStringContextOwnedCallback {
        Arc::new(
            move |contact_id: String,
                  channel: String,
                  context_id: Option<String>,
                  operation: Option<WorkflowHandoffOperationOwner>| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                let operation_instance_id = operation
                    .as_ref()
                    .map(|operation| operation.harness_handle().instance_id().clone());
                let app_core = ctx.app_core_raw().clone();
                spawn_ctx(ctx, async move {
                    if let Some(operation) = operation {
                        let _ = operation.handoff_to_app_workflow(
                            SemanticOperationTransferScope::InviteActorToChannel,
                        );
                    }

                    let dispatch = std::panic::AssertUnwindSafe(
                        aura_app::ui::workflows::messaging::invite_user_to_channel_with_context(
                            &app_core,
                            &contact_id,
                            &channel,
                            context_id
                                .as_deref()
                                .and_then(|context_id| context_id.parse().ok()),
                            operation_instance_id,
                            None,
                            None,
                        ),
                    )
                    .catch_unwind();
                    match dispatch.await {
                        Ok(Ok(_)) => {}
                        Ok(Err(error)) => {
                            send_ui_update_reliable(
                                &tx,
                                UiUpdate::ToastAdded(ToastMessage::error(
                                    "invitation",
                                    format!("Invite to channel failed: {error}"),
                                )),
                            )
                            .await;
                        }
                        Err(panic) => {
                            let detail = if let Some(message) = panic.downcast_ref::<&str>() {
                                format!("invite_to_channel callback panicked: {message}")
                            } else if let Some(message) = panic.downcast_ref::<String>() {
                                format!("invite_to_channel callback panicked: {message}")
                            } else {
                                "invite_to_channel callback panicked".to_string()
                            };
                            send_ui_update_reliable(
                                &tx,
                                UiUpdate::ToastAdded(ToastMessage::error(
                                    "invitation",
                                    detail.clone(),
                                )),
                            )
                            .await;
                        }
                    }
                });
            },
        )
    }

    fn make_remove_contact(ctx: Arc<IoContext>, _tx: UiUpdateSender) -> IdCallback {
        Arc::new(move |contact_id: String| {
            let ctx = ctx.clone();
            let cmd = EffectCommand::RemoveContact { contact_id };
            spawn_ctx(ctx.clone(), async move {
                let _ = ctx.dispatch(cmd).await;
            });
        })
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
            let cmd = EffectCommand::InviteLanPeer {
                authority_id: parsed_authority_id,
                address,
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        ctx.mark_peer_invited(&authority_id_clone).await;
                        send_ui_update_required(
                            &tx,
                            UiUpdate::LanPeerInvited {
                                peer_id: authority_id_clone,
                            },
                        )
                        .await;
                    }
                    Err(_e) => {
                        tracing::debug!(error = %_e, "dispatch error (surfaced via ERROR_SIGNAL)");
                    }
                }
            });
        })
    }
}
