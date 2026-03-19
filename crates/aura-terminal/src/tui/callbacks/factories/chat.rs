//! Chat domain callbacks.

use super::*;

/// All callbacks for the chat screen
#[derive(Clone)]
pub struct ChatCallbacks {
    pub(crate) on_send: SendCallback,
    pub(crate) on_accept_pending_channel_invitation: NoArgOwnedCallback,
    pub(crate) on_join_channel: JoinChannelCallback,
    pub on_retry_message: RetryMessageCallback,
    pub(crate) on_create_channel: CreateChannelCallback,
    pub on_set_topic: SetTopicCallback,
    pub on_close_channel: IdCallback,
    pub on_list_participants: IdCallback,
}

impl ChatCallbacks {
    /// Create chat callbacks from context
    pub fn new(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
        _app_core: Arc<async_lock::RwLock<aura_app::ui::types::AppCore>>,
    ) -> Self {
        Self {
            on_send: Self::make_send(ctx.clone(), tx.clone()),
            on_accept_pending_channel_invitation: Self::make_accept_pending_channel_invitation(
                ctx.clone(),
                tx.clone(),
            ),
            on_join_channel: Self::make_join_channel(ctx.clone(), tx.clone()),
            on_retry_message: Self::make_retry_message(ctx.clone(), tx.clone()),
            on_create_channel: Self::make_create_channel(ctx.clone(), tx.clone()),
            on_set_topic: Self::make_set_topic(ctx.clone(), tx.clone()),
            on_close_channel: Self::make_close_channel(ctx.clone(), tx.clone()),
            on_list_participants: Self::make_list_participants(ctx, tx),
        }
    }

    pub fn send(&self, channel_id: String, content: String) {
        (self.on_send)(channel_id, content, None);
    }

