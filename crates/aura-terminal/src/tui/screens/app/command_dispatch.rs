//! # Command Dispatch
//!
//! Handles TuiCommand and DispatchCommand execution, mapping state machine
//! commands to callbacks and UI updates.

use crate::tui::callbacks::CallbackRegistry;
use crate::tui::components::{ContactSelectState, ToastLevel, ToastMessage};
use crate::tui::state_machine::{DispatchCommand, TuiCommand, TuiState};
use crate::tui::types::TraversalDepth;
use crate::tui::updates::{UiUpdate, UiUpdateSender};
use iocraft::prelude::*;

/// Context needed for command dispatch
pub struct DispatchContext<'a> {
    /// Callback registry for domain actions
    pub callbacks: &'a CallbackRegistry,
    /// Optional UI update sender for toasts
    pub update_tx: Option<&'a UiUpdateSender>,
    /// Guardian select state for index-to-id mapping
    pub guardian_select_state: &'a UseState<ContactSelectState>,
    /// TuiState for modal management
    pub tui_state: &'a UseState<TuiState>,
    /// Should exit flag
    pub should_exit: &'a UseState<bool>,
    /// Version counters for triggering re-renders
    pub guardian_select_version: &'a UseState<u32>,
}

/// Execute a list of TuiCommands
pub fn execute_commands(commands: Vec<TuiCommand>, ctx: &DispatchContext) {
    for cmd in commands {
        execute_command(cmd, ctx);
    }
}

/// Execute a single TuiCommand
fn execute_command(cmd: TuiCommand, ctx: &DispatchContext) {
    match cmd {
        TuiCommand::Exit => {
            ctx.should_exit.set(true);
        }
        TuiCommand::Dispatch(dispatch_cmd) => {
            execute_dispatch_command(dispatch_cmd, ctx);
        }
        TuiCommand::ShowToast { message, level } => {
            if let Some(tx) = ctx.update_tx {
                let toast_level = match level {
                    crate::tui::state_machine::ToastLevel::Info => ToastLevel::Info,
                    crate::tui::state_machine::ToastLevel::Success => ToastLevel::Success,
                    crate::tui::state_machine::ToastLevel::Warning => ToastLevel::Warning,
                    crate::tui::state_machine::ToastLevel::Error => ToastLevel::Error,
                };
                let toast_id = format!(
                    "toast-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0)
                );
                let toast = ToastMessage::new(toast_id, message).with_level(toast_level);
                let _ = tx.send(UiUpdate::ToastAdded(toast));
            }
        }
        TuiCommand::DismissToast { id } => {
            if let Some(tx) = ctx.update_tx {
                let toast_id = format!("toast-{}", id);
                let _ = tx.send(UiUpdate::ToastDismissed { toast_id });
            }
        }
        TuiCommand::ClearAllToasts => {
            if let Some(tx) = ctx.update_tx {
                let _ = tx.send(UiUpdate::ToastsCleared);
            }
        }
        TuiCommand::Render => {
            // Render is handled by iocraft automatically
        }
    }
}

