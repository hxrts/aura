//! # Callback Factories
//!
//! Factory functions that create domain-specific callbacks.
//! Each factory takes an `IoContext` and `UiUpdateSender` and returns
//! a struct containing all callbacks for that domain.
//!
//! This eliminates the ~800 lines of callback creation in `run_app_with_context`.

use std::future::Future;
use std::sync::Arc;

use crate::tui::commands::{parse_command, IrcCommand};
use crate::tui::components::ToastMessage;
use crate::tui::context::IoContext;
use crate::tui::effects::EffectCommand;
use crate::tui::effects::{CapabilityPolicy, CommandDispatcher};
use crate::tui::types::{MfaPolicy, TraversalDepth};
use crate::tui::updates::{UiUpdate, UiUpdateSender};
use aura_core::identifiers::ChannelId;
use aura_core::effects::reactive::ReactiveEffects;
use aura_app::signal_defs::CHAT_SIGNAL;

use super::types::*;

fn spawn_ctx<F>(ctx: Arc<IoContext>, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    ctx.tasks().spawn(fut);
}

// =============================================================================
// Chat Callbacks
// =============================================================================

/// All callbacks for the chat screen
#[derive(Clone)]
pub struct ChatCallbacks {
    pub on_send: SendCallback,
    pub on_retry_message: RetryMessageCallback,
    pub on_channel_select: ChannelSelectCallback,
    pub on_create_channel: CreateChannelCallback,
    pub on_set_topic: SetTopicCallback,
    pub on_close_channel: IdCallback,
    pub on_list_participants: IdCallback,
}

