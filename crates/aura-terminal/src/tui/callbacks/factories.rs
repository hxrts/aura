//! # Callback Factories
//!
//! Factory functions that create domain-specific callbacks.
//! Each factory takes an `IoContext` and `UiUpdateSender` and returns
//! a struct containing all callbacks for that domain.

use std::future::Future;
use std::sync::Arc;

use crate::tui::components::copy_to_clipboard;
use crate::tui::components::ToastMessage;
use crate::tui::context::IoContext;
use crate::tui::effects::{EffectCommand, OpResponse};
use crate::tui::semantic_lifecycle::{SemanticOperationTransferScope, SubmittedOperationOwner};
use crate::tui::types::{AccessLevel, MfaPolicy};
use crate::tui::updates::{UiOperation, UiUpdate, UiUpdateSender};
use aura_app::ui::types::InvitationBridgeType;
use aura_app::ui::workflows::invitation::import_invitation_details;
use aura_app::ui::workflows::semantic_facts::publish_authoritative_operation_failure_with_instance;
use aura_app::ui_contract::{
    ChannelFactKey, InvitationFactKind, OperationId, OperationState, RuntimeEventKind, RuntimeFact,
    SemanticFailureCode, SemanticFailureDomain, SemanticOperationError, SemanticOperationKind,
};
use aura_core::types::identifiers::CeremonyId;
use aura_core::AuthorityId;
use futures::FutureExt;

use super::types::*;

const ACCEPT_PENDING_CHANNEL_INVITATION_CALLBACK_TIMEOUT_MS: u64 = 4_000;

#[allow(clippy::needless_pass_by_value)] // Arc clone pattern for task spawning
fn spawn_ctx<F>(ctx: Arc<IoContext>, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    ctx.tasks().spawn(fut);
}

use crate::tui::updates::{send_ui_update_lossy, send_ui_update_required};

async fn send_ui_update_reliable(tx: &UiUpdateSender, update: UiUpdate) {
    send_ui_update_required(tx, update).await;
}

fn enqueue_ui_update_required(ctx: Arc<IoContext>, tx: UiUpdateSender, update: UiUpdate) {
    spawn_ctx(ctx, async move {
        send_ui_update_required(&tx, update).await;
    });
}

fn invitation_import_runtime_fact_update(
    invitation: Option<&aura_app::ui::types::InvitationInfo>,
) -> Option<UiUpdate> {
    let invitation = invitation?;
    if matches!(
        invitation.invitation_type,
        InvitationBridgeType::Contact { .. }
    ) {
        Some(UiUpdate::RuntimeFactsUpdated {
            replace_kinds: vec![RuntimeEventKind::InvitationAccepted],
            facts: vec![RuntimeFact::InvitationAccepted {
                invitation_kind: InvitationFactKind::Contact,
                authority_id: Some(invitation.sender_id.to_string()),
                operation_state: Some(OperationState::Succeeded),
            }],
        })
    } else {
        None
    }
}

fn invitation_import_success_updates(
    code: &str,
    invitation: Option<&aura_app::ui::types::InvitationInfo>,
) -> Vec<UiUpdate> {
    let mut updates = vec![UiUpdate::InvitationImported {
        invitation_code: code.to_string(),
    }];
    if let Some(update) = invitation_import_runtime_fact_update(invitation) {
        updates.push(update);
    }
    updates
}

async fn run_invitation_import_flow(
    ctx: Arc<IoContext>,
    tx: UiUpdateSender,
    code: String,
    operation: SubmittedOperationOwner,
) {
    let app_core = ctx.app_core_raw().clone();
    let invitation = import_invitation_details(&app_core, &code).await.ok();
    match ctx
        .dispatch(EffectCommand::ImportInvitation { code: code.clone() })
        .await
    {
        Ok(_) => {
            let _ =
                operation.relinquish_to_workflow(SemanticOperationTransferScope::InvitationImport);
            for update in invitation_import_success_updates(&code, invitation.as_ref()) {
                send_ui_update_required(&tx, update).await;
            }
        }
        Err(error) => {
            tracing::error!(error = %error, "ImportInvitation dispatch failed");
            operation.fail(error.to_string()).await;
            send_ui_update_required(
                &tx,
                UiUpdate::ToastAdded(ToastMessage::error(
                    "invitation",
                    format!("Import invitation failed: {error}"),
                )),
            )
            .await;
        }
    }
}

fn enqueue_invalid_lan_authority_toast(
    ctx: Arc<IoContext>,
    tx: UiUpdateSender,
    authority_id: String,
    error: String,
) {
    enqueue_ui_update_required(
        ctx,
        tx,
        UiUpdate::ToastAdded(ToastMessage::error(
            "lan",
            format!("Invalid authority id '{authority_id}': {error}"),
        )),
    );
}

#[derive(Clone, Copy)]
enum CommandOutcomeStatus {
    Ok,
    Denied,
    Invalid,
    Failed,
}

impl CommandOutcomeStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Denied => "denied",
            Self::Invalid => "invalid",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy)]
enum CommandReasonCode {
    None,
    MissingActiveContext,
    PermissionDenied,
    NotMember,
    NotFound,
    InvalidArgument,
    InvalidState,
    Muted,
    Banned,
    Internal,
}

