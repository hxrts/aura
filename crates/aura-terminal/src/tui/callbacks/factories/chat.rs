//! Chat domain callbacks.

use super::*;

/// All callbacks for the chat screen
#[derive(Clone)]
pub struct ChatCallbacks {
    pub(crate) on_run_slash_command: SlashCommandCallback,
    pub(crate) on_send_owned: SendOwnedCallback,
    pub(crate) on_accept_pending_channel_invitation: NoArgOwnedCallback,
    pub(crate) on_join_channel: JoinChannelCallback,
    pub(crate) on_retry_message: RetryMessageCallback,
    pub(crate) on_create_channel: CreateChannelCallback,
    pub(crate) on_set_topic: SetTopicCallback,
    pub(crate) on_close_channel: IdLocalOwnedCallback,
    pub on_list_participants: IdCallback,
}

impl ChatCallbacks {
    /// Create chat callbacks from context
    pub fn new(runtime: &CallbackFactoryRuntime) -> Self {
        let ctx = runtime.ctx();
        let tx = runtime.tx();
        Self {
            on_run_slash_command: Self::make_run_slash_command(ctx.clone(), tx.clone()),
            on_send_owned: Self::make_send_owned(ctx.clone(), tx.clone()),
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
        (self.on_run_slash_command)(channel_id, content);
    }

    fn make_accept_pending_channel_invitation(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> NoArgOwnedCallback {
        Arc::new(move |operation| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            spawn_handoff_workflow_callback_with_success(
                ctx,
                tx,
                operation,
                WorkflowHandoffSpec::new(
                    SemanticOperationTransferScope::AcceptPendingChannelInvitation,
                    "invitation",
                    "Accept pending invitation failed",
                    "accept_pending_channel_invitation callback",
                ),
                |app_core, operation_instance_id| async move {
                    aura_app::ui::workflows::invitation::accept_pending_channel_invitation_with_binding_terminal_status(
                        &app_core,
                        operation_instance_id,
                    )
                    .await
                },
                |tx, accepted| async move {
                    if let Some(channel_name) = accepted.channel_name.clone() {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ChannelCreated {
                                operation_instance_id: None,
                                channel_id: accepted.binding.channel_id.clone(),
                                context_id: accepted.binding.context_id.clone(),
                                name: channel_name,
                            },
                        )
                        .await;
                    }
                    send_ui_update_required(&tx, UiUpdate::ChannelSelected(accepted.binding)).await;
                },
            );
        })
    }