impl ChatCallbacks {
    /// Create chat callbacks from context
    pub fn new(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
        app_core: Arc<async_lock::RwLock<aura_app::AppCore>>,
    ) -> Self {
        Self {
            on_send: Self::make_send(ctx.clone(), tx.clone()),
            on_retry_message: Self::make_retry_message(ctx.clone(), tx.clone()),
            on_channel_select: Self::make_channel_select(ctx.clone(), app_core, tx.clone()),
            on_create_channel: Self::make_create_channel(ctx.clone(), tx.clone()),
            on_set_topic: Self::make_set_topic(ctx.clone(), tx.clone()),
            on_close_channel: Self::make_close_channel(ctx.clone(), tx.clone()),
            on_list_participants: Self::make_list_participants(ctx, tx),
        }
    }
    fn make_send(ctx: Arc<IoContext>, tx: UiUpdateSender) -> SendCallback {
        Arc::new(move |channel_id: String, content: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let channel_id_clone = channel_id;
            let content_clone = content;

            spawn_ctx(ctx.clone(), async move {
                let trimmed = content_clone.trim_start();
                if trimmed.starts_with("/") {
                    // IRC-style command path
                    match parse_command(trimmed) {
                        Ok(IrcCommand::Help { .. }) => {
                            let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::info(
                                "help",
                                "Use ? for TUI help. Supported slash commands: /msg /me /nick /who /whois /join /leave /topic /invite /kick /ban /unban /mute /unmute /pin /unpin /op /deop /mode",
                            )));
                            return;
                        }
                        Ok(irc) => {
                            let mut dispatcher =
                                CommandDispatcher::with_policy(CapabilityPolicy::AllowAll);
                            dispatcher.set_current_channel(channel_id_clone.clone());

                            let effect = match dispatcher.dispatch(irc) {
                                Ok(cmd) => cmd,
                                Err(e) => {
                                    let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::error(
                                        "command",
                                        e.to_string(),
                                    )));
                                    return;
                                }
                            };

                            match effect {
                                EffectCommand::ListParticipants { channel } => {
                                    match aura_app::workflows::query::list_participants(
                                        ctx.app_core_raw(),
                                        &channel,
                                    )
                                    .await
                                    {
                                        Ok(list) => {
                                            let msg = if list.is_empty() {
                                                "No participants".to_string()
                                            } else {
                                                list.join(", ")
                                            };
                                            let _ = tx.try_send(UiUpdate::ToastAdded(
                                                ToastMessage::info("participants", msg),
                                            ));
                                        }
                                        Err(e) => {
                                            let _ = tx.try_send(UiUpdate::ToastAdded(
                                                ToastMessage::error("participants", e.to_string()),
                                            ));
                                        }
                                    }
                                }
                                EffectCommand::GetUserInfo { target } => {
                                    match aura_app::workflows::query::get_user_info(
                                        ctx.app_core_raw(),
                                        &target,
                                    )
                                    .await
                                    {
                                        Ok(contact) => {
                                            let id = contact.id.to_string();
                                            let name = if !contact.nickname.is_empty() {
                                                contact.nickname
                                            } else if let Some(s) = &contact.suggested_name {
                                                s.clone()
                                            } else {
                                                id.chars().take(8).collect::<String>() + "..."
                                            };
                                            let msg = format!("User: {name} ({id})");
                                            let _ = tx.try_send(UiUpdate::ToastAdded(
                                                ToastMessage::info("whois", msg),
                                            ));
                                        }
                                        Err(e) => {
                                            let _ = tx.try_send(UiUpdate::ToastAdded(
                                                ToastMessage::error("whois", e.to_string()),
                                            ));
                                        }
                                    }
                                }
                                _ => {
                                    let _ = ctx.dispatch(effect).await;
                                }
                            }
                            return;
                        }
                        Err(e) => {
                            let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::error(
                                "command",
                                e.to_string(),
                            )));
                            return;
                        }
                    }
                }

                // Normal message path
                let cmd = EffectCommand::SendMessage {
                    channel: channel_id_clone.clone(),
                    content: content_clone.clone(),
                };

                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.try_send(UiUpdate::MessageSent {
                            channel: channel_id_clone,
                            content: content_clone,
                        });
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
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
                            let _ = tx.try_send(UiUpdate::MessageRetried { message_id: msg_id });
                        }
                        Err(_e) => {
                            // Error already emitted to ERROR_SIGNAL by dispatch layer.
                        }
                    }
                });
            },
        )
    }

    fn make_channel_select(
        ctx: Arc<IoContext>,
        app_core: Arc<async_lock::RwLock<aura_app::AppCore>>,
        tx: UiUpdateSender,
    ) -> ChannelSelectCallback {
        Arc::new(move |channel_id: String| {
            let ctx = ctx.clone();
            let app_core = app_core.clone();
            let tx = tx.clone();
            let channel_id_clone = channel_id.clone();
            spawn_ctx(ctx, async move {
                // Parse channel_id string to ChannelId
                let channel_id_typed = channel_id.parse::<ChannelId>().ok();
                let core = app_core.read().await;
                core.views().select_channel(channel_id_typed);
                let reactive = core.reactive().clone();
                drop(core);

                if let Ok(mut chat_state) = reactive.read(&*CHAT_SIGNAL).await {
                    chat_state.select_channel(channel_id_typed);
                    let _ = reactive.emit(&*CHAT_SIGNAL, chat_state).await;
                }
                let _ = tx.try_send(UiUpdate::ChannelSelected(channel_id_clone));
            });
        })
    }

    fn make_list_participants(ctx: Arc<IoContext>, tx: UiUpdateSender) -> IdCallback {
        Arc::new(move |channel_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let channel_id_clone = channel_id;
            spawn_ctx(ctx.clone(), async move {
                match aura_app::workflows::query::list_participants(
                    ctx.app_core_raw(),
                    &channel_id_clone,
                )
                .await
                {
                    Ok(participants) => {
                        let _ = tx.try_send(UiUpdate::ChannelInfoParticipants {
                            channel_id: channel_id_clone,
                            participants,
                        });
                    }
                    Err(e) => {
                        let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::error(
                            "participants",
                            e.to_string(),
                        )));
                    }
                }
            });
        })
    }

    fn make_create_channel(ctx: Arc<IoContext>, tx: UiUpdateSender) -> CreateChannelCallback {
        Arc::new(
            move |name: String, topic: Option<String>, members: Vec<String>, threshold_k: u8| {
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
                    match ctx.dispatch(cmd).await {
                        Ok(_) => {
                            let _ = tx.try_send(UiUpdate::ChannelCreated(channel_name));
                        }
                        Err(_e) => {
                            // Error already emitted to ERROR_SIGNAL by dispatch layer.
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
                        let _ = tx.try_send(UiUpdate::TopicSet {
                            channel: ch,
                            topic: t,
                        });
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
                    }
                }
            });
        })
    }

    fn make_close_channel(ctx: Arc<IoContext>, tx: UiUpdateSender) -> IdCallback {
        Arc::new(move |channel_id: String| {
            let ctx = ctx.clone();
            let _tx = tx.clone();
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
    pub on_import_invitation: ImportInvitationCallback,
    pub on_invite_lan_peer: Arc<dyn Fn(String, String) + Send + Sync>,
    pub on_remove_contact: IdCallback,
}

impl ContactsCallbacks {
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_update_nickname: Self::make_update_nickname(ctx.clone(), tx.clone()),
            on_start_chat: Self::make_start_chat(ctx.clone(), tx.clone()),
            on_import_invitation: Self::make_import_invitation(ctx.clone(), tx.clone()),
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
                        let _ = tx.try_send(UiUpdate::NicknameUpdated {
                            contact_id: contact_id_clone,
                            nickname: nickname_clone,
                        });
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
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
                        let _ = tx.try_send(UiUpdate::ChatStarted {
                            contact_id: contact_id_clone,
                        });
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
                    }
                }
            });
        })
    }

    fn make_import_invitation(ctx: Arc<IoContext>, tx: UiUpdateSender) -> ImportInvitationCallback {
        Arc::new(move |code: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let code_clone = code.clone();
            let cmd = EffectCommand::ImportInvitation { code };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.try_send(UiUpdate::InvitationImported {
                            invitation_code: code_clone,
                        });
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
                    }
                }
            });
        })
    }

    fn make_remove_contact(ctx: Arc<IoContext>, tx: UiUpdateSender) -> IdCallback {
        Arc::new(move |contact_id: String| {
            let ctx = ctx.clone();
            let _tx = tx.clone();
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
            let cmd = EffectCommand::InviteLanPeer {
                authority_id,
                address,
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        ctx.mark_peer_invited(&authority_id_clone).await;
                        let _ = tx.try_send(UiUpdate::LanPeerInvited {
                            peer_id: authority_id_clone,
                        });
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
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
    pub on_create: CreateInvitationCallback,
    pub on_export: ExportInvitationCallback,
    pub on_import: ImportInvitationCallback,
}

impl InvitationsCallbacks {
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
            let cmd = EffectCommand::AcceptInvitation { invitation_id };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.try_send(UiUpdate::InvitationAccepted {
                            invitation_id: inv_id,
                        });
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
                    }
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
                        let _ = tx.try_send(UiUpdate::InvitationDeclined {
                            invitation_id: inv_id,
                        });
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
                    }
                }
            });
        })
    }

    fn make_revoke(ctx: Arc<IoContext>, tx: UiUpdateSender) -> InvitationCallback {
        Arc::new(move |invitation_id: String| {
            let ctx = ctx.clone();
            let _tx = tx.clone();
            let cmd = EffectCommand::CancelInvitation { invitation_id };
            spawn_ctx(ctx.clone(), async move {
                let _ = ctx.dispatch(cmd).await;
            });
        })
    }

    fn make_create(ctx: Arc<IoContext>, tx: UiUpdateSender) -> CreateInvitationCallback {
        Arc::new(
            move |receiver_id: String,
                  invitation_type: String,
                  message: Option<String>,
                  ttl_secs: Option<u64>| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                spawn_ctx(ctx.clone(), async move {
                    match ctx
                        .create_invitation_code(&receiver_id, &invitation_type, message, ttl_secs)
                        .await
                    {
                        Ok(code) => {
                            let _ = tx.try_send(UiUpdate::InvitationExported { code });
                        }
                        Err(_e) => {}
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
                        let _ = tx.try_send(UiUpdate::InvitationExported { code });
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
                    }
                }
            });
        })
    }

    fn make_import(ctx: Arc<IoContext>, tx: UiUpdateSender) -> ImportInvitationCallback {
        Arc::new(move |code: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let code_clone = code.clone();
            let cmd = EffectCommand::ImportInvitation { code };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.try_send(UiUpdate::InvitationImported {
                            invitation_code: code_clone,
                        });
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
                    }
                }
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
                        let _ = tx.try_send(UiUpdate::RecoveryStarted);
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
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
                        let _ = tx.try_send(UiUpdate::GuardianAdded {
                            contact_id: "unknown".to_string(),
                        });
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
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
                        let _ = tx.try_send(UiUpdate::GuardianSelected {
                            contact_id: contact_id_clone,
                        });
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
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
                        let _ = tx.try_send(UiUpdate::ApprovalSubmitted {
                            request_id: request_id_clone,
                        });
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
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
    pub on_update_display_name: UpdateDisplayNameCallback,
    pub on_update_threshold: UpdateThresholdCallback,
    pub on_add_device: AddDeviceCallback,
    pub on_remove_device: RemoveDeviceCallback,
    pub on_import_device_enrollment_on_mobile: ImportDeviceEnrollmentCallback,
}

