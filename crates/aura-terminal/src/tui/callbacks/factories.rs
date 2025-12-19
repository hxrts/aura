//! # Callback Factories
//!
//! Factory functions that create domain-specific callbacks.
//! Each factory takes an `IoContext` and `UiUpdateSender` and returns
//! a struct containing all callbacks for that domain.
//!
//! This eliminates the ~800 lines of callback creation in `run_app_with_context`.

use std::sync::Arc;

use crate::tui::context::IoContext;
use crate::tui::effects::EffectCommand;
use crate::tui::types::{Device, MfaPolicy, TraversalDepth};
use crate::tui::updates::{UiUpdate, UiUpdateSender};

use super::types::*;

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
            on_channel_select: Self::make_channel_select(app_core, tx.clone()),
            on_create_channel: Self::make_create_channel(ctx.clone(), tx.clone()),
            on_set_topic: Self::make_set_topic(ctx, tx),
        }
    }

    fn make_send(ctx: Arc<IoContext>, tx: UiUpdateSender) -> SendCallback {
        Arc::new(move |channel_id: String, content: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let channel_id_clone = channel_id.clone();
            let content_clone = content.clone();
            let cmd = EffectCommand::SendMessage {
                channel: channel_id,
                content,
            };
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::MessageSent {
                            channel: channel_id_clone,
                            content: content_clone,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "SendMessage".to_string(),
                            error: e.to_string(),
                        });
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
                tokio::spawn(async move {
                    match ctx.dispatch(cmd).await {
                        Ok(_) => {
                            let _ = tx.send(UiUpdate::MessageRetried { message_id: msg_id });
                        }
                        Err(e) => {
                            let _ = tx.send(UiUpdate::OperationFailed {
                                operation: "RetryMessage".to_string(),
                                error: e.to_string(),
                            });
                        }
                    }
                });
            },
        )
    }

    fn make_channel_select(
        app_core: Arc<async_lock::RwLock<aura_app::AppCore>>,
        tx: UiUpdateSender,
    ) -> ChannelSelectCallback {
        Arc::new(move |channel_id: String| {
            let app_core = app_core.clone();
            let tx = tx.clone();
            let channel_id_clone = channel_id.clone();
            tokio::spawn(async move {
                let core = app_core.read().await;
                core.views().select_channel(Some(channel_id));
                let _ = tx.send(UiUpdate::ChannelSelected(channel_id_clone));
            });
        })
    }

    fn make_create_channel(ctx: Arc<IoContext>, tx: UiUpdateSender) -> CreateChannelCallback {
        Arc::new(move |name: String, topic: Option<String>| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let channel_name = name.clone();
            let cmd = EffectCommand::CreateChannel {
                name,
                topic,
                members: vec![],
            };
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::ChannelCreated(channel_name));
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "CreateChannel".to_string(),
                            error: e.to_string(),
                        });
                    }
                }
            });
        })
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
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::TopicSet {
                            channel: ch,
                            topic: t,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "SetTopic".to_string(),
                            error: e.to_string(),
                        });
                    }
                }
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
}

impl ContactsCallbacks {
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_update_nickname: Self::make_update_nickname(ctx.clone(), tx.clone()),
            on_start_chat: Self::make_start_chat(ctx.clone(), tx.clone()),
            on_import_invitation: Self::make_import_invitation(ctx.clone(), tx.clone()),
            on_invite_lan_peer: Self::make_invite_lan_peer(ctx, tx),
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
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::NicknameUpdated {
                            contact_id: contact_id_clone,
                            nickname: nickname_clone,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "UpdateNickname".to_string(),
                            error: e.to_string(),
                        });
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
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::ChatStarted {
                            contact_id: contact_id_clone,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "StartChat".to_string(),
                            error: e.to_string(),
                        });
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
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::InvitationImported {
                            invitation_code: code_clone,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "ImportInvitation".to_string(),
                            error: e.to_string(),
                        });
                    }
                }
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
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        ctx.mark_peer_invited(&authority_id_clone).await;
                        let _ = tx.send(UiUpdate::LanPeerInvited {
                            peer_id: authority_id_clone,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "InviteLanPeer".to_string(),
                            error: e.to_string(),
                        });
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
    pub on_create: CreateInvitationCallback,
    pub on_export: ExportInvitationCallback,
    pub on_import: ImportInvitationCallback,
}