impl CommandReasonCode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::MissingActiveContext => "missing_active_context",
            Self::PermissionDenied => "permission_denied",
            Self::NotMember => "not_member",
            Self::NotFound => "not_found",
            Self::InvalidArgument => "invalid_argument",
            Self::InvalidState => "invalid_state",
            Self::Muted => "muted",
            Self::Banned => "banned",
            Self::Internal => "internal",
        }
    }
}

fn classify_command_error(message: &str) -> (CommandOutcomeStatus, CommandReasonCode) {
    let lower = message.to_ascii_lowercase();

    if lower.contains("no active home selected")
        || lower.contains("missing current channel")
        || lower.contains("missing channel scope")
    {
        return (
            CommandOutcomeStatus::Invalid,
            CommandReasonCode::MissingActiveContext,
        );
    }
    if lower.contains("permission denied")
        || (lower.contains("requires") && lower.contains("capability"))
    {
        return (
            CommandOutcomeStatus::Denied,
            CommandReasonCode::PermissionDenied,
        );
    }
    if lower.contains("cannot create one_hop_link from home")
        || lower.contains("only members can be designated as moderators")
        || lower.contains("only moderators")
        || lower.contains("requires a moderator home")
    {
        return (
            CommandOutcomeStatus::Denied,
            CommandReasonCode::PermissionDenied,
        );
    }
    if lower.contains("not a member") {
        return (CommandOutcomeStatus::Denied, CommandReasonCode::NotMember);
    }
    if lower.contains("muted") {
        return (CommandOutcomeStatus::Denied, CommandReasonCode::Muted);
    }
    if lower.contains("banned") || lower.contains("ban ") {
        return (CommandOutcomeStatus::Denied, CommandReasonCode::Banned);
    }
    if lower.contains("unknown")
        || lower.contains("not found")
        || lower.contains("missing target")
        || lower.contains("unknown channel scope")
    {
        return (CommandOutcomeStatus::Invalid, CommandReasonCode::NotFound);
    }
    if lower.contains("parse error")
        || lower.contains("invalid argument")
        || lower.contains("missing required argument")
        || lower.contains("invalid ")
    {
        return (
            CommandOutcomeStatus::Invalid,
            CommandReasonCode::InvalidArgument,
        );
    }
    if lower.contains("stale snapshot") || lower.contains("invalid state") {
        return (
            CommandOutcomeStatus::Failed,
            CommandReasonCode::InvalidState,
        );
    }

    (CommandOutcomeStatus::Failed, CommandReasonCode::Internal)
}

fn command_outcome_message(
    message: impl Into<String>,
    status: CommandOutcomeStatus,
    reason: CommandReasonCode,
    consistency: Option<&str>,
) -> String {
    let metadata = format!(
        "[s={} r={} c={}]",
        status.as_str(),
        reason.as_str(),
        consistency.unwrap_or("none")
    );
    let message = message.into();
    if message.is_empty() {
        metadata
    } else {
        format!("{metadata} {message}")
    }
}

// =============================================================================
// Chat Callbacks
// =============================================================================