impl SettingsCallbacks {
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_update_mfa: Self::make_update_mfa(ctx.clone(), tx.clone()),
            on_update_display_name: Self::make_update_display_name(ctx.clone(), tx.clone()),
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
                        let _ = tx.try_send(UiUpdate::MfaPolicyChanged(policy));
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
                    }
                }
            });
        })
    }

    fn make_update_display_name(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> UpdateDisplayNameCallback {
        Arc::new(move |name: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let name_clone = name.clone();
            let cmd = EffectCommand::UpdateNickname { name: name };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.try_send(UiUpdate::DisplayNameChanged(name_clone));
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
                    }
                }
            });
        })
    }

    fn make_update_threshold(ctx: Arc<IoContext>, tx: UiUpdateSender) -> UpdateThresholdCallback {
        Arc::new(move |threshold_k: u8, threshold_n: u8| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::UpdateThreshold {
                threshold_k,
                threshold_n,
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.try_send(UiUpdate::ThresholdChanged {
                            k: threshold_k,
                            n: threshold_n,
                        });
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
                    }
                }
            });
        })
    }

    fn make_add_device(ctx: Arc<IoContext>, tx: UiUpdateSender) -> AddDeviceCallback {
        Arc::new(move |device_name: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            spawn_ctx(ctx.clone(), async move {
                let start = match ctx.start_device_enrollment(&device_name).await {
                    Ok(start) => start,
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by operational layer.
                        return;
                    }
                };

                let _ = tx.try_send(UiUpdate::DeviceEnrollmentStarted {
                    ceremony_id: start.ceremony_id.clone(),
                    device_name: device_name.clone(),
                    enrollment_code: start.enrollment_code.clone(),
                    pending_epoch: start.pending_epoch,
                    device_id: start.device_id.clone(),
                });

                // Prime status quickly (best-effort) so the modal has counters immediately.
                if let Ok(status) =
                    aura_app::workflows::ceremonies::get_key_rotation_ceremony_status(
                        ctx.app_core_raw(),
                        &start.ceremony_id,
                    )
                    .await
                {
                    let _ = tx.try_send(UiUpdate::KeyRotationCeremonyStatus {
                        ceremony_id: status.ceremony_id.clone(),
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
                    });
                }

                let app = ctx.app_core_raw().clone();
                let tx_monitor = tx.clone();
                let ceremony_id = start.ceremony_id.clone();
                spawn_ctx(ctx.clone(), async move {
                    let _ = aura_app::workflows::ceremonies::monitor_key_rotation_ceremony(
                        &app,
                        ceremony_id,
                        tokio::time::Duration::from_millis(500),
                        |status| {
                            let _ = tx_monitor.try_send(UiUpdate::KeyRotationCeremonyStatus {
                                ceremony_id: status.ceremony_id.clone(),
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
                            });
                        },
                        tokio::time::sleep,
                    )
                    .await;
                });
            });
        })
    }

    fn make_remove_device(ctx: Arc<IoContext>, tx: UiUpdateSender) -> RemoveDeviceCallback {
        Arc::new(move |device_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let device_id_clone = device_id;

            spawn_ctx(ctx.clone(), async move {
                let ceremony_id = match ctx.start_device_removal(&device_id_clone).await {
                    Ok(id) => id,
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by operational layer.
                        return;
                    }
                };

                let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::info(
                    "device-removal-started",
                    "Device removal started",
                )));

                // Best-effort: monitor completion and toast success/failure.
                let app = ctx.app_core_raw().clone();
                let tx_monitor = tx.clone();
                spawn_ctx(ctx.clone(), async move {
                    match aura_app::workflows::ceremonies::monitor_key_rotation_ceremony(
                        &app,
                        ceremony_id,
                        tokio::time::Duration::from_millis(250),
                        |_| {},
                        tokio::time::sleep,
                    )
                    .await
                    {
                        Ok(status) if status.is_complete => {
                            let _ =
                                tx_monitor.try_send(UiUpdate::ToastAdded(ToastMessage::success(
                                    "device-removal-complete",
                                    "Device removal complete",
                                )));
                        }
                        Ok(status) if status.has_failed => {
                            let msg = status
                                .error_message
                                .unwrap_or_else(|| "Device removal failed".to_string());
                            let _ = tx_monitor.try_send(UiUpdate::ToastAdded(ToastMessage::error(
                                "device-removal-failed",
                                msg,
                            )));
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

    #[cfg(feature = "development")]
    fn make_import_device_enrollment(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> ImportDeviceEnrollmentCallback {
        Arc::new(move |code: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            spawn_ctx(ctx.clone(), async move {
                match ctx.import_invitation_on_mobile(&code).await {
                    Ok(()) => {
                        let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::success(
                            "devices",
                            "Mobile device accepted enrollment",
                        )));
                    }
                    Err(e) => {
                        let _ =
                            tx.try_send(UiUpdate::ToastAdded(ToastMessage::error("devices", e)));
                    }
                }
            });
        })
    }

    #[cfg(not(feature = "development"))]
    fn make_import_device_enrollment(
        _ctx: Arc<IoContext>,
        _tx: UiUpdateSender,
    ) -> ImportDeviceEnrollmentCallback {
        Arc::new(move |_code: String| {
            // No-op in production builds
        })
    }
}

// =============================================================================
// Home Messaging Callbacks
// =============================================================================

/// All callbacks for home messaging
#[derive(Clone)]
pub struct HomeCallbacks {
    pub on_send: HomeSendCallback,
}

impl HomeCallbacks {
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_send: Self::make_send(ctx, tx),
        }
    }
    fn make_send(ctx: Arc<IoContext>, tx: UiUpdateSender) -> HomeSendCallback {
        Arc::new(move |content: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let content_clone = content;
            spawn_ctx(ctx.clone(), async move {
                let home_id = {
                    use aura_app::signal_defs::HOMES_SIGNAL;
                    use aura_core::effects::reactive::ReactiveEffects;

                    let core = ctx.app_core_raw().read().await;

                    if let Ok(homes) = core.read(&*HOMES_SIGNAL).await {
                        homes
                            .current_home_id
                            .as_ref()
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| "home".to_string())
                    } else {
                        "home".to_string()
                    }
                };

                let channel = format!("home:{home_id}");
                let home_id_clone = home_id.clone();

                let trimmed = content_clone.trim_start();
                if trimmed.starts_with("/") {
                    match parse_command(trimmed) {
                        Ok(IrcCommand::Help { .. }) => {
                            let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::info(
                                "help",
                                "Use ? for TUI help. Supported slash commands: /msg /me /nick /who /whois /join /leave /topic /invite /kick /ban /unban /mute /unmute /pin /unpin /op /deop /mode",
                            )));
                            return;
                        }
                        Ok(irc) => {
                            let mut dispatcher =
                                CommandDispatcher::with_policy(CapabilityPolicy::AllowAll);
                            dispatcher.set_current_channel(channel.clone());

                            let effect = match dispatcher.dispatch(irc) {
                                Ok(cmd) => cmd,
                                Err(e) => {
                                    let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::error(
                                        "command",
                                        e.to_string(),
                                    )));
                                    return;
                                }
                            };

                            match effect {
                                EffectCommand::ListParticipants { channel } => {
                                    match aura_app::workflows::query::list_participants(
                                        ctx.app_core_raw(),
                                        &channel,
                                    )
                                    .await
                                    {
                                        Ok(list) => {
                                            let msg = if list.is_empty() {
                                                "No participants".to_string()
                                            } else {
                                                list.join(", ")
                                            };
                                            let _ = tx.try_send(UiUpdate::ToastAdded(
                                                ToastMessage::info("participants", msg),
                                            ));
                                        }
                                        Err(e) => {
                                            let _ = tx.try_send(UiUpdate::ToastAdded(
                                                ToastMessage::error("participants", e.to_string()),
                                            ));
                                        }
                                    }
                                }
                                EffectCommand::GetUserInfo { target } => {
                                    match aura_app::workflows::query::get_user_info(
                                        ctx.app_core_raw(),
                                        &target,
                                    )
                                    .await
                                    {
                                        Ok(contact) => {
                                            let id = contact.id.to_string();
                                            let name = if !contact.nickname.is_empty() {
                                                contact.nickname
                                            } else if let Some(s) = &contact.suggested_name {
                                                s.clone()
                                            } else {
                                                id.chars().take(8).collect::<String>() + "..."
                                            };
                                            let msg = format!("User: {name} ({id})");
                                            let _ = tx.try_send(UiUpdate::ToastAdded(
                                                ToastMessage::info("whois", msg),
                                            ));
                                        }
                                        Err(e) => {
                                            let _ = tx.try_send(UiUpdate::ToastAdded(
                                                ToastMessage::error("whois", e.to_string()),
                                            ));
                                        }
                                    }
                                }
                                _ => {
                                    let _ = ctx.dispatch(effect).await;
                                }
                            }
                            return;
                        }
                        Err(e) => {
                            let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::error(
                                "command",
                                e.to_string(),
                            )));
                            return;
                        }
                    }
                }

                let cmd = EffectCommand::SendMessage {
                    channel,
                    content: content_clone.clone(),
                };

                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.try_send(UiUpdate::HomeMessageSent {
                            home_id: home_id_clone,
                            content: content_clone,
                        });
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
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
    pub on_enter_home: Arc<dyn Fn(String, TraversalDepth) + Send + Sync>,
    pub on_go_home: GoHomeCallback,
    pub on_back_to_street: GoHomeCallback,
}