impl InvitationsCallbacks {
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_accept: Self::make_accept(ctx.clone(), tx.clone()),
            on_decline: Self::make_decline(ctx.clone(), tx.clone()),
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
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::InvitationAccepted {
                            invitation_id: inv_id,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "AcceptInvitation".to_string(),
                            error: e.to_string(),
                        });
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
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::InvitationDeclined {
                            invitation_id: inv_id,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "DeclineInvitation".to_string(),
                            error: e.to_string(),
                        });
                    }
                }
            });
        })
    }

    fn make_create(ctx: Arc<IoContext>, tx: UiUpdateSender) -> CreateInvitationCallback {
        Arc::new(
            move |invitation_type: String, message: Option<String>, ttl_secs: Option<u64>| {
                let ctx = ctx.clone();
                let tx = tx.clone();
                let inv_type = invitation_type.clone();
                let cmd = EffectCommand::CreateInvitation {
                    invitation_type,
                    message,
                    ttl_secs,
                };
                tokio::spawn(async move {
                    match ctx.dispatch(cmd).await {
                        Ok(_) => {
                            let _ = tx.send(UiUpdate::InvitationCreated {
                                invitation_code: inv_type,
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(UiUpdate::OperationFailed {
                                operation: "CreateInvitation".to_string(),
                                error: e.to_string(),
                            });
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
            tokio::spawn(async move {
                match ctx.export_invitation_code(&invitation_id).await {
                    Ok(code) => {
                        let _ = tx.send(UiUpdate::InvitationExported { code });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "ExportInvitation".to_string(),
                            error: e,
                        });
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
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::InvitationImported {
                            invitation_code: code_clone,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "ImportInvitation".to_string(),
                            error: e.to_string(),
                        });
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
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::RecoveryStarted);
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "StartRecovery".to_string(),
                            error: e.to_string(),
                        });
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
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::GuardianAdded {
                            contact_id: "unknown".to_string(),
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "AddGuardian".to_string(),
                            error: e.to_string(),
                        });
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
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::GuardianSelected {
                            contact_id: contact_id_clone,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "SelectGuardian".to_string(),
                            error: e.to_string(),
                        });
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
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::ApprovalSubmitted {
                            request_id: request_id_clone,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "SubmitApproval".to_string(),
                            error: e.to_string(),
                        });
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
}

impl SettingsCallbacks {
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_update_mfa: Self::make_update_mfa(ctx.clone(), tx.clone()),
            on_update_display_name: Self::make_update_display_name(ctx.clone(), tx.clone()),
            on_update_threshold: Self::make_update_threshold(ctx.clone(), tx.clone()),
            on_add_device: Self::make_add_device(ctx.clone(), tx.clone()),
            on_remove_device: Self::make_remove_device(ctx, tx),
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
            tokio::spawn(async move {
                ctx.set_mfa_policy(policy).await;
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::MfaPolicyChanged(policy));
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "UpdateMfaPolicy".to_string(),
                            error: e.to_string(),
                        });
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
            let cmd = EffectCommand::UpdateNickname { name: name.clone() };
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        ctx.set_display_name(&name_clone).await;
                        let _ = tx.send(UiUpdate::DisplayNameChanged(name_clone));
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "UpdateDisplayName".to_string(),
                            error: e.to_string(),
                        });
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
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::ThresholdChanged {
                            k: threshold_k,
                            n: threshold_n,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "UpdateThreshold".to_string(),
                            error: e.to_string(),
                        });
                    }
                }
            });
        })
    }

    fn make_add_device(ctx: Arc<IoContext>, tx: UiUpdateSender) -> AddDeviceCallback {
        Arc::new(move |device_name: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let device_name_clone = device_name.clone();
            let cmd = EffectCommand::AddDevice { device_name };
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let device = Device {
                            id: format!("device-{}", device_name_clone),
                            name: device_name_clone,
                            last_seen: None,
                            is_current: false,
                        };
                        let _ = tx.send(UiUpdate::DeviceAdded(device));
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "AddDevice".to_string(),
                            error: e.to_string(),
                        });
                    }
                }
            });
        })
    }

    fn make_remove_device(ctx: Arc<IoContext>, tx: UiUpdateSender) -> RemoveDeviceCallback {
        Arc::new(move |device_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let device_id_clone = device_id.clone();
            let cmd = EffectCommand::RemoveDevice { device_id };
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::DeviceRemoved {
                            device_id: device_id_clone,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "RemoveDevice".to_string(),
                            error: e.to_string(),
                        });
                    }
                }
            });
        })
    }
}