/// All callbacks for the chat screen
#[derive(Clone)]
pub struct ChatCallbacks {
    pub on_send: SendCallback,
    pub(crate) on_accept_pending_channel_invitation: NoArgOwnedCallback,
    pub on_join_channel: JoinChannelCallback,
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
    fn make_accept_pending_channel_invitation(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> NoArgOwnedCallback {
        Arc::new(move |operation| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let app_core = ctx.app_core_raw().clone();
            let operation_instance_id = operation.harness_handle().instance_id.clone();
            spawn_ctx(ctx.clone(), async move {
                let _ = operation.relinquish_to_workflow(
                    SemanticOperationTransferScope::AcceptPendingChannelInvitation,
                );
                let accept = std::panic::AssertUnwindSafe(
                    aura_app::ui::workflows::invitation::accept_pending_home_invitation_with_instance(
                        &app_core,
                        Some(operation_instance_id.clone()),
                    ),
                )
                .catch_unwind();
                match tokio::time::timeout(
                    std::time::Duration::from_millis(
                        ACCEPT_PENDING_CHANNEL_INVITATION_CALLBACK_TIMEOUT_MS,
                    ),
                    accept,
                )
                .await
                {
                    Ok(Ok(Ok(_))) => {}
                    Ok(Ok(Err(error))) => {
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::error(
                                "invitation",
                                format!("Accept pending invitation failed: {error}"),
                            )),
                        )
                        .await;
                    }
                    Ok(Err(panic)) => {
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
                        let _ = publish_authoritative_operation_failure_with_instance(
                            &app_core,
                            OperationId::invitation_accept(),
                            Some(operation_instance_id.clone()),
                            SemanticOperationKind::AcceptPendingChannelInvitation,
                            SemanticOperationError::new(
                                SemanticFailureDomain::Command,
                                SemanticFailureCode::InternalError,
                            )
                            .with_detail(detail.clone()),
                        )
                        .await;
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::error("invitation", detail)),
                        )
                        .await;
                    }
                    Err(_) => {
                        let detail = format!(
                            "accept_pending_channel_invitation callback timed out after {}ms",
                            ACCEPT_PENDING_CHANNEL_INVITATION_CALLBACK_TIMEOUT_MS
                        );
                        let _ = publish_authoritative_operation_failure_with_instance(
                            &app_core,
                            OperationId::invitation_accept(),
                            Some(operation_instance_id),
                            SemanticOperationKind::AcceptPendingChannelInvitation,
                            SemanticOperationError::new(
                                SemanticFailureDomain::Invitation,
                                SemanticFailureCode::OperationTimedOut,
                            )
                            .with_detail(detail.clone()),
                        )
                        .await;
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
        Arc::new(move |channel_id: String, content: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let strong_resolver = strong_resolver.clone();
            let app_core = ctx.app_core_raw().clone();
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
                            let (status, reason) = classify_command_error(&e.to_string());
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
                                    let (status, reason) = classify_command_error(&e.to_string());
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
                                    let (status, reason) = classify_command_error(&e.to_string());
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
                                    let (status, reason) = classify_command_error(&e.to_string());
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
                let cmd = EffectCommand::SendMessage {
                    channel: channel_id_clone.clone(),
                    content: content_clone.clone(),
                };

                send_ui_update_reliable(
                    &tx,
                    UiUpdate::MessageSent {
                        channel: channel_id_clone.clone(),
                        content: content_clone.clone(),
                    },
                )
                .await;

                match ctx.dispatch(cmd).await {
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
        Arc::new(move |channel_name: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            spawn_ctx(ctx.clone(), async move {
                let cmd = EffectCommand::JoinChannel {
                    channel: channel_name.clone(),
                };
                match ctx.dispatch(cmd).await {
                    Ok(_) => {}
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

// =============================================================================
// Contacts Callbacks
// =============================================================================

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
                  operation: Option<SubmittedOperationOwner>| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                let operation_instance_id = operation
                    .as_ref()
                    .map(|operation| operation.harness_handle().instance_id.clone());
                let app_core = ctx.app_core_raw().clone();
                spawn_ctx(ctx.clone(), async move {
                    if let Some(operation) = operation {
                        let _ = operation.relinquish_to_workflow(
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
                    enqueue_invalid_lan_authority_toast(
                        ctx.clone(),
                        tx.clone(),
                        authority_id,
                        error.to_string(),
                    );
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

// =============================================================================
// Invitations Callbacks
// =============================================================================

/// All callbacks for the invitations screen
#[derive(Clone)]
pub struct InvitationsCallbacks {
    pub on_accept: InvitationCallback,
    pub on_decline: InvitationCallback,
    pub on_revoke: InvitationCallback,
    pub(crate) on_create: CreateInvitationCallback,
    pub on_export: ExportInvitationCallback,
    pub(crate) on_import: ImportInvitationOwnedCallback,
}

impl InvitationsCallbacks {
    #[must_use]
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_accept: Self::make_accept(ctx.clone(), tx.clone()),
            on_decline: Self::make_decline(ctx.clone(), tx.clone()),
            on_revoke: Self::make_revoke(ctx.clone(), tx.clone()),
            on_create: Self::make_create(ctx.clone(), tx.clone()),
            on_export: Self::make_export(ctx.clone(), tx.clone()),
            on_import: Self::make_import(ctx, tx),
        }
    }

    fn make_accept(ctx: Arc<IoContext>, tx: UiUpdateSender) -> InvitationCallback {
        Arc::new(move |invitation_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let inv_id = invitation_id.clone();
            let app_core = ctx.app_core_raw().clone();
            let cmd = EffectCommand::AcceptInvitation { invitation_id };
            spawn_ctx(ctx.clone(), async move {
                let accepted_invitation = {
                    let core = app_core.read().await;
                    core.snapshot().invitations.invitation(&inv_id).cloned()
                };
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::InvitationAccepted {
                                invitation_id: inv_id.clone(),
                            },
                        )
                        .await;
                        let mut runtime_facts = Vec::new();
                        let replace_kinds = vec![RuntimeEventKind::InvitationAccepted];
                        if let Some(invitation) = accepted_invitation.as_ref() {
                            let invitation_kind = if matches!(
                                invitation.invitation_type,
                                aura_app::ui::types::InvitationType::Home
                            ) {
                                InvitationFactKind::Contact
                            } else {
                                InvitationFactKind::Generic
                            };
                            runtime_facts.push(RuntimeFact::InvitationAccepted {
                                invitation_kind,
                                authority_id: Some(invitation.from_id.to_string()),
                                operation_state: Some(OperationState::Succeeded),
                            });
                        }
                        if !runtime_facts.is_empty() {
                            send_ui_update_reliable(
                                &tx,
                                UiUpdate::RuntimeFactsUpdated {
                                    replace_kinds,
                                    facts: runtime_facts,
                                },
                            )
                            .await;
                        }
                    }
                    Err(_error) => {}
                }
            });
        })
    }

    fn make_decline(ctx: Arc<IoContext>, tx: UiUpdateSender) -> InvitationCallback {
        Arc::new(move |invitation_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let inv_id = invitation_id.clone();
            let cmd = EffectCommand::DeclineInvitation { invitation_id };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::InvitationDeclined {
                                invitation_id: inv_id,
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

    fn make_revoke(ctx: Arc<IoContext>, _tx: UiUpdateSender) -> InvitationCallback {
        Arc::new(move |invitation_id: String| {
            let ctx = ctx.clone();
            let cmd = EffectCommand::CancelInvitation { invitation_id };
            spawn_ctx(ctx.clone(), async move {
                let _ = ctx.dispatch(cmd).await;
            });
        })
    }

    fn make_create(ctx: Arc<IoContext>, tx: UiUpdateSender) -> CreateInvitationCallback {
        Arc::new(
            move |receiver_id: AuthorityId,
                  invitation_type: String,
                  message: Option<String>,
                  ttl_secs: Option<u64>,
                  operation: Option<SubmittedOperationOwner>| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                spawn_ctx(ctx.clone(), async move {
                    let result = ctx
                        .create_invitation_code(receiver_id, &invitation_type, message, ttl_secs)
                        .await;
                    match result {
                        Ok(code) => {
                            if let Some(operation) = operation {
                                operation.succeed().await;
                            }
                            let _ = copy_to_clipboard(&code);
                            send_ui_update_reliable(&tx, UiUpdate::InvitationExported { code })
                                .await;
                        }
                        Err(e) => {
                            if let Some(operation) = operation {
                                operation.fail(e.to_string()).await;
                            }
                            send_ui_update_reliable(
                                &tx,
                                UiUpdate::ToastAdded(ToastMessage::error(
                                    "invitation",
                                    format!("Create invitation failed: {e}"),
                                )),
                            )
                            .await;
                        }
                    }
                });
            },
        )
    }

    fn make_export(ctx: Arc<IoContext>, tx: UiUpdateSender) -> ExportInvitationCallback {
        Arc::new(move |invitation_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            spawn_ctx(ctx.clone(), async move {
                match ctx.export_invitation_code(&invitation_id).await {
                    Ok(code) => {
                        let _ = copy_to_clipboard(&code);
                        send_ui_update_reliable(&tx, UiUpdate::InvitationExported { code }).await;
                    }
                    Err(_e) => {
                        tracing::debug!(error = %_e, "dispatch error (surfaced via ERROR_SIGNAL)");
                    }
                }
            });
        })
    }

    fn make_import(ctx: Arc<IoContext>, tx: UiUpdateSender) -> ImportInvitationOwnedCallback {
        Arc::new(move |code: String, operation: SubmittedOperationOwner| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            spawn_ctx(ctx.clone(), async move {
                run_invitation_import_flow(ctx, tx, code, operation).await;
            });
        })
    }
}

// =============================================================================
// Recovery Callbacks
// =============================================================================

/// All callbacks for the recovery screen
#[derive(Clone)]
pub struct RecoveryCallbacks {
    pub on_start_recovery: RecoveryCallback,
    pub on_add_guardian: RecoveryCallback,
    pub on_select_guardian: GuardianSelectCallback,
    pub on_submit_approval: ApprovalCallback,
}

impl RecoveryCallbacks {
    #[must_use]
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_start_recovery: Self::make_start_recovery(ctx.clone(), tx.clone()),
            on_add_guardian: Self::make_add_guardian(ctx.clone(), tx.clone()),
            on_select_guardian: Self::make_select_guardian(ctx.clone(), tx.clone()),
            on_submit_approval: Self::make_submit_approval(ctx, tx),
        }
    }

    fn make_start_recovery(ctx: Arc<IoContext>, tx: UiUpdateSender) -> RecoveryCallback {
        Arc::new(move || {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::StartRecovery;
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(&tx, UiUpdate::RecoveryStarted).await;
                    }
                    Err(_e) => {
                        tracing::debug!(error = %_e, "dispatch error (surfaced via ERROR_SIGNAL)");
                    }
                }
            });
        })
    }

    fn make_add_guardian(ctx: Arc<IoContext>, tx: UiUpdateSender) -> RecoveryCallback {
        Arc::new(move || {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::InviteGuardian { contact_id: None };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::GuardianAdded {
                                contact_id: "unknown".to_string(),
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

    fn make_select_guardian(ctx: Arc<IoContext>, tx: UiUpdateSender) -> GuardianSelectCallback {
        Arc::new(move |contact_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let contact_id_clone = contact_id.clone();
            let cmd = EffectCommand::InviteGuardian {
                contact_id: Some(contact_id),
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::GuardianSelected {
                                contact_id: contact_id_clone,
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

    fn make_submit_approval(ctx: Arc<IoContext>, tx: UiUpdateSender) -> ApprovalCallback {
        Arc::new(move |request_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let request_id_clone = request_id.clone();
            let cmd = EffectCommand::SubmitGuardianApproval {
                guardian_id: request_id,
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ApprovalSubmitted {
                                request_id: request_id_clone,
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

// =============================================================================
// Settings Callbacks
// =============================================================================

/// All callbacks for the settings screen
#[derive(Clone)]
pub struct SettingsCallbacks {
    pub on_update_mfa: Arc<dyn Fn(MfaPolicy) + Send + Sync>,
    pub on_update_nickname_suggestion: UpdateNicknameSuggestionCallback,
    pub on_update_threshold: UpdateThresholdCallback,
    pub on_add_device: AddDeviceCallback,
    pub on_remove_device: RemoveDeviceCallback,
    pub on_import_device_enrollment_on_mobile: ImportDeviceEnrollmentCallback,
}

impl SettingsCallbacks {
    #[must_use]
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_update_mfa: Self::make_update_mfa(ctx.clone(), tx.clone()),
            on_update_nickname_suggestion: Self::make_update_nickname_suggestion(
                ctx.clone(),
                tx.clone(),
            ),
            on_update_threshold: Self::make_update_threshold(ctx.clone(), tx.clone()),
            on_add_device: Self::make_add_device(ctx.clone(), tx.clone()),
            on_remove_device: Self::make_remove_device(ctx.clone(), tx.clone()),
            on_import_device_enrollment_on_mobile: Self::make_import_device_enrollment(ctx, tx),
        }
    }

    fn make_update_mfa(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> Arc<dyn Fn(MfaPolicy) + Send + Sync> {
        Arc::new(move |policy: MfaPolicy| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::UpdateMfaPolicy {
                require_mfa: policy.requires_mfa(),
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(&tx, UiUpdate::MfaPolicyChanged(policy)).await;
                    }
                    Err(_e) => {
                        tracing::debug!(error = %_e, "dispatch error (surfaced via ERROR_SIGNAL)");
                    }
                }
            });
        })
    }

    fn make_update_nickname_suggestion(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> UpdateNicknameSuggestionCallback {
        Arc::new(move |name: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let name_clone = name.clone();
            let cmd = EffectCommand::UpdateNickname { name };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::NicknameSuggestionChanged(name_clone),
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

    fn make_update_threshold(ctx: Arc<IoContext>, tx: UiUpdateSender) -> UpdateThresholdCallback {
        Arc::new(move |threshold_k: u8, threshold_n: u8| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let config = match crate::tui::effects::ThresholdConfig::new(threshold_k, threshold_n) {
                Ok(config) => config,
                Err(error) => {
                    enqueue_ui_update_required(
                        ctx.clone(),
                        tx.clone(),
                        UiUpdate::operation_failed(
                            UiOperation::UpdateThreshold,
                            crate::error::TerminalError::Input(error),
                        ),
                    );
                    return;
                }
            };
            let cmd = EffectCommand::UpdateThreshold { config };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ThresholdChanged {
                                k: threshold_k,
                                n: threshold_n,
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

    fn make_add_device(ctx: Arc<IoContext>, tx: UiUpdateSender) -> AddDeviceCallback {
        Arc::new(
            move |nickname_suggestion: String, invitee_authority_id: Option<AuthorityId>| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                spawn_ctx(ctx.clone(), async move {
                    let start = match ctx
                        .start_device_enrollment(&nickname_suggestion, invitee_authority_id)
                        .await
                    {
                        Ok(start) => start,
                        Err(error) => {
                            send_ui_update_reliable(
                                &tx,
                                UiUpdate::ToastAdded(ToastMessage::error(
                                    "devices",
                                    format!("Start device enrollment failed: {error}"),
                                )),
                            )
                            .await;
                            return;
                        }
                    };

                    send_ui_update_reliable(
                        &tx,
                        UiUpdate::DeviceEnrollmentStarted {
                            ceremony_id: start.ceremony_id.clone(),
                            nickname_suggestion: nickname_suggestion.clone(),
                            enrollment_code: start.enrollment_code.clone(),
                            pending_epoch: start.pending_epoch,
                            device_id: start.device_id.clone(),
                        },
                    )
                    .await;

                    // Prime status quickly (best-effort) so the modal has counters immediately.
                    let ceremony_id_typed = CeremonyId::new(start.ceremony_id.clone());
                    if let Ok(status) =
                        aura_app::ui::workflows::ceremonies::get_key_rotation_ceremony_status(
                            ctx.app_core_raw(),
                            &ceremony_id_typed,
                        )
                        .await
                    {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::KeyRotationCeremonyStatus {
                                ceremony_id: status.ceremony_id.to_string(),
                                kind: status.kind,
                                accepted_count: status.accepted_count,
                                total_count: status.total_count,
                                threshold: status.threshold,
                                is_complete: status.is_complete,
                                has_failed: status.has_failed,
                                accepted_participants: status.accepted_participants.clone(),
                                error_message: status.error_message.clone(),
                                pending_epoch: status.pending_epoch,
                                agreement_mode: status.agreement_mode,
                                reversion_risk: status.reversion_risk,
                            },
                        )
                        .await;
                    }

                    let app = ctx.app_core_raw().clone();
                    let tx_monitor = tx.clone();
                    let ceremony_id = CeremonyId::new(start.ceremony_id.clone());
                    spawn_ctx(ctx.clone(), async move {
                        let _ = aura_app::ui::workflows::ceremonies::monitor_key_rotation_ceremony(
                            &app,
                            ceremony_id,
                            tokio::time::Duration::from_millis(500),
                            |status| {
                                let _ = send_ui_update_lossy(
                                    &tx_monitor,
                                    UiUpdate::KeyRotationCeremonyStatus {
                                        ceremony_id: status.ceremony_id.to_string(),
                                        kind: status.kind,
                                        accepted_count: status.accepted_count,
                                        total_count: status.total_count,
                                        threshold: status.threshold,
                                        is_complete: status.is_complete,
                                        has_failed: status.has_failed,
                                        accepted_participants: status.accepted_participants.clone(),
                                        error_message: status.error_message.clone(),
                                        pending_epoch: status.pending_epoch,
                                        agreement_mode: status.agreement_mode,
                                        reversion_risk: status.reversion_risk,
                                    },
                                );
                            },
                            tokio::time::sleep,
                        )
                        .await;
                    });
                });
            },
        )
    }

    fn make_remove_device(ctx: Arc<IoContext>, tx: UiUpdateSender) -> RemoveDeviceCallback {
        Arc::new(move |device_id| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let device_id_clone = device_id.to_string();

            spawn_ctx(ctx.clone(), async move {
                let ceremony_id = match ctx.start_device_removal(&device_id_clone).await {
                    Ok(id) => id,
                    Err(_e) => {
                        tracing::debug!(error = %_e, "dispatch error (surfaced via ERROR_SIGNAL)");
                        return;
                    }
                };

                send_ui_update_required(
                    &tx,
                    UiUpdate::ToastAdded(ToastMessage::info(
                        "device-removal-started",
                        "Device removal started",
                    )),
                )
                .await;

                #[cfg(feature = "development")]
                {
                    // In demo mode, make sure the simulated mobile device processes incoming
                    // threshold key packages so the removal ceremony can reach completion.
                    if device_id_clone == ctx.demo_mobile_device_id() {
                        let demo_ctx = ctx.clone();
                        spawn_ctx(ctx.clone(), async move {
                            for _ in 0..6 {
                                let _ = demo_ctx.process_demo_mobile_ceremony_acceptances().await;
                                tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
                            }
                        });
                    }
                }

                // Best-effort: monitor completion and toast success/failure.
                let app = ctx.app_core_raw().clone();
                let tx_monitor = tx.clone();
                spawn_ctx(ctx.clone(), async move {
                    match aura_app::ui::workflows::ceremonies::monitor_key_rotation_ceremony(
                        &app,
                        CeremonyId::new(ceremony_id),
                        tokio::time::Duration::from_millis(250),
                        |_| {},
                        tokio::time::sleep,
                    )
                    .await
                    {
                        Ok(status) if status.is_complete => {
                            send_ui_update_required(
                                &tx_monitor,
                                UiUpdate::ToastAdded(ToastMessage::success(
                                    "device-removal-complete",
                                    "Device removal complete",
                                )),
                            )
                            .await;
                        }
                        Ok(status) if status.has_failed => {
                            let msg = status
                                .error_message
                                .unwrap_or_else(|| "Device removal failed".to_string());
                            send_ui_update_required(
                                &tx_monitor,
                                UiUpdate::ToastAdded(ToastMessage::error(
                                    "device-removal-failed",
                                    msg,
                                )),
                            )
                            .await;
                        }
                        Ok(_) => {}
                        Err(_e) => {
                            // monitor already emitted error via ERROR_SIGNAL on polling failures.
                        }
                    }
                });
            });
        })
    }

    fn make_import_device_enrollment(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> ImportDeviceEnrollmentCallback {
        Arc::new(move |code: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let should_complete_onboarding = !ctx.has_account();
            spawn_ctx(ctx.clone(), async move {
                match ctx.import_device_enrollment_code(&code).await {
                    Ok(()) => {
                        if should_complete_onboarding {
                            send_ui_update_required(&tx, UiUpdate::AccountCreated).await;
                        }
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::success(
                                "devices",
                                "Device enrollment invitation accepted",
                            )),
                        )
                        .await;
                    }
                    Err(e) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::operation_failed(UiOperation::ImportDeviceEnrollmentCode, e),
                        )
                        .await;
                    }
                }
            });
        })
    }
}

// =============================================================================
// Neighborhood Callbacks
// =============================================================================

/// All callbacks for the neighborhood screen
#[derive(Clone)]
pub struct NeighborhoodCallbacks {
    pub on_enter_home: Arc<dyn Fn(String, AccessLevel) + Send + Sync>,
    pub on_go_home: GoHomeCallback,
    pub on_back_to_limited: GoHomeCallback,
    pub on_set_moderator: SetModeratorCallback,
    pub on_create_home: CreateHomeCallback,
    pub on_create_neighborhood: CreateNeighborhoodCallback,
    pub on_add_home_to_neighborhood: NeighborhoodHomeCallback,
    pub on_link_home_one_hop_link: NeighborhoodHomeCallback,
}

impl NeighborhoodCallbacks {
    #[must_use]
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_enter_home: Self::make_enter_home(ctx.clone(), tx.clone()),
            on_go_home: Self::make_go_home(ctx.clone(), tx.clone()),
            on_back_to_limited: Self::make_back_to_limited(ctx.clone(), tx.clone()),
            on_set_moderator: Self::make_set_moderator(ctx.clone(), tx.clone()),
            on_create_home: Self::make_create_home(ctx.clone(), tx.clone()),
            on_create_neighborhood: Self::make_create_neighborhood(ctx.clone(), tx.clone()),
            on_add_home_to_neighborhood: Self::make_add_home_to_neighborhood(
                ctx.clone(),
                tx.clone(),
            ),
            on_link_home_one_hop_link: Self::make_link_home_one_hop_link(ctx, tx),
        }
    }

    fn make_enter_home(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> Arc<dyn Fn(String, AccessLevel) + Send + Sync> {
        Arc::new(move |home_id: String, depth: AccessLevel| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let home_id_clone = home_id.clone();
            let depth_str = match depth {
                AccessLevel::Limited => "Limited",
                AccessLevel::Partial => "Partial",
                AccessLevel::Full => "Full",
            }
            .to_string();
            let cmd = EffectCommand::MovePosition {
                neighborhood_id: "current".to_string(),
                home_id,
                depth: depth_str,
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::HomeEntered {
                                home_id: home_id_clone,
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

    fn make_go_home(ctx: Arc<IoContext>, tx: UiUpdateSender) -> GoHomeCallback {
        Arc::new(move || {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::MovePosition {
                neighborhood_id: "current".to_string(),
                home_id: "home".to_string(),
                depth: "Full".to_string(),
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(&tx, UiUpdate::NavigatedHome).await;
                    }
                    Err(_e) => {
                        tracing::debug!(error = %_e, "dispatch error (surfaced via ERROR_SIGNAL)");
                    }
                }
            });
        })
    }

    fn make_back_to_limited(ctx: Arc<IoContext>, tx: UiUpdateSender) -> GoHomeCallback {
        Arc::new(move || {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::MovePosition {
                neighborhood_id: "current".to_string(),
                home_id: "current".to_string(),
                depth: "Limited".to_string(),
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(&tx, UiUpdate::NavigatedToLimited).await;
                    }
                    Err(_e) => {
                        tracing::debug!(error = %_e, "dispatch error (surfaced via ERROR_SIGNAL)");
                    }
                }
            });
        })
    }

    fn make_set_moderator(ctx: Arc<IoContext>, tx: UiUpdateSender) -> SetModeratorCallback {
        Arc::new(
            move |home_id: Option<String>, target_id: String, assign: bool| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                let cmd = if assign {
                    EffectCommand::GrantModerator {
                        channel: home_id,
                        target: target_id.clone(),
                    }
                } else {
                    EffectCommand::RevokeModerator {
                        channel: home_id,
                        target: target_id.clone(),
                    }
                };
                spawn_ctx(ctx.clone(), async move {
                    match ctx.dispatch(cmd).await {
                        Ok(_) => {
                            let action = if assign { "granted" } else { "revoked" };
                            send_ui_update_required(
                                &tx,
                                UiUpdate::ToastAdded(ToastMessage::success(
                                    "moderation",
                                    format!("Moderator designation {action} for {target_id}"),
                                )),
                            )
                            .await;
                        }
                        Err(e) => {
                            send_ui_update_required(
                                &tx,
                                UiUpdate::ToastAdded(ToastMessage::error(
                                    "moderation",
                                    format!("Failed to update moderator designation: {e}"),
                                )),
                            )
                            .await;
                        }
                    }
                });
            },
        )
    }

    fn make_create_home(ctx: Arc<IoContext>, tx: UiUpdateSender) -> CreateHomeCallback {
        Arc::new(move |name: String, _description: Option<String>| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let display_name = name.trim().to_string();
            let cmd = EffectCommand::CreateHome {
                name: Some(display_name.clone()),
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::success(
                                "home",
                                format!("Home '{display_name}' created"),
                            )),
                        )
                        .await;
                    }
                    Err(_error) => {}
                }
            });
        })
    }

    fn make_create_neighborhood(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> CreateNeighborhoodCallback {
        Arc::new(move |name: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let display_name = if name.trim().is_empty() {
                "Neighborhood".to_string()
            } else {
                name.trim().to_string()
            };
            let cmd = EffectCommand::CreateNeighborhood {
                name: display_name.clone(),
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::success(
                                "neighborhood",
                                format!("Neighborhood '{display_name}' ready"),
                            )),
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

    fn make_add_home_to_neighborhood(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> NeighborhoodHomeCallback {
        Arc::new(move |home_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::AddHomeToNeighborhood { home_id };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::success(
                                "neighborhood",
                                "Home added to neighborhood",
                            )),
                        )
                        .await;
                    }
                    Err(e) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::error(
                                "neighborhood",
                                format!("Failed to add home to neighborhood: {e}"),
                            )),
                        )
                        .await;
                    }
                }
            });
        })
    }

    fn make_link_home_one_hop_link(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> NeighborhoodHomeCallback {
        Arc::new(move |home_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::LinkHomeOneHopLink { home_id };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::success(
                                "neighborhood",
                                "OneHopLink linked",
                            )),
                        )
                        .await;
                    }
                    Err(e) => {
                        send_ui_update_required(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::error(
                                "neighborhood",
                                format!("Failed to link one_hop_link: {e}"),
                            )),
                        )
                        .await;
                    }
                }
            });
        })
    }
}