impl NeighborhoodCallbacks {
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_enter_home: Self::make_enter_home(ctx.clone(), tx.clone()),
            on_go_home: Self::make_go_home(ctx.clone(), tx.clone()),
            on_back_to_street: Self::make_back_to_street(ctx, tx),
        }
    }

    fn make_enter_home(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> Arc<dyn Fn(String, TraversalDepth) + Send + Sync> {
        Arc::new(move |home_id: String, depth: TraversalDepth| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let home_id_clone = home_id.clone();
            let depth_str = match depth {
                TraversalDepth::Street => "Street",
                TraversalDepth::Frontage => "Frontage",
                TraversalDepth::Interior => "Interior",
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
                        let _ = tx.try_send(UiUpdate::HomeEntered {
                            home_id: home_id_clone,
                        });
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
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
                depth: "Interior".to_string(),
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.try_send(UiUpdate::NavigatedHome);
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
                    }
                }
            });
        })
    }

    fn make_back_to_street(ctx: Arc<IoContext>, tx: UiUpdateSender) -> GoHomeCallback {
        Arc::new(move || {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::MovePosition {
                neighborhood_id: "current".to_string(),
                home_id: "current".to_string(),
                depth: "Street".to_string(),
            };
            spawn_ctx(ctx.clone(), async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.try_send(UiUpdate::NavigatedToStreet);
                    }
                    Err(_e) => {
                        // Error already emitted to ERROR_SIGNAL by dispatch layer.
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
    pub on_create_account: CreateAccountCallback,
}

impl AppCallbacks {
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_create_account: Self::make_create_account(ctx, tx),
        }
    }

    fn make_create_account(ctx: Arc<IoContext>, tx: UiUpdateSender) -> CreateAccountCallback {
        Arc::new(move |display_name: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            spawn_ctx(ctx.clone(), async move {
                // Create the account file via async storage effects.
                match ctx.create_account(&display_name).await {
                    Ok(()) => {
                        // Then persist the display name to settings storage and emit SETTINGS_SIGNAL.
                        // This is best-effort and does not hold up account creation.
                        let _ = ctx
                            .dispatch(EffectCommand::UpdateNickname {
                                name: display_name.clone(),
                            })
                            .await;

                        let _ = tx.try_send(UiUpdate::DisplayNameChanged(display_name.clone()));

                        // Then dispatch the intent to create a journal fact.
                        let cmd = EffectCommand::CreateAccount {
                            display_name: display_name.clone(),
                        };

                        match ctx.dispatch(cmd).await {
                            Ok(_) => {
                                let _ = tx.try_send(UiUpdate::AccountCreated);
                            }
                            Err(e) => {
                                // Non-fatal: file was created, journal fact is optional.
                                tracing::warn!("Journal fact creation failed: {}", e);
                                let _ = tx.try_send(UiUpdate::AccountCreated);
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.try_send(UiUpdate::OperationFailed {
                            operation: "CreateAccount".to_string(),
                            error: e,
                        });
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
    pub home: HomeCallbacks,
    pub neighborhood: NeighborhoodCallbacks,
    pub app: AppCallbacks,
}

impl CallbackRegistry {
    /// Create all callbacks from context
    pub fn new(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
        app_core: Arc<async_lock::RwLock<aura_app::AppCore>>,
    ) -> Self {
        Self {
            chat: ChatCallbacks::new(ctx.clone(), tx.clone(), app_core),
            contacts: ContactsCallbacks::new(ctx.clone(), tx.clone()),
            invitations: InvitationsCallbacks::new(ctx.clone(), tx.clone()),
            recovery: RecoveryCallbacks::new(ctx.clone(), tx.clone()),
            settings: SettingsCallbacks::new(ctx.clone(), tx.clone()),
            home: HomeCallbacks::new(ctx.clone(), tx.clone()),
            neighborhood: NeighborhoodCallbacks::new(ctx.clone(), tx.clone()),
            app: AppCallbacks::new(ctx, tx),
        }
    }
}