// =============================================================================
// Block Callbacks
// =============================================================================

/// All callbacks for the block screen
#[derive(Clone)]
pub struct BlockCallbacks {
    pub on_send: BlockSendCallback,
    pub on_invite: BlockInviteCallback,
    pub on_navigate_neighborhood: BlockNavCallback,
    pub on_grant_steward: GrantStewardCallback,
    pub on_revoke_steward: RevokeStewardCallback,
}

impl BlockCallbacks {
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_send: Self::make_send(ctx.clone(), tx.clone()),
            on_invite: Self::make_invite(ctx.clone(), tx.clone()),
            on_navigate_neighborhood: Self::make_navigate_neighborhood(ctx.clone(), tx.clone()),
            on_grant_steward: Self::make_grant_steward(ctx.clone(), tx.clone()),
            on_revoke_steward: Self::make_revoke_steward(ctx, tx),
        }
    }

    fn make_send(ctx: Arc<IoContext>, tx: UiUpdateSender) -> BlockSendCallback {
        Arc::new(move |content: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let content_clone = content.clone();
            tokio::spawn(async move {
                let block_id = {
                    use aura_app::signal_defs::{BLOCKS_SIGNAL, BLOCK_SIGNAL};
                    use aura_core::effects::reactive::ReactiveEffects;

                    let core = ctx.app_core().read().await;

                    // Prefer multi-block selection; fall back to legacy singular block.
                    if let Ok(blocks) = core.read(&*BLOCKS_SIGNAL).await {
                        blocks
                            .current_block_id
                            .clone()
                            .unwrap_or_else(|| "home".to_string())
                    } else if let Ok(block) = core.read(&*BLOCK_SIGNAL).await {
                        block.id.clone()
                    } else {
                        "home".to_string()
                    }
                };
                let channel = format!("block:{}", block_id);
                let block_id_clone = block_id.clone();
                let cmd = EffectCommand::SendMessage {
                    channel,
                    content: content_clone.clone(),
                };
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::BlockMessageSent {
                            block_id: block_id_clone,
                            content: content_clone,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "SendBlockMessage".to_string(),
                            error: e.to_string(),
                        });
                    }
                }
            });
        })
    }

    fn make_invite(ctx: Arc<IoContext>, tx: UiUpdateSender) -> BlockInviteCallback {
        Arc::new(move |contact_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let contact_id_clone = contact_id.clone();
            let cmd = EffectCommand::SendBlockInvitation {
                contact_id: contact_id.clone(),
            };
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::BlockInviteSent {
                            contact_id: contact_id_clone,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "SendBlockInvitation".to_string(),
                            error: e.to_string(),
                        });
                    }
                }
            });
        })
    }

    fn make_navigate_neighborhood(ctx: Arc<IoContext>, tx: UiUpdateSender) -> BlockNavCallback {
        Arc::new(move || {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let cmd = EffectCommand::MovePosition {
                neighborhood_id: "current".to_string(),
                block_id: "current".to_string(),
                depth: "Street".to_string(),
            };
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::NavigatedToNeighborhood);
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "NavigateToNeighborhood".to_string(),
                            error: e.to_string(),
                        });
                    }
                }
            });
        })
    }

    fn make_grant_steward(ctx: Arc<IoContext>, tx: UiUpdateSender) -> GrantStewardCallback {
        Arc::new(move |resident_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let resident_id_clone = resident_id.clone();
            let cmd = EffectCommand::GrantSteward {
                target: resident_id.clone(),
            };
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::StewardGranted {
                            contact_id: resident_id_clone,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "GrantSteward".to_string(),
                            error: e.to_string(),
                        });
                    }
                }
            });
        })
    }

    fn make_revoke_steward(ctx: Arc<IoContext>, tx: UiUpdateSender) -> RevokeStewardCallback {
        Arc::new(move |resident_id: String| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let resident_id_clone = resident_id.clone();
            let cmd = EffectCommand::RevokeSteward {
                target: resident_id.clone(),
            };
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::StewardRevoked {
                            contact_id: resident_id_clone,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "RevokeSteward".to_string(),
                            error: e.to_string(),
                        });
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
    pub on_enter_block: Arc<dyn Fn(String, TraversalDepth) + Send + Sync>,
    pub on_go_home: GoHomeCallback,
    pub on_back_to_street: GoHomeCallback,
}