/// Execute a DispatchCommand by invoking the appropriate callback
fn execute_dispatch_command(cmd: DispatchCommand, ctx: &DispatchContext) {
    let cb = &ctx.callbacks;

    match cmd {
        // === Account Commands ===
        DispatchCommand::CreateAccount { name } => {
            if let Some(ref callback) = cb.account.on_create {
                callback(name);
            }
        }
        DispatchCommand::AddGuardian { contact_id } => {
            if let Some(ref callback) = cb.recovery.on_select_guardian {
                callback(contact_id);
            }
        }
        DispatchCommand::SelectGuardianByIndex { index } => {
            // Map index to contact_id from legacy modal state
            let contact_id = ctx
                .guardian_select_state
                .read()
                .contacts
                .get(index)
                .map(|c| c.id.clone());

            // Hide the modal
            ctx.guardian_select_state.write().hide();
            ctx.guardian_select_version
                .set(ctx.guardian_select_version.get().wrapping_add(1));

            // Also close in TuiState
            ctx.tui_state.write().modal.close();

            // Call the callback with contact_id
            if let Some(contact_id) = contact_id {
                if let Some(ref callback) = cb.recovery.on_select_guardian {
                    callback(contact_id);
                }
            }
        }

        // === Block Screen Commands ===
        DispatchCommand::SendBlockMessage { content } => {
            if let Some(ref callback) = cb.block.on_send {
                callback(content);
            }
        }
        DispatchCommand::InviteToBlock { contact_id } => {
            if let Some(ref callback) = cb.block.on_invite {
                callback(contact_id);
            }
        }
        DispatchCommand::GrantSteward { resident_id } => {
            if let Some(ref callback) = cb.block.on_grant_steward {
                callback(resident_id);
            }
        }
        DispatchCommand::RevokeSteward { resident_id } => {
            if let Some(ref callback) = cb.block.on_revoke_steward {
                callback(resident_id);
            }
        }

        // === Chat Screen Commands ===
        DispatchCommand::SelectChannel { channel_id } => {
            if let Some(ref callback) = cb.chat.on_channel_select {
                callback(channel_id);
            }
        }
        DispatchCommand::SendChatMessage {
            channel_id,
            content,
        } => {
            if let Some(ref callback) = cb.chat.on_send {
                callback(channel_id, content);
            }
        }
        DispatchCommand::RetryMessage { message_id } => {
            // Note: RetryMessage requires channel and content from the failed message
            // The callback expects (message_id, channel_id, content)
            // For now, log a warning since we don't have the full message context here
            if cb.chat.on_retry_message.is_some() {
                tracing::warn!(
                    "RetryMessage not fully implemented: message_id={}",
                    message_id
                );
            }
        }
        DispatchCommand::CreateChannel { name } => {
            if let Some(ref callback) = cb.chat.on_create_channel {
                callback(name, None);
            }
        }
        DispatchCommand::SetChannelTopic { channel_id, topic } => {
            if let Some(ref callback) = cb.chat.on_set_topic {
                callback(channel_id, topic);
            }
        }

        // === Contacts Screen Commands ===
        DispatchCommand::UpdatePetname {
            contact_id,
            petname,
        } => {
            if let Some(ref callback) = cb.contacts.on_update_petname {
                callback(contact_id, petname);
            }
        }
        DispatchCommand::StartChat { contact_id } => {
            if let Some(ref callback) = cb.contacts.on_start_chat {
                callback(contact_id);
            }
        }

        // === Invitations Screen Commands ===
        DispatchCommand::AcceptInvitation { invitation_id } => {
            if let Some(ref callback) = cb.invitations.on_accept {
                callback(invitation_id);
            }
        }
        DispatchCommand::DeclineInvitation { invitation_id } => {
            if let Some(ref callback) = cb.invitations.on_decline {
                callback(invitation_id);
            }
        }
        DispatchCommand::CreateInvitation {
            invitation_type,
            message,
        } => {
            if let Some(ref callback) = cb.invitations.on_create {
                // Third argument is TTL in seconds (None = no expiry)
                callback(invitation_type, message, None);
            }
        }
        DispatchCommand::ImportInvitation { code } => {
            if let Some(ref callback) = cb.invitations.on_import {
                callback(code);
            }
        }
        DispatchCommand::ExportInvitation { invitation_id } => {
            if let Some(ref callback) = cb.invitations.on_export {
                callback(invitation_id);
            }
        }

        // === Recovery Screen Commands ===
        DispatchCommand::StartRecovery => {
            if let Some(ref callback) = cb.recovery.on_start {
                callback();
            }
        }
        DispatchCommand::ApproveRecovery { request_id } => {
            if let Some(ref callback) = cb.recovery.on_submit_approval {
                callback(request_id);
            }
        }

        // === Guardian Ceremony Commands ===
        DispatchCommand::StartGuardianCeremony {
            contact_ids,
            threshold_k,
        } => {
            // Guardian ceremony is a multi-step process
            // This command initiates the ceremony with selected guardians
            tracing::info!(
                "Starting guardian ceremony with {} guardians, threshold {}",
                contact_ids.len(),
                threshold_k
            );
            // The actual ceremony flow would be handled by the state machine
            // through the guardian setup modal steps
        }
        DispatchCommand::CancelGuardianCeremony => {
            tracing::info!("Guardian ceremony cancelled");
            // State machine handles closing the modal
        }
        DispatchCommand::ConfirmGuardianCeremony => {
            tracing::info!("Guardian ceremony confirmed");
            // The ceremony completion is handled via callbacks when the user
            // confirms in the final step of the modal flow
        }

        // === Settings Screen Commands ===
        DispatchCommand::UpdateNickname { nickname } => {
            if let Some(ref callback) = cb.settings.on_update_nickname {
                callback(nickname);
            }
        }
        DispatchCommand::UpdateThreshold { k, n } => {
            if let Some(ref callback) = cb.settings.on_update_threshold {
                callback(k, n);
            }
        }
        DispatchCommand::UpdateMfaPolicy { policy } => {
            if let Some(ref callback) = cb.settings.on_update_mfa {
                callback(policy);
            }
        }
        DispatchCommand::AddDevice { name } => {
            if let Some(ref callback) = cb.settings.on_add_device {
                callback(name);
            }
        }
        DispatchCommand::RemoveDevice { device_id } => {
            if let Some(ref callback) = cb.settings.on_remove_device {
                callback(device_id);
            }
        }

        // === Neighborhood Screen Commands ===
        DispatchCommand::EnterBlock { block_id } => {
            if let Some(ref callback) = cb.neighborhood.on_enter_block {
                // Default to Street-level traversal depth
                callback(block_id, TraversalDepth::default());
            }
        }
        DispatchCommand::GoHome => {
            if let Some(ref callback) = cb.neighborhood.on_go_home {
                callback();
            }
        }
        DispatchCommand::BackToStreet => {
            if let Some(ref callback) = cb.neighborhood.on_back_to_street {
                callback();
            }
        }

        // === Navigation Commands ===
        DispatchCommand::NavigateTo(_screen) => {
            // Navigation is handled by TuiState directly
            // The state machine already updates the screen
        }
    }
}
