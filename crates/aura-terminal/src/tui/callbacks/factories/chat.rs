//! Chat domain callbacks.

use super::*;
use aura_app::ui_contract::{OperationId, SemanticOperationKind};

/// All callbacks for the chat screen
#[derive(Clone)]
pub struct ChatCallbacks {
    pub(crate) on_send: SendCallback,
    pub(crate) on_send_owned: SendOwnedCallback,
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
            on_send: Self::make_send_command(ctx.clone(), tx.clone()),
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
        (self.on_send)(channel_id, content);
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
                OperationId::invitation_accept(),
                SemanticOperationKind::AcceptPendingChannelInvitation,
                SemanticOperationTransferScope::AcceptPendingChannelInvitation,
                "invitation",
                "Accept pending invitation failed",
                "accept_pending_channel_invitation callback",
                |app_core, operation_instance_id| async move {
                    aura_app::ui::workflows::invitation::accept_pending_channel_invitation_with_binding_terminal_status(
                        &app_core,
                        operation_instance_id,
                    )
                    .await
                },
                |tx, accepted| async move {
                    send_ui_update_required(&tx, UiUpdate::ChannelSelected(accepted.binding)).await;
                },
            );
        })
    }

    fn make_send_command(ctx: Arc<IoContext>, tx: UiUpdateSender) -> SendCallback {
        let strong_resolver =
            Arc::new(aura_app::ui::workflows::strong_command::CommandResolver::default());
        Arc::new(move |channel_id: String, content: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let strong_resolver = strong_resolver.clone();
            let channel_id_clone = channel_id;
            let content_clone = content;

            spawn_ctx(ctx.clone(), async move {
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
                                    let state_label = result.consistency_label();

                                    if let Some(classification) = result.terminal_classification() {
                                        let details = result
                                            .details
                                            .as_deref()
                                            .map(str::to_owned)
                                            .or_else(|| {
                                                result
                                                    .default_terminal_detail()
                                                    .map(ToOwned::to_owned)
                                            })
                                            .unwrap_or_else(|| {
                                                "command did not reach the required lifecycle state"
                                                    .to_string()
                                            });
                                        let message = command_outcome_message(
                                            format!("/{irc_name}: {details} ({state_label})"),
                                            classification.status,
                                            classification.reason,
                                            Some(state_label),
                                        );
                                        send_ui_update_reliable(
                                            &tx,
                                            UiUpdate::ToastAdded(ToastMessage::error(
                                                "command", message,
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
                                                "command", message,
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
                                                "command", message,
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

                    let result = match channel_id_clone.parse() {
                        Ok(channel_id) => {
                            aura_app::ui::workflows::messaging::send_message_now_with_instance(
                                &app_core,
                                channel_id,
                                &content_clone,
                                Some(operation_instance_id.clone()),
                            )
                            .await
                        }
                        Err(_) => {
                            aura_app::ui::workflows::messaging::send_message_by_name_now_with_instance(
                                &app_core,
                                &channel_id_clone,
                                &content_clone,
                                Some(operation_instance_id.clone()),
                            )
                            .await
                        }
                    };

                    match result {
                        Ok(_) => {
                            // Terminal settlement first.
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

                            // Best-effort runtime fact after terminal settlement.
                            let channel_fact =
                                match channel_id_clone.parse::<aura_core::ChannelId>() {
                                    Ok(channel_id) => {
                                        ChannelFactKey::identified(channel_id.to_string())
                                    }
                                    Err(_) => ChannelFactKey::named(channel_id_clone.clone()),
                                };
                            send_ui_update_reliable(
                                &tx,
                                UiUpdate::RuntimeFactsUpdated {
                                    replace_kinds: vec![RuntimeEventKind::MessageCommitted],
                                    facts: vec![RuntimeFact::MessageCommitted {
                                        channel: channel_fact,
                                        content: content_clone.clone(),
                                    }],
                                },
                            )
                            .await;
                        }
                        Err(e) => {
                            // Terminal failure settlement.
                            let terminal = aura_app::ui_contract::WorkflowTerminalStatus {
                                causality: None,
                                status: SemanticOperationStatus::failed(
                                    transfer.kind(),
                                    SemanticOperationError::new(
                                        SemanticFailureDomain::Command,
                                        SemanticFailureCode::InternalError,
                                    )
                                    .with_detail(e.to_string()),
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
            },
        )
    }

    fn make_retry_message(ctx: Arc<IoContext>, tx: UiUpdateSender) -> RetryMessageCallback {
        Arc::new(
            move |message_id: String, channel: String, content: String| {
                let msg_id = message_id.clone();
                spawn_observed_dispatch_callback(
                    ctx.clone(),
                    tx.clone(),
                    EffectCommand::RetryMessage {
                        message_id,
                        channel,
                        content,
                    },
                    move |tx| async move {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::MessageRetried { message_id: msg_id },
                        )
                        .await;
                    },
                    |error| async move {
                        tracing::debug!(error = %error, "dispatch error (surfaced via ERROR_SIGNAL)");
                    },
                );
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
                    OperationId::join_channel(),
                    SemanticOperationKind::JoinChannel,
                    SemanticOperationTransferScope::JoinChannel,
                    "chat",
                    "Join channel failed",
                    "join_channel callback",
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
                        match ctx
                            .dispatch_with_response(EffectCommand::CreateChannel {
                                name,
                                topic,
                                members,
                                threshold_k,
                            })
                            .await
                        {
                            Ok(OpResponse::ChannelCreated {
                                channel_id,
                                context_id,
                            }) => Ok((channel_id, context_id)),
                            Ok(other) => Err(crate::error::TerminalError::Operation(format!(
                                "Create channel returned unexpected response: {other:?}"
                            ))),
                            Err(error) => Err(error),
                        }
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
        Arc::new(move |channel_id: String, topic: String| {
            let ch = channel_id.clone();
            let t = topic.clone();
            spawn_observed_dispatch_callback(
                ctx.clone(),
                tx.clone(),
                EffectCommand::SetTopic {
                    channel: channel_id,
                    text: topic,
                },
                move |tx| async move {
                    send_ui_update_required(
                        &tx,
                        UiUpdate::TopicSet {
                            channel: ch,
                            topic: t,
                        },
                    )
                    .await;
                },
                |error| async move {
                    tracing::debug!(error = %error, "dispatch error (surfaced via ERROR_SIGNAL)");
                },
            );
        })
    }

    fn make_close_channel(ctx: Arc<IoContext>, tx: UiUpdateSender) -> IdCallback {
        Arc::new(move |channel_id: String| {
            spawn_observed_dispatch_callback(
                ctx.clone(),
                tx.clone(),
                EffectCommand::CloseChannel {
                    channel: channel_id,
                },
                |_| async {},
                |error| async move {
                    tracing::debug!(error = %error, "dispatch error (surfaced via ERROR_SIGNAL)");
                },
            );
        })
    }
}