impl NeighborhoodCallbacks {
    pub fn new(ctx: Arc<IoContext>, tx: UiUpdateSender) -> Self {
        Self {
            on_enter_block: Self::make_enter_block(ctx.clone(), tx.clone()),
            on_go_home: Self::make_go_home(ctx.clone(), tx.clone()),
            on_back_to_street: Self::make_back_to_street(ctx, tx),
        }
    }

    fn make_enter_block(
        ctx: Arc<IoContext>,
        tx: UiUpdateSender,
    ) -> Arc<dyn Fn(String, TraversalDepth) + Send + Sync> {
        Arc::new(move |block_id: String, depth: TraversalDepth| {
            let ctx = ctx.clone();
            let tx = tx.clone();
            let block_id_clone = block_id.clone();
            let depth_str = match depth {
                TraversalDepth::Street => "Street",
                TraversalDepth::Frontage => "Frontage",
                TraversalDepth::Interior => "Interior",
            }
            .to_string();
            let cmd = EffectCommand::MovePosition {
                neighborhood_id: "current".to_string(),
                block_id,
                depth: depth_str,
            };
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::BlockEntered {
                            block_id: block_id_clone,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "EnterBlock".to_string(),
                            error: e.to_string(),
                        });
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
                block_id: "home".to_string(),
                depth: "Interior".to_string(),
            };
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::NavigatedHome);
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "GoHome".to_string(),
                            error: e.to_string(),
                        });
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
                block_id: "current".to_string(),
                depth: "Street".to_string(),
            };
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        let _ = tx.send(UiUpdate::NavigatedToStreet);
                    }
                    Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "BackToStreet".to_string(),
                            error: e.to_string(),
                        });
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
            tokio::spawn(async move {
                // First create the account file (disk I/O) off the async runtime.
                let ctx_for_files = ctx.clone();
                let display_name_for_files = display_name.clone();
                let file_result = tokio::task::spawn_blocking(move || {
                    ctx_for_files.create_account(&display_name_for_files)
                })
                .await
                .map_err(|e| format!("Account creation task failed: {}", e));

                match file_result {
                    Ok(Ok(())) => {
                        // Then persist the display name to settings storage and emit SETTINGS_SIGNAL.
                        // This is best-effort and does not block account creation.
                        let _ = ctx
                            .dispatch(EffectCommand::UpdateNickname {
                                name: display_name.clone(),
                            })
                            .await;

                        let _ = tx.send(UiUpdate::DisplayNameChanged(display_name.clone()));

                        // Then dispatch the intent to create a journal fact.
                        let cmd = EffectCommand::CreateAccount {
                            display_name: display_name.clone(),
                        };

                        match ctx.dispatch(cmd).await {
                            Ok(_) => {
                                let _ = tx.send(UiUpdate::AccountCreated);
                            }
                            Err(e) => {
                                // Non-fatal: file was created, journal fact is optional.
                                tracing::warn!("Journal fact creation failed: {}", e);
                                let _ = tx.send(UiUpdate::AccountCreated);
                            }
                        }
                    }
                    Ok(Err(e)) | Err(e) => {
                        let _ = tx.send(UiUpdate::OperationFailed {
                            operation: "CreateAccount".to_string(),
                            error: e.to_string(),
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
    pub block: BlockCallbacks,
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
            block: BlockCallbacks::new(ctx.clone(), tx.clone()),
            neighborhood: NeighborhoodCallbacks::new(ctx.clone(), tx.clone()),
            app: AppCallbacks::new(ctx, tx),
        }
    }
}