// =============================================================================
// App Callbacks (Global)
// =============================================================================

/// Global app callbacks (account setup, etc)
#[derive(Clone)]
pub struct AppCallbacks {
    pub(crate) on_create_account: CreateAccountCallback,
    pub on_import_device_enrollment_during_onboarding: ImportDeviceEnrollmentCallback,
}

impl AppCallbacks {
    #[must_use]
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_create_account: Self::make_create_account(ctx.clone(), tx.clone()),
            on_import_device_enrollment_during_onboarding:
                Self::make_import_device_enrollment_during_onboarding(ctx, tx),
        }
    }

    fn make_create_account(ctx: Arc<IoContext>, tx: UiUpdateSender) -> CreateAccountCallback {
        Arc::new(
            move |nickname_suggestion: String, operation: SubmittedOperationOwner| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                spawn_ctx(ctx.clone(), async move {
                    let account_result = std::panic::AssertUnwindSafe(async {
                        ctx.create_account(&nickname_suggestion).await
                    })
                    .catch_unwind()
                    .await;

                    match account_result {
                        Ok(Ok(())) => {
                            tracing::info!("tui create_account callback succeeded");
                            send_ui_update_reliable(
                                &tx,
                                UiUpdate::NicknameSuggestionChanged(nickname_suggestion.clone()),
                            )
                            .await;
                            operation.succeed().await;
                            send_ui_update_reliable(&tx, UiUpdate::AccountCreated).await;
                        }
                        Ok(Err(e)) => {
                            tracing::error!("tui create_account callback failed: {e}");
                            operation.fail(e.to_string()).await;
                            send_ui_update_reliable(
                                &tx,
                                UiUpdate::operation_failed(UiOperation::CreateAccount, e),
                            )
                            .await;
                        }
                        Err(_) => {
                            tracing::error!("tui create_account callback panicked");
                            operation.fail("panic in CreateAccount callback").await;
                            send_ui_update_reliable(
                                &tx,
                                UiUpdate::operation_failed(
                                    UiOperation::CreateAccount,
                                    crate::error::TerminalError::Operation(
                                        "panic in CreateAccount callback".to_string(),
                                    ),
                                ),
                            )
                            .await;
                        }
                    }
                });
            },
        )
    }

    fn make_import_device_enrollment_during_onboarding(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> ImportDeviceEnrollmentCallback {
        Arc::new(move |code: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            spawn_ctx(ctx.clone(), async move {
                match ctx.import_device_enrollment_code(&code).await {
                    Ok(()) => {
                        send_ui_update_reliable(&tx, UiUpdate::AccountCreated).await;
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::ToastAdded(ToastMessage::success(
                                "devices",
                                "Device enrollment invitation accepted",
                            )),
                        )
                        .await;
                    }
                    Err(e) => {
                        send_ui_update_reliable(
                            &tx,
                            UiUpdate::operation_failed(UiOperation::ImportDeviceEnrollmentCode, e),
                        )
                        .await;
                    }
                }
            });
        })
    }
}