    fn make_run_slash_command(ctx: Arc<IoContext>, tx: UiUpdateSender) -> SlashCommandCallback {
        let slash_resolver =
            Arc::new(aura_app::ui::workflows::strong_command::CommandResolver::default());
        Arc::new(move |channel_id: String, content: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let slash_resolver = slash_resolver.clone();
            let channel_id_clone = channel_id;
            let content_clone = content;

            spawn_ctx(ctx.clone(), async move {
                // Channel ID is now passed from the TUI's selected_channel to avoid
                // race conditions with async channel selection updates

                let trimmed = content_clone.trim_start();
                if trimmed.starts_with("/") {
                    let actor = {
                        let core = ctx.app_core_raw().read().await;
                        core.runtime()
                            .map(|runtime| runtime.authority_id())
                            .or_else(|| core.authority().copied())
                    };
                    let report = aura_app::ui::workflows::slash_commands::prepare_and_execute(
                        slash_resolver.as_ref(),
                        ctx.app_core_raw(),
                        trimmed,
                        Some(&channel_id_clone),
                        actor,
                    )
                    .await;
                    if let Some(semantic) = report
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.semantic_operation.clone())
                    {
                        let owner =
                            crate::tui::semantic_lifecycle::LocalTerminalOperationOwner::submit(
                                ctx.app_core_raw().clone(),
                                ctx.tasks(),
                                tx.clone(),
                                semantic.operation_id,
                                semantic.kind,
                            );
                        match report.feedback.terminal_settlement.clone() {
                            Some(aura_app::ui::workflows::slash_commands::SlashCommandTerminalSettlement::Succeeded) => {
                                owner.succeed().await;
                            }
                            Some(aura_app::ui::workflows::slash_commands::SlashCommandTerminalSettlement::Failed(error)) => {
                                owner.fail_with(error).await;
                            }
                            None => {}
                        }
                    }
                    let feedback = report.feedback;
                    let toast = match feedback.toast_kind {
                        aura_app::ui::workflows::slash_commands::SlashCommandToastKind::Success => {
                            ToastMessage::success(feedback.topic, feedback.message)
                        }
                        aura_app::ui::workflows::slash_commands::SlashCommandToastKind::Info => {
                            ToastMessage::info(feedback.topic, feedback.message)
                        }
                        aura_app::ui::workflows::slash_commands::SlashCommandToastKind::Error => {
                            ToastMessage::error(feedback.topic, feedback.message)
                        }
                    };
                    send_ui_update_reliable(&tx, UiUpdate::ToastAdded(toast)).await;
                    return;
                }

                send_ui_update_reliable(
                    &tx,
                    UiUpdate::ToastAdded(ToastMessage::error(
                        "chat",
                        "Observed chat callbacks cannot send parity-critical messages",
                    )),
                )
                .await;
            });
        })
    }

    fn make_send_owned(ctx: Arc<IoContext>, tx: UiUpdateSender) -> SendOwnedCallback {
        Arc::new(
            move |channel_id: String, content: String, operation: WorkflowHandoffOperationOwner| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                let app_core = ctx.app_core_raw().clone();
                let channel_id_clone = channel_id;
                let content_clone = content;

                spawn_ctx(ctx, async move {
                    let operation_instance_id = operation.harness_handle().instance_id().clone();
                    let transfer = operation
                        .handoff_to_app_workflow(SemanticOperationTransferScope::SendChatMessage);

                    // Pre-settlement optimistic UI hint (best-effort).
                    let _ = tx.try_send(UiUpdate::MessageSent {
                        channel: channel_id_clone.clone(),
                        content: content_clone.clone(),
                    });

                    let target = match channel_id_clone.parse() {
                        Ok(channel_id) => {
                            aura_app::ui::workflows::messaging::handoff::SendChatTarget::ChannelId(
                                channel_id,
                            )
                        }
                        Err(_) => {
                            aura_app::ui::workflows::messaging::handoff::SendChatTarget::ChannelName(
                                channel_id_clone.clone(),
                            )
                        }
                    };
                    let workflow_app_core = app_core.clone();
                    let result = transfer
                        .run_workflow(
                            app_core,
                            tx.clone(),
                            "make_send_owned send_chat_message",
                            aura_app::ui::workflows::messaging::handoff::send_chat_message(
                                &workflow_app_core,
                                aura_app::ui::workflows::messaging::handoff::SendChatMessageRequest {
                                    target,
                                    content: content_clone.clone(),
                                    operation_instance_id: Some(operation_instance_id),
                                },
                            ),
                        )
                        .await;

                    if let Err(error) = result {
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::error(
                                "chat",
                                format!("Send message failed: {error}"),
                            )),
                        )
                        .await;
                    }
                });
            },
        )
    }

    fn make_retry_message(ctx: Arc<IoContext>, tx: UiUpdateSender) -> RetryMessageCallback {
        Arc::new(
            move |message_id: String,
                  channel: String,
                  content: String,
                  operation: WorkflowHandoffOperationOwner| {
                let msg_id = message_id;
                let ctx = ctx.clone();
                let tx = tx.clone();
                let app_core = ctx.app_core_raw().clone();
                spawn_ctx(ctx, async move {
                    let operation_instance_id = operation.harness_handle().instance_id().clone();
                    let transfer = operation
                        .handoff_to_app_workflow(SemanticOperationTransferScope::SendChatMessage);

                    let result = match channel.parse() {
                        Ok(channel_id) => {
                            aura_app::ui::workflows::messaging::send_message_now_with_instance(
                                &app_core,
                                channel_id,
                                &content,
                                Some(operation_instance_id.clone()),
                            )
                            .await
                        }
                        Err(_) => {
                            aura_app::ui::workflows::messaging::send_message_by_name_now_with_instance(
                                &app_core,
                                &channel,
                                &content,
                                Some(operation_instance_id.clone()),
                            )
                            .await
                        }
                    };

                    match result {
                        Ok(_) => {
                            let terminal = aura_app::ui_contract::WorkflowTerminalStatus {
                                causality: None,
                                status: SemanticOperationStatus::new(
                                    transfer.kind(),
                                    SemanticOperationPhase::Succeeded,
                                ),
                            };
                            let _ = apply_handed_off_terminal_status(
                                &app_core,
                                &tx,
                                transfer.operation_id().clone(),
                                operation_instance_id,
                                transfer.kind(),
                                Some(terminal),
                            )
                            .await;
                            send_ui_update_required(
                                &tx,
                                UiUpdate::MessageRetried { message_id: msg_id },
                            )
                            .await;
                        }
                        Err(error) => {
                            let terminal = aura_app::ui_contract::WorkflowTerminalStatus {
                                causality: None,
                                status: SemanticOperationStatus::failed(
                                    transfer.kind(),
                                    SemanticOperationError::new(
                                        SemanticFailureDomain::Command,
                                        SemanticFailureCode::InternalError,
                                    )
                                    .with_detail(error.to_string()),
                                ),
                            };
                            let _ = apply_handed_off_terminal_status(
                                &app_core,
                                &tx,
                                transfer.operation_id().clone(),
                                operation_instance_id,
                                transfer.kind(),
                                Some(terminal),
                            )
                            .await;
                            emit_error_toast(&tx, "chat", format!("Retry message failed: {error}"))
                                .await;
                        }
                    }
                });
            },
        )
    }

    fn make_join_channel(ctx: Arc<IoContext>, tx: UiUpdateSender) -> JoinChannelCallback {
        Arc::new(
            move |channel_name: String, operation: WorkflowHandoffOperationOwner| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                spawn_handoff_workflow_callback_with_success(
                    ctx,
                    tx,
                    operation,
                    WorkflowHandoffSpec::new(
                        SemanticOperationTransferScope::JoinChannel,
                        "chat",
                        "Join channel failed",
                        "join_channel callback",
                    ),
                    move |app_core, operation_instance_id| async move {
                        aura_app::ui::workflows::messaging::join_channel_by_name_with_binding_terminal_status(
                                &app_core,
                                &channel_name,
                                operation_instance_id,
                            )
                            .await
                    },
                    |tx, binding| async move {
                        send_ui_update_required(&tx, UiUpdate::ChannelSelected(binding)).await;
                    },
                );
            },
        )
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
                  operation: LocalTerminalOperationOwner| {
                let channel_name = name.clone();
                let operation_instance_id = operation.ui_update_instance_id();
                spawn_local_terminal_result_callback(
                    ctx.clone(),
                    tx.clone(),
                    operation,
                    "CreateChannel callback",
                    move |ctx| async move {
                        let app_core = ctx.app_core_raw().clone();
                        let timestamp_ms =
                            aura_app::ui::workflows::time::current_time_ms(&app_core)
                                .await
                                .map_err(aura_core::AuraError::from)
                                .map_err(crate::error::TerminalError::from)?;
                        let created =
                            aura_app::ui::workflows::messaging::create_channel_with_authoritative_binding(
                                &app_core,
                                &name,
                                topic,
                                &members,
                                threshold_k,
                                timestamp_ms,
                            )
                            .await?;
                        Ok((
                            created.channel_id.to_string(),
                            created.context_id.map(|context_id| context_id.to_string()),
                        ))
                    },
                    move |tx, (channel_id, context_id)| async move {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ChannelCreated {
                                operation_instance_id,
                                channel_id,
                                context_id,
                                name: channel_name,
                            },
                        )
                        .await;
                    },
                    |tx, error| async move {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::error(
                                "create-channel",
                                error.to_string(),
                            )),
                        )
                        .await;
                    },
                );
            },
        )
    }

    fn make_set_topic(ctx: Arc<IoContext>, tx: UiUpdateSender) -> SetTopicCallback {
        Arc::new(
            move |channel_id: String, topic: String, operation: LocalTerminalOperationOwner| {
                let ch = channel_id.clone();
                let t = topic.clone();
                spawn_local_terminal_result_callback(
                    ctx.clone(),
                    tx.clone(),
                    operation,
                    "SetTopic callback",
                    move |ctx| async move {
                        let app_core = ctx.app_core_raw().clone();
                        let timestamp_ms =
                            aura_app::ui::workflows::time::current_time_ms(&app_core)
                                .await
                                .map_err(aura_core::AuraError::from)
                                .map_err(crate::error::TerminalError::from)?;
                        aura_app::ui::workflows::messaging::set_topic_by_name(
                            &app_core,
                            &channel_id,
                            &topic,
                            timestamp_ms,
                        )
                        .await
                        .map_err(Into::into)
                    },
                    move |tx, ()| async move {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::TopicSet {
                                channel: ch,
                                topic: t,
                            },
                        )
                        .await;
                    },
                    |tx, error| async move {
                        emit_error_toast(&tx, "chat", format!("Set topic failed: {error}")).await;
                    },
                );
            },
        )
    }

    fn make_close_channel(ctx: Arc<IoContext>, tx: UiUpdateSender) -> IdLocalOwnedCallback {
        Arc::new(
            move |channel_id: String, operation: LocalTerminalOperationOwner| {
                spawn_local_terminal_result_callback(
                    ctx.clone(),
                    tx.clone(),
                    operation,
                    "CloseChannel callback",
                    move |ctx| async move {
                        let app_core = ctx.app_core_raw().clone();
                        let timestamp_ms =
                            aura_app::ui::workflows::time::current_time_ms(&app_core)
                                .await
                                .map_err(aura_core::AuraError::from)
                                .map_err(crate::error::TerminalError::from)?;
                        aura_app::ui::workflows::messaging::close_channel_by_name(
                            &app_core,
                            &channel_id,
                            timestamp_ms,
                        )
                        .await
                        .map_err(Into::into)
                    },
                    |_tx, ()| async move {},
                    |tx, error| async move {
                        emit_error_toast(&tx, "chat", format!("Close channel failed: {error}"))
                            .await;
                    },
                );
            },
        )
    }
}