    fn make_accept_pending_channel_invitation(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> NoArgOwnedCallback {
        Arc::new(move |operation| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let app_core = ctx.app_core_raw().clone();
            let operation_instance_id = operation.harness_handle().instance_id;
            spawn_ctx(ctx, async move {
                let _ = operation.handoff_to_app_workflow(
                    SemanticOperationTransferScope::AcceptPendingChannelInvitation,
                );
                let accept = std::panic::AssertUnwindSafe(
                    aura_app::ui::workflows::invitation::accept_pending_home_invitation_with_instance(
                        &app_core,
                        Some(operation_instance_id),
                    ),
                )
                .catch_unwind();
                match accept.await {
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => {
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::error(
                                "invitation",
                                format!("Accept pending invitation failed: {error}"),
                            )),
                        )
                        .await;
                    }
                    Err(panic) => {
                        let detail = if let Some(message) = panic.downcast_ref::<&str>() {
                            format!(
                                "accept_pending_channel_invitation callback panicked: {message}"
                            )
                        } else if let Some(message) = panic.downcast_ref::<String>() {
                            format!(
                                "accept_pending_channel_invitation callback panicked: {message}"
                            )
                        } else {
                                "accept_pending_channel_invitation callback panicked".to_string()
                        };
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::error("invitation", detail)),
                        )
                        .await;
                    }
                }
            });
        })
    }

    fn make_send(ctx: Arc<IoContext>, tx: UiUpdateSender) -> SendCallback {
        let strong_resolver =
            Arc::new(aura_app::ui::workflows::strong_command::CommandResolver::default());
        Arc::new(move |channel_id: String, content: String, operation| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let strong_resolver = strong_resolver.clone();
            let app_core = ctx.app_core_raw().clone();
            let channel_id_clone = channel_id;
            let content_clone = content;

            spawn_ctx(ctx.clone(), async move {
                let operation_instance_id = operation.map(|operation| {
                    let operation_instance_id = operation.harness_handle().instance_id;
                    let _ = operation.handoff_to_app_workflow(
                        SemanticOperationTransferScope::SendChatMessage,
                    );
                    operation_instance_id
                });

                // Channel ID is now passed from the TUI's selected_channel to avoid
                // race conditions with async channel selection updates

                let trimmed = content_clone.trim_start();
                if trimmed.starts_with("/") {
                    // IRC-style command path
                    let parsed = match aura_app::ui::workflows::strong_command::ParsedCommand::parse(
                        trimmed,
                    ) {
                        Ok(command) => command,
                        Err(e) => {
                            let (status, reason) = classify_chat_command_error(&e);
                            let message =
                                command_outcome_message(e.to_string(), status, reason, None);
                            send_ui_update_reliable(
                                &tx,
                                UiUpdate::ToastAdded(ToastMessage::error("command", message)),
                            )
                            .await;
                            return;
                        }
                    };
                    let irc_name = trimmed
                        .split_whitespace()
                        .next()
                        .unwrap_or("/command")
                        .trim_start_matches('/')
                        .to_string();
                    let joined_channel_name = match &parsed {
                        aura_app::ui::workflows::strong_command::ParsedCommand::Join {
                            channel,
                        } => Some(channel.trim_start_matches('#').to_string()),
                        _ => None,
                    };

                    match parsed {
                        aura_app::ui::workflows::strong_command::ParsedCommand::Help {
                            command,
                        } => {
                            if let Some(raw_name) = command
                                .as_deref()
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                            {
                                let normalized = raw_name.trim_start_matches('/').to_lowercase();
                                if let Some(help) =
                                    aura_app::ui::workflows::chat_commands::command_help(
                                        &normalized,
                                    )
                                {
                                    send_ui_update_reliable(
                                        &tx,
                                        UiUpdate::ToastAdded(ToastMessage::info(
                                            "help",
                                            format!("{} — {}", help.syntax, help.description),
                                        )),
                                    )
                                    .await;
                                } else {
                                    send_ui_update_reliable(
                                        &tx,
                                        UiUpdate::ToastAdded(ToastMessage::error(
                                            "help",
                                            format!("Unknown command: /{normalized}"),
                                        )),
                                    )
                                    .await;
                                }
                            } else {
                                send_ui_update_reliable(
                                    &tx,
                                    UiUpdate::ToastAdded(ToastMessage::info(
                                        "help",
                                        "Use ? for TUI help. Run /help <command> for details. Core commands: /msg /me /nick /who /whois /join /leave /topic /invite /homeinvite /homeaccept /kick /ban /unban /mute /unmute /pin /unpin /op /deop /mode /neighborhood /nhadd /nhlink",
                                    )),
                                )
                                .await;
                            }
                            return;
                        }
                        parsed => {
                            let actor = {
                                let core = ctx.app_core_raw().read().await;
                                core.runtime()
                                    .map(|runtime| runtime.authority_id())
                                    .or_else(|| core.authority().copied())
                            };

                            let snapshot =
                                strong_resolver.capture_snapshot(ctx.app_core_raw()).await;
                            let resolved = match strong_resolver.resolve(parsed, &snapshot) {
                                Ok(value) => value,
                                Err(e) => {
                                    let (status, reason) = classify_command_resolver_error(&e);
                                    let message = command_outcome_message(
                                        e.to_string(),
                                        status,
                                        reason,
                                        None,
                                    );
                                    send_ui_update_reliable(
                                        &tx,
                                        UiUpdate::ToastAdded(ToastMessage::error(
                                            "command", message,
                                        )),
                                    )
                                    .await;
                                    return;
                                }
                            };
                            let plan = match strong_resolver.plan(
                                resolved,
                                &snapshot,
                                Some(&channel_id_clone),
                                actor,
                            ) {
                                Ok(value) => value,
                                Err(e) => {
                                    let (status, reason) = classify_command_resolver_error(&e);
                                    let message = command_outcome_message(
                                        e.to_string(),
                                        status,
                                        reason,
                                        None,
                                    );
                                    send_ui_update_reliable(
                                        &tx,
                                        UiUpdate::ToastAdded(ToastMessage::error(
                                            "command", message,
                                        )),
                                    )
                                    .await;
                                    return;
                                }
                            };

                            match aura_app::ui::workflows::strong_command::execute_planned(
                                ctx.app_core_raw(),
                                plan,
                            )
                            .await
                            {
                                Ok(result) => {
                                    if let Some(channel_name) = joined_channel_name.as_deref() {
                                        if let Ok(chat) =
                                            aura_app::ui::workflows::messaging::get_chat_state(
                                                ctx.app_core_raw(),
                                            )
                                            .await
                                        {
                                            if let Some(channel) =
                                                chat.all_channels().find(|candidate| {
                                                    candidate
                                                        .name
                                                        .eq_ignore_ascii_case(channel_name)
                                                })
                                            {
                                                send_ui_update_required(
                                                    &tx,
                                                    UiUpdate::ChannelSelected(
                                                        channel.id.to_string(),
                                                    ),
                                                )
                                                .await;
                                            }
                                        }
                                    }
                                    let state_label = match result.consistency_state {
                                        aura_app::ui::workflows::strong_command::ConsistencyState::Accepted => "accepted",
                                        aura_app::ui::workflows::strong_command::ConsistencyState::Replicated => "replicated",
                                        aura_app::ui::workflows::strong_command::ConsistencyState::Enforced => "enforced",
                                        aura_app::ui::workflows::strong_command::ConsistencyState::TimedOutPartial => "partial-timeout",
                                    };

                                    if matches!(
                                        result.consistency_state,
                                        aura_app::ui::workflows::strong_command::ConsistencyState::TimedOutPartial
                                    ) {
                                        let details = result
                                            .details
                                            .unwrap_or_else(|| "consistency barrier timed out".to_string());
                                        let message = command_outcome_message(
                                            format!("/{irc_name}: {details} ({state_label})"),
                                            CommandOutcomeStatus::Failed,
                                            CommandReasonCode::Internal,
                                            Some(state_label),
                                        );
                                        send_ui_update_reliable(
                                            &tx,
                                            UiUpdate::ToastAdded(ToastMessage::error(
                                                "command",
                                                message,
                                            )),
                                        )
                                        .await;
                                    } else if let Some(details) = result.details {
                                        let message = command_outcome_message(
                                            format!("{details} ({state_label})"),
                                            CommandOutcomeStatus::Ok,
                                            CommandReasonCode::None,
                                            Some(state_label),
                                        );
                                        send_ui_update_reliable(
                                            &tx,
                                            UiUpdate::ToastAdded(ToastMessage::info(
                                                "command",
                                                message,
                                            )),
                                        )
                                        .await;
                                    } else {
                                        let message = command_outcome_message(
                                            format!("/{irc_name} ({state_label})"),
                                            CommandOutcomeStatus::Ok,
                                            CommandReasonCode::None,
                                            Some(state_label),
                                        );
                                        send_ui_update_reliable(
                                            &tx,
                                            UiUpdate::ToastAdded(ToastMessage::success(
                                                "command",
                                                message,
                                            )),
                                        )
                                        .await;
                                    }
                                }
                                Err(e) => {
                                    let (status, reason) = classify_command_error(&e);
                                    let message = command_outcome_message(
                                        format!("/{irc_name}: {e}"),
                                        status,
                                        reason,
                                        None,
                                    );
                                    send_ui_update_reliable(
                                        &tx,
                                        UiUpdate::ToastAdded(ToastMessage::error(
                                            "command", message,
                                        )),
                                    )
                                    .await;
                                }
                            }
                            return;
                        }
                    }
                }

                // Normal message path
                send_ui_update_reliable(
                    &tx,
                    UiUpdate::MessageSent {
                        channel: channel_id_clone.clone(),
                        content: content_clone.clone(),
                    },
                )
                .await;

                let result = match channel_id_clone.parse() {
                    Ok(channel_id) => {
                        if let Some(operation_instance_id) = operation_instance_id.clone() {
                            aura_app::ui::workflows::messaging::send_message_now_with_instance(
                                &app_core,
                                channel_id,
                                &content_clone,
                                Some(operation_instance_id),
                            )
                            .await
                        } else {
                            aura_app::ui::workflows::messaging::send_message_now(
                                &app_core,
                                channel_id,
                                &content_clone,
                            )
                            .await
                        }
                    }
                    Err(_) => {
                        if let Some(operation_instance_id) = operation_instance_id {
                            aura_app::ui::workflows::messaging::send_message_by_name_now_with_instance(
                                &app_core,
                                &channel_id_clone,
                                &content_clone,
                                Some(operation_instance_id),
                            )
                            .await
                        } else {
                            aura_app::ui::workflows::messaging::send_message_by_name_now(
                                &app_core,
                                &channel_id_clone,
                                &content_clone,
                            )
                            .await
                        }
                    }
                };

                match result {
                    Ok(_) => {
                        let channel_name = {
                            let core = app_core.read().await;
                            core.snapshot()
                                .chat
                                .all_channels()
                                .find(|channel| channel.id.to_string() == channel_id_clone)
                                .map(|channel| Some(channel.name.clone()))
                                .unwrap_or(None)
                        };
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::RuntimeFactsUpdated {
                                replace_kinds: vec![RuntimeEventKind::MessageCommitted],
                                facts: vec![RuntimeFact::MessageCommitted {
                                    channel: ChannelFactKey {
                                        id: Some(channel_id_clone.clone()),
                                        name: channel_name.clone(),
                                    },
                                    content: content_clone.clone(),
                                }],
                            },
                        )
                        .await;
                    }
                    Err(e) => {
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::error(
                                "chat",
                                format!("Send message failed: {e}"),
                            )),
                        )
                        .await;
                    }
                }
            });
        })
    }

    fn make_retry_message(ctx: Arc<IoContext>, tx: UiUpdateSender) -> RetryMessageCallback {
        Arc::new(
            move |message_id: String, channel: String, content: String| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                let msg_id = message_id.clone();
                let cmd = EffectCommand::RetryMessage {
                    message_id,
                    channel,
                    content,
                };
                spawn_ctx(ctx.clone(), async move {
                    match ctx.dispatch(cmd).await {
                        Ok(_) => {
                            send_ui_update_required(
                                &tx,
                                UiUpdate::MessageRetried { message_id: msg_id },
                            )
                            .await;
                        }
                        Err(_e) => {
                            tracing::debug!(error = %_e, "dispatch error (surfaced via ERROR_SIGNAL)");
                        }
                    }
                });
            },
        )
    }

    fn make_join_channel(ctx: Arc<IoContext>, tx: UiUpdateSender) -> JoinChannelCallback {
        Arc::new(move |channel_name: String, operation: Option<SubmittedOperationOwner>| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let app_core = ctx.app_core_raw().clone();
            spawn_ctx(ctx.clone(), async move {
                let operation_instance_id = operation
                    .as_ref()
                    .map(|operation| operation.harness_handle().instance_id);
                if let Some(operation) = operation {
                    let _ = operation.handoff_to_app_workflow(SemanticOperationTransferScope::JoinChannel);
                }
                match aura_app::ui::workflows::messaging::join_channel_by_name_with_instance(
                    &app_core,
                    &channel_name,
                    operation_instance_id,
                )
                .await
                {
                    Ok(()) => {}
                    Err(error) => {
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::error(
                                "chat",
                                format!("Join channel failed: {error}"),
                            )),
                        )
                        .await;
                    }
                }
            });
        })
    }

    fn make_list_participants(ctx: Arc<IoContext>, tx: UiUpdateSender) -> IdCallback {
        Arc::new(move |channel_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let channel_id_clone = channel_id;
            spawn_ctx(ctx.clone(), async move {
                match aura_app::ui::workflows::query::list_participants(
                    ctx.app_core_raw(),
                    &channel_id_clone,
                )
                .await
                {
                    Ok(participants) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ChannelInfoParticipants {
                                channel_id: channel_id_clone,
                                participants,
                            },
                        )
                        .await;
                    }
                    Err(e) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::error(
                                "participants",
                                e.to_string(),
                            )),
                        )
                        .await;
                    }
                }
            });
        })
    }

    fn make_create_channel(ctx: Arc<IoContext>, tx: UiUpdateSender) -> CreateChannelCallback {
        Arc::new(
            move |name: String,
                  topic: Option<String>,
                  members: Vec<String>,
                  threshold_k: u8,
                  operation: Option<SubmittedOperationOwner>| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                let channel_name = name.clone();
                let cmd = EffectCommand::CreateChannel {
                    name,
                    topic,
                    members,
                    threshold_k,
                };
                spawn_ctx(ctx.clone(), async move {
                    match ctx.dispatch_with_response(cmd).await {
                        Ok(OpResponse::ChannelCreated {
                            channel_id,
                            context_id,
                        }) => {
                            send_ui_update_required(
                                &tx,
                                UiUpdate::ChannelCreated {
                                    channel_id,
                                    context_id,
                                    name: channel_name,
                                },
                            )
                            .await;
                            if let Some(operation) = operation {
                                operation.succeed().await;
                            }
                        }
                        Ok(other) => {
                            if let Some(operation) = operation {
                                operation
                                    .fail(format!(
                                        "Create channel returned unexpected response: {other:?}"
                                    ))
                                    .await;
                            }
                            send_ui_update_required(
                                &tx,
                                UiUpdate::ToastAdded(ToastMessage::error(
                                    "create-channel",
                                    format!(
                                        "Create channel returned unexpected response: {other:?}"
                                    ),
                                )),
                            )
                            .await;
                        }
                        Err(e) => {
                            if let Some(operation) = operation {
                                operation.fail(e.to_string()).await;
                            }
                            send_ui_update_required(
                                &tx,
                                UiUpdate::ToastAdded(ToastMessage::error(
                                    "create-channel",
                                    e.to_string(),
                                )),
                            )
                            .await;
                        }
                    }
                });
            },
        )
    }

    fn make_set_topic(ctx: Arc<IoContext>, tx: UiUpdateSender) -> SetTopicCallback {
        Arc::new(move |channel_id: String, topic: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let ch = channel_id.clone();
            let t = topic.clone();
            let cmd = EffectCommand::SetTopic {
                channel: channel_id,
                text: topic,
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::TopicSet {
                                channel: ch,
                                topic: t,
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

    fn make_close_channel(ctx: Arc<IoContext>, _tx: UiUpdateSender) -> IdCallback {
        Arc::new(move |channel_id: String| {
            let ctx = ctx.clone();
            let cmd = EffectCommand::CloseChannel {
                channel: channel_id,
            };
            spawn_ctx(ctx.clone(), async move {
                let _ = ctx.dispatch(cmd).await;
            });
        })
    }
}