// =============================================================================
// All Callbacks Registry
// =============================================================================

/// Registry containing all domain callbacks
#[derive(Clone)]
pub struct CallbackRegistry {
    pub chat: ChatCallbacks,
    pub contacts: ContactsCallbacks,
    pub invitations: InvitationsCallbacks,
    pub recovery: RecoveryCallbacks,
    pub settings: SettingsCallbacks,
    pub neighborhood: NeighborhoodCallbacks,
    pub app: AppCallbacks,
}

impl CallbackRegistry {
    /// Create all callbacks from context
    pub fn new(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
        app_core: Arc<async_lock::RwLock<aura_app::ui::types::AppCore>>,
    ) -> Self {
        Self {
            chat: ChatCallbacks::new(ctx.clone(), tx.clone(), app_core),
            contacts: ContactsCallbacks::new(ctx.clone(), tx.clone()),
            invitations: InvitationsCallbacks::new(ctx.clone(), tx.clone()),
            recovery: RecoveryCallbacks::new(ctx.clone(), tx.clone()),
            settings: SettingsCallbacks::new(ctx.clone(), tx.clone()),
            neighborhood: NeighborhoodCallbacks::new(ctx.clone(), tx.clone()),
            app: AppCallbacks::new(ctx, tx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_app::ui::types::{InvitationBridgeStatus, InvitationInfo};

    fn authority(value: &str) -> AuthorityId {
        value.parse().expect("valid authority id")
    }

    fn contact_invitation() -> InvitationInfo {
        InvitationInfo {
            invitation_id: "inv-contact".into(),
            sender_id: authority("authority-00000000-0000-0000-0000-000000000001"),
            receiver_id: authority("authority-00000000-0000-0000-0000-000000000002"),
            invitation_type: InvitationBridgeType::Contact { nickname: None },
            status: InvitationBridgeStatus::Pending,
            created_at_ms: 0,
            expires_at_ms: None,
            message: None,
        }
    }

    fn channel_invitation() -> InvitationInfo {
        InvitationInfo {
            invitation_id: "inv-channel".into(),
            sender_id: authority("authority-00000000-0000-0000-0000-000000000001"),
            receiver_id: authority("authority-00000000-0000-0000-0000-000000000002"),
            invitation_type: InvitationBridgeType::Channel {
                home_id: "home-test".to_string(),
                context_id: None,
                nickname_suggestion: None,
            },
            status: InvitationBridgeStatus::Pending,
            created_at_ms: 0,
            expires_at_ms: None,
            message: None,
        }
    }

    #[test]
    fn invitation_import_success_updates_emit_import_before_runtime_fact() {
        let updates = invitation_import_success_updates("code-123", Some(&contact_invitation()));

        assert!(matches!(
            updates.first(),
            Some(UiUpdate::InvitationImported { invitation_code })
                if invitation_code == "code-123"
        ));
        assert!(matches!(
            updates.get(1),
            Some(UiUpdate::RuntimeFactsUpdated { .. })
        ));
    }

    #[test]
    fn invitation_import_success_updates_skip_runtime_fact_for_non_contact_invites() {
        let updates = invitation_import_success_updates("code-456", Some(&channel_invitation()));

        assert_eq!(updates.len(), 1);
        assert!(matches!(
            updates.first(),
            Some(UiUpdate::InvitationImported { invitation_code })
                if invitation_code == "code-456"
        ));
    }
}
