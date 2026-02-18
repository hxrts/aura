//! Modal keyboard handlers
//!
//! All queue-based modal key event handlers.
//!
//! Note: Modal state types are passed by value from the dispatcher's match arms.
//! This is intentional for the queue-based modal system where states are moved
//! out of the enum for handling.

#![allow(clippy::needless_pass_by_value)]

use aura_core::effects::terminal::{KeyCode, KeyEvent};

use crate::tui::components::copy_to_clipboard;
use crate::tui::navigation::{navigate_list, NavKey};
use crate::tui::screens::Screen;

use super::super::commands::{DispatchCommand, TuiCommand};
use super::super::modal_queue::{
    ChatMemberSelectModalState, ConfirmAction, ContactSelectModalState, QueuedModal,
};
use super::super::toast::{QueuedToast, ToastLevel};
use super::super::views::{
    AccountSetupModalState, AddDeviceField, AddDeviceModalState, ConfirmRemoveModalState,
    CreateChannelModalState, CreateInvitationField, CreateInvitationModalState,
    DeviceEnrollmentCeremonyModalState, DeviceSelectModalState, GuardianSetupModalState,
    GuardianSetupStep, ImportInvitationModalState, NicknameModalState,
    NicknameSuggestionModalState, TopicModalState,
};
use super::super::TuiState;

/// Handle queue-based modal key events (unified dispatcher)
///
/// This routes key events to the appropriate handler based on the QueuedModal variant.
/// All new modal handlers should use this queue-based system.
pub fn handle_queued_modal_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal: QueuedModal,
) {
    // First, check for toast dismissal on Esc (toasts have priority)
    if key.code == KeyCode::Esc {
        if let Some(toast_id) = state.toast_queue.current().map(|t| t.id.clone()) {
            state.toast_queue.dismiss();
            commands.push(TuiCommand::DismissToast { id: toast_id });
            return;
        }
    }

    // Route to specific handlers based on modal type
    match modal {
        QueuedModal::AccountSetup(modal_state) => {
            handle_account_setup_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::Help { .. } => {
            handle_help_modal_key_queue(state, key);
        }
        QueuedModal::Confirm { on_confirm, .. } => {
            handle_confirm_modal_key_queue(state, commands, key, on_confirm);
        }
        QueuedModal::GuardianSelect(modal_state) => {
            handle_guardian_select_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::ContactSelect(modal_state) => {
            handle_contact_select_key_queue(state, commands, key, modal_state);
        }
        // Chat screen modals
        QueuedModal::ChatCreate(modal_state) => {
            handle_chat_create_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::ChatTopic(modal_state) => {
            handle_chat_topic_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::ChatInfo(_) => {
            // Info modal is read-only - just Esc to dismiss
            if key.code == KeyCode::Esc {
                state.modal_queue.dismiss();
            }
        }
        // Contacts screen modals
        QueuedModal::ContactsNickname(modal_state) => {
            handle_nickname_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::ContactsImport(modal_state) => {
            handle_import_invitation_key_queue(state, commands, key, modal_state, Screen::Contacts);
        }
        QueuedModal::ContactsCreate(modal_state) => {
            handle_create_invitation_key_queue(state, commands, key, modal_state, Screen::Contacts);
        }
        QueuedModal::ContactsCode(modal_state) => {
            // Code display modal: Esc to dismiss, c to copy
            match key.code {
                KeyCode::Esc => {
                    state.modal_queue.dismiss();
                }
                KeyCode::Char('c') => {
                    // Copy code to clipboard (c or Cmd+C)
                    if !modal_state.code.is_empty() && copy_to_clipboard(&modal_state.code).is_ok()
                    {
                        // Update state to show "copied" feedback
                        state.modal_queue.update_active(|m| {
                            if let QueuedModal::ContactsCode(s) = m {
                                s.set_copied();
                            }
                        });
                        state.toast_success("Copied to clipboard");
                    }
                }
                _ => {}
            }
        }
        QueuedModal::GuardianSetup(modal_state) => {
            handle_guardian_setup_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::MfaSetup(modal_state) => {
            handle_mfa_setup_key_queue(state, commands, key, modal_state);
        }
        // Settings screen modals
        QueuedModal::SettingsNicknameSuggestion(modal_state) => {
            handle_settings_nickname_suggestion_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::SettingsAddDevice(modal_state) => {
            handle_settings_add_device_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::SettingsDeviceImport(modal_state) => {
            handle_device_import_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::SettingsDeviceEnrollment(modal_state) => {
            handle_device_enrollment_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::SettingsDeviceSelect(modal_state) => {
            handle_device_select_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::SettingsRemoveDevice(modal_state) => {
            handle_settings_remove_device_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::AuthorityPicker(modal_state) => {
            handle_authority_picker_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::ChatMemberSelect(modal_state) => {
            handle_chat_member_select_key_queue(state, commands, key, modal_state);
        }
        // Neighborhood screen modals
        QueuedModal::NeighborhoodHomeCreate(modal_state) => {
            if modal_state.creating {
                return;
            }

            match key.code {
                KeyCode::Esc => {
                    state.modal_queue.dismiss();
                }
                KeyCode::Tab => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::NeighborhoodHomeCreate(ref mut s) = modal {
                            s.next_field();
                        }
                    });
                }
                KeyCode::Enter => {
                    if modal_state.can_submit() {
                        let name = modal_state.name.clone();
                        let description = modal_state.get_description().map(|s| s.to_string());

                        state.modal_queue.update_active(|modal| {
                            if let QueuedModal::NeighborhoodHomeCreate(ref mut s) = modal {
                                s.start_creating();
                            }
                        });

                        commands.push(TuiCommand::Dispatch(DispatchCommand::CreateHome {
                            name,
                            description,
                        }));
                    }
                }
                KeyCode::Char(c) => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::NeighborhoodHomeCreate(ref mut s) = modal {
                            s.push_char(c);
                        }
                    });
                }
                KeyCode::Backspace => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::NeighborhoodHomeCreate(ref mut s) = modal {
                            s.pop_char();
                        }
                    });
                }
                _ => {}
            }
        }
    }
}

/// Handle account setup modal keys (queue-based)
fn handle_account_setup_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    current_state: AccountSetupModalState,
) {
    // If we're in success state, Enter dismisses
    if current_state.success {
        if key.code == KeyCode::Enter {
            state.modal_queue.dismiss();
        }
        return;
    }

    // If we're in error state, Enter resets to input
    if current_state.error.is_some() {
        if key.code == KeyCode::Enter {
            // Reset to input state
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::AccountSetup(ref mut s) = modal {
                    s.reset_to_input();
                }
            });
        }
        return;
    }

    // If we're creating, don't process input
    if current_state.creating {
        return;
    }

    // Normal input handling
    match key.code {
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::AccountSetup(ref mut s) = modal {
                    s.nickname_suggestion.push(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::AccountSetup(ref mut s) = modal {
                    s.nickname_suggestion.pop();
                }
            });
        }
        KeyCode::Enter => {
            if current_state.can_submit() {
                let name = current_state.nickname_suggestion;
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::AccountSetup(ref mut s) = modal {
                        s.start_creating();
                    }
                });
                commands.push(TuiCommand::Dispatch(DispatchCommand::CreateAccount {
                    name,
                }));
            }
        }
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        _ => {}
    }
}

/// Handle help modal keys (queue-based)
fn handle_help_modal_key_queue(state: &mut TuiState, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            state.modal_queue.dismiss();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.help.scroll = navigate_list(state.help.scroll, state.help.scroll_max, NavKey::Up);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.help.scroll =
                navigate_list(state.help.scroll, state.help.scroll_max, NavKey::Down);
        }
        _ => {}
    }
}

/// Handle confirm modal keys (queue-based)
fn handle_confirm_modal_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    on_confirm: Option<ConfirmAction>,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
            // Execute confirm action if provided
            if let Some(action) = on_confirm {
                match action {
                    ConfirmAction::DeleteChannel { channel_id } => {
                        commands.push(TuiCommand::Dispatch(DispatchCommand::DeleteChannel {
                            channel_id,
                        }));
                    }
                    ConfirmAction::RemoveContact { contact_id } => {
                        commands.push(TuiCommand::Dispatch(DispatchCommand::RemoveContact {
                            contact_id,
                        }));
                    }
                    ConfirmAction::RevokeInvitation { invitation_id } => {
                        commands.push(TuiCommand::Dispatch(DispatchCommand::RevokeInvitation {
                            invitation_id,
                        }));
                    }
                    ConfirmAction::RemoveDevice { device_id } => {
                        commands.push(TuiCommand::Dispatch(DispatchCommand::RemoveDevice {
                            device_id,
                        }));
                    }
                }
            }
            state.modal_queue.dismiss();
        }
        _ => {}
    }
}

/// Handle guardian select modal keys (queue-based)
fn handle_guardian_select_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ContactSelectModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::GuardianSelect(ref mut s) = modal {
                    s.selected_index =
                        navigate_list(s.selected_index, s.contacts.len(), NavKey::Up);
                }
            });
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::GuardianSelect(ref mut s) = modal {
                    s.selected_index =
                        navigate_list(s.selected_index, s.contacts.len(), NavKey::Down);
                }
            });
        }
        KeyCode::Enter => {
            if let Some((contact_id, _)) = modal_state.contacts.get(modal_state.selected_index) {
                commands.push(TuiCommand::Dispatch(DispatchCommand::AddGuardian {
                    contact_id: contact_id.clone(),
                }));
                state.modal_queue.dismiss();
            }
        }
        _ => {}
    }
}

/// Handle contact select modal keys (queue-based)
fn handle_contact_select_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ContactSelectModalState,
) {
    let contact_count = modal_state.contacts.len();
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ContactSelect(ref mut s) = modal {
                    s.selected_index =
                        navigate_list(s.selected_index, s.contacts.len(), NavKey::Up);
                }
            });
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ContactSelect(ref mut s) = modal {
                    s.selected_index =
                        navigate_list(s.selected_index, s.contacts.len(), NavKey::Down);
                }
            });
        }
        KeyCode::Enter => {
            if contact_count > 0 {
                commands.push(TuiCommand::Dispatch(
                    DispatchCommand::SelectContactByIndex {
                        index: modal_state.selected_index,
                    },
                ));
            }
            // Note: Don't dismiss here - let command handler do it
        }
        _ => {}
    }
}

/// Handle chat create channel modal keys (queue-based)
fn handle_chat_create_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: CreateChannelModalState,
) {
    use crate::tui::state::CreateChannelStep;
    use aura_core::effects::terminal::KeyCode::*;

    match modal_state.step {
        CreateChannelStep::Details => match key.code {
            Esc => {
                state.modal_queue.dismiss();
            }
            Tab => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ChatCreate(ref mut s) = modal {
                        s.active_field = (s.active_field + 1) % 2;
                    }
                });
            }
            Enter => {
                if modal_state.can_submit() {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::ChatCreate(ref mut s) = modal {
                            s.step = CreateChannelStep::Members;
                            s.error = None;
                        }
                    });
                }
            }
            Char(c) => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ChatCreate(ref mut s) = modal {
                        if s.active_field == 0 {
                            s.name.push(c);
                        } else {
                            s.topic.push(c);
                        }
                    }
                });
            }
            Backspace => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ChatCreate(ref mut s) = modal {
                        if s.active_field == 0 {
                            s.name.pop();
                        } else {
                            s.topic.pop();
                        }
                    }
                });
            }
            _ => {}
        },
        CreateChannelStep::Members => match key.code {
            Esc => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ChatCreate(ref mut s) = modal {
                        s.step = CreateChannelStep::Details;
                    }
                });
            }
            Up | Char('k') => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ChatCreate(ref mut s) = modal {
                        if s.focused_index > 0 {
                            s.focused_index -= 1;
                        }
                    }
                });
            }
            Down | Char('j') => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ChatCreate(ref mut s) = modal {
                        if s.focused_index + 1 < s.contacts.len() {
                            s.focused_index += 1;
                        }
                    }
                });
            }
            Char(' ') => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ChatCreate(ref mut s) = modal {
                        s.toggle_selection();
                    }
                });
            }
            Enter => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ChatCreate(ref mut s) = modal {
                        s.ensure_threshold();
                        s.step = CreateChannelStep::Threshold;
                    }
                });
            }
            _ => {}
        },
        CreateChannelStep::Threshold => match key.code {
            Esc => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ChatCreate(ref mut s) = modal {
                        s.step = CreateChannelStep::Members;
                    }
                });
            }
            Up | Char('k') => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ChatCreate(ref mut s) = modal {
                        let total = s.total_participants().max(1);
                        s.threshold_custom = true;
                        s.threshold_k = (s.threshold_k + 1).min(total);
                    }
                });
            }
            Down | Char('j') => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ChatCreate(ref mut s) = modal {
                        s.threshold_custom = true;
                        s.threshold_k = s.threshold_k.saturating_sub(1).max(1);
                    }
                });
            }
            Enter => {
                // Create channel directly from Threshold step
                if modal_state.can_submit() {
                    let topic = if modal_state.topic.trim().is_empty() {
                        None
                    } else {
                        Some(modal_state.topic.clone())
                    };
                    let members = modal_state.selected_member_ids();
                    let member_count = members.len();
                    let channel_name = modal_state.name.clone();

                    commands.push(TuiCommand::Dispatch(DispatchCommand::CreateChannel {
                        name: channel_name.clone(),
                        topic,
                        members,
                        threshold_k: modal_state.threshold_k,
                    }));

                    // Dismiss modal and show toast
                    state.modal_queue.dismiss();
                    state.next_toast_id += 1;
                    let toast_message = if member_count > 0 {
                        format!(
                            "Created '{}'. {} invite{} sent.",
                            channel_name,
                            member_count,
                            if member_count == 1 { "" } else { "s" }
                        )
                    } else {
                        format!("Created '{channel_name}'.")
                    };
                    state.toast_queue.enqueue(QueuedToast::new(
                        state.next_toast_id,
                        toast_message,
                        ToastLevel::Success,
                    ));
                }
            }
            _ => {}
        },
    }
}

/// Handle chat member selection modal keys (queue-based)
fn handle_chat_member_select_key_queue(
    state: &mut TuiState,
    _commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ChatMemberSelectModalState,
) {
    match key.code {
        KeyCode::Esc => {
            let draft = modal_state.draft;
            state.modal_queue.update_active(|modal| {
                *modal = QueuedModal::ChatCreate(draft);
            });
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ChatMemberSelect(ref mut s) = modal {
                    s.picker.selected_index =
                        navigate_list(s.picker.selected_index, s.picker.contacts.len(), NavKey::Up);
                }
            });
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ChatMemberSelect(ref mut s) = modal {
                    s.picker.selected_index = navigate_list(
                        s.picker.selected_index,
                        s.picker.contacts.len(),
                        NavKey::Down,
                    );
                }
            });
        }
        KeyCode::Char(' ') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ChatMemberSelect(ref mut s) = modal {
                    s.picker.toggle_selection();
                }
            });
        }
        KeyCode::Enter => {
            let mut draft = modal_state.draft;
            draft.member_ids = modal_state.picker.selected_ids;
            state.modal_queue.update_active(|modal| {
                *modal = QueuedModal::ChatCreate(draft);
            });
        }
        _ => {}
    }
}

/// Handle chat topic edit modal keys (queue-based)
fn handle_chat_topic_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: TopicModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter => {
            commands.push(TuiCommand::Dispatch(DispatchCommand::SetChannelTopic {
                channel_id: modal_state.channel_id.clone(),
                topic: modal_state.value,
            }));
            state.modal_queue.dismiss();
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ChatTopic(ref mut s) = modal {
                    s.value.push(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ChatTopic(ref mut s) = modal {
                    s.value.pop();
                }
            });
        }
        _ => {}
    }
}

/// Handle nickname edit modal keys (queue-based)
fn handle_nickname_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: NicknameModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter => {
            if modal_state.can_submit() {
                let nickname = modal_state.value.trim().to_string();
                commands.push(TuiCommand::Dispatch(DispatchCommand::UpdateNickname {
                    contact_id: modal_state.contact_id,
                    nickname,
                }));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ContactsNickname(ref mut s) = modal {
                    s.value.push(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ContactsNickname(ref mut s) = modal {
                    s.value.pop();
                }
            });
        }
        _ => {}
    }
}

/// Handle import invitation modal keys (queue-based)
fn handle_import_invitation_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ImportInvitationModalState,
    _source_screen: Screen,
) {
    // Demo shortcuts: Ctrl+A / Ctrl+L fill Alice/Carol invite codes.
    //
    // These are handled at the state machine layer so they work consistently
    // for the Contacts invitation import workflow.
    let is_ctrl_a = (key.modifiers.ctrl()
        && matches!(key.code, KeyCode::Char('a') | KeyCode::Char('A')))
        // Some terminals report Ctrl+a as the control character (SOH, 0x01) with no modifiers.
        || matches!(key.code, KeyCode::Char('\u{1}'));
    let is_ctrl_l = (key.modifiers.ctrl()
        && matches!(key.code, KeyCode::Char('l') | KeyCode::Char('L')))
        // Some terminals report Ctrl+l as the control character (FF, 0x0c) with no modifiers.
        || matches!(key.code, KeyCode::Char('\u{c}'));

    if is_ctrl_a || is_ctrl_l {
        // Dismiss the demo hint toast since the user used a shortcut
        state.toast_queue.dismiss();

        let code = if is_ctrl_a {
            state.contacts.demo_alice_code.clone()
        } else {
            state.contacts.demo_carol_code.clone()
        };

        if !code.is_empty() {
            state.modal_queue.update_active(|modal| match modal {
                QueuedModal::ContactsImport(ref mut s) => s.code = code.clone(),
                _ => {}
            });
        }
        return;
    }

    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter => {
            if modal_state.can_submit() {
                commands.push(TuiCommand::Dispatch(DispatchCommand::ImportInvitation {
                    code: modal_state.code,
                }));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| match modal {
                QueuedModal::ContactsImport(ref mut s) => s.code.push(c),
                _ => {}
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| match modal {
                QueuedModal::ContactsImport(ref mut s) => {
                    s.code.pop();
                }
                _ => {}
            });
        }
        _ => {}
    }
}

/// Handle device enrollment import modal keys (queue-based)
fn handle_device_import_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ImportInvitationModalState,
) {
    // Demo shortcut: Ctrl+M fills the Mobile device enrollment code.
    let is_ctrl_m =
        key.modifiers.ctrl() && matches!(key.code, KeyCode::Char('m') | KeyCode::Char('M'));
    let is_enter_autofill = key.code == KeyCode::Enter
        && modal_state.code.is_empty()
        && !state.settings.demo_mobile_device_id.is_empty();

    if is_ctrl_m || is_enter_autofill {
        state.toast_queue.dismiss();
        let code = state.settings.last_device_enrollment_code.clone();
        if !code.is_empty() {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsDeviceImport(ref mut s) = modal {
                    s.code = code.clone();
                }
            });
        } else {
            state.settings.pending_mobile_enrollment_autofill = true;
            // Demo mode: Mobile agent is simulated, use legacy bearer token mode
            commands.push(TuiCommand::Dispatch(DispatchCommand::AddDevice {
                name: "Mobile".to_string(),
                invitee_authority_id: None,
            }));
            state.next_toast_id += 1;
            state.toast_queue.enqueue(QueuedToast::new(
                state.next_toast_id,
                "Generating Mobile enrollment code…",
                ToastLevel::Info,
            ));
        }
        return;
    }

    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter => {
            if modal_state.can_submit() {
                commands.push(TuiCommand::Dispatch(
                    DispatchCommand::ImportDeviceEnrollmentOnMobile {
                        code: modal_state.code,
                    },
                ));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsDeviceImport(ref mut s) = modal {
                    s.code.push(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsDeviceImport(ref mut s) = modal {
                    s.code.pop();
                }
            });
        }
        _ => {}
    }
}

/// Handle create invitation modal keys (queue-based)
///
/// Field-focus navigation:
/// - ↑/↓: Navigate between Type, Message, and TTL fields
/// - ←/→: Change value (Type and TTL fields only)
/// - Typing: Edit message when Message field is focused
/// - Enter: Create invitation from any field
/// - Esc: Cancel
fn handle_create_invitation_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: CreateInvitationModalState,
    _source_screen: Screen,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter => {
            // Submit from any field
            if modal_state.receiver_id.trim().is_empty() {
                commands.push(TuiCommand::ShowToast {
                    message: "No receiver selected for invitation".to_string(),
                    level: ToastLevel::Error,
                });
                return;
            }

            // Convert type_index to stable invitation type string
            let invitation_type = match modal_state.type_index {
                0 => "guardian".to_string(),
                1 => "contact".to_string(),
                _ => "channel".to_string(),
            };

            commands.push(TuiCommand::Dispatch(DispatchCommand::CreateInvitation {
                receiver_id: modal_state.receiver_id.clone(),
                invitation_type,
                message: (!modal_state.message.trim().is_empty())
                    .then(|| modal_state.message.clone()),
                ttl_secs: modal_state.ttl_secs(),
            }));
            state.modal_queue.dismiss();
        }
        KeyCode::Up => {
            // Navigate to previous field
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ContactsCreate(ref mut s) = modal {
                    s.focus_prev();
                }
            });
        }
        KeyCode::Down => {
            // Navigate to next field
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ContactsCreate(ref mut s) = modal {
                    s.focus_next();
                }
            });
        }
        KeyCode::Left => {
            // Change value: cycle backward for Type and TTL fields
            match modal_state.focused_field {
                CreateInvitationField::Type => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::ContactsCreate(ref mut s) = modal {
                            s.type_prev();
                        }
                    });
                }
                CreateInvitationField::Ttl => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::ContactsCreate(ref mut s) = modal {
                            s.ttl_prev();
                        }
                    });
                }
                CreateInvitationField::Message => {
                    // No-op for message field (could move cursor left in future)
                }
            }
        }
        KeyCode::Right => {
            // Change value: cycle forward for Type and TTL fields
            match modal_state.focused_field {
                CreateInvitationField::Type => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::ContactsCreate(ref mut s) = modal {
                            s.type_next();
                        }
                    });
                }
                CreateInvitationField::Ttl => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::ContactsCreate(ref mut s) = modal {
                            s.ttl_next();
                        }
                    });
                }
                CreateInvitationField::Message => {
                    // No-op for message field (could move cursor right in future)
                }
            }
        }
        KeyCode::Char(c) => {
            // Typing only works in Message field
            if modal_state.focused_field == CreateInvitationField::Message {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ContactsCreate(ref mut s) = modal {
                        s.message.push(c);
                    }
                });
            }
        }
        KeyCode::Backspace => {
            // Backspace only works in Message field
            if modal_state.focused_field == CreateInvitationField::Message {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ContactsCreate(ref mut s) = modal {
                        s.message.pop();
                    }
                });
            }
        }
        _ => {}
    }
}

/// Handle guardian setup modal keys (queue-based)
fn handle_guardian_setup_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: GuardianSetupModalState,
) {
    match modal_state.step() {
        GuardianSetupStep::SelectContacts => {
            match key.code {
                KeyCode::Esc => {
                    state.modal_queue.dismiss();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.move_focus_up();
                        }
                    });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.move_focus_down();
                        }
                    });
                }
                KeyCode::Char(' ') => {
                    // Toggle selection
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.toggle_selection();
                        }
                    });
                }
                KeyCode::Enter => {
                    if modal_state.can_proceed_to_threshold() {
                        state.modal_queue.update_active(|modal| {
                            if let QueuedModal::GuardianSetup(ref mut s) = modal {
                                s.advance_to_threshold();
                            }
                        });
                    }
                }
                _ => {}
            }
        }
        GuardianSetupStep::ChooseThreshold => {
            match key.code {
                KeyCode::Esc => {
                    // Go back to contact selection
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.back_to_selection();
                        }
                    });
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.increment_k();
                        }
                    });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.decrement_k();
                        }
                    });
                }
                KeyCode::Enter => {
                    if modal_state.can_start_ceremony() {
                        // Dispatch command to start guardian setup ceremony
                        commands.push(TuiCommand::Dispatch(
                            DispatchCommand::StartGuardianCeremony {
                                contact_ids: modal_state.selected_contact_ids(),
                                threshold_k: modal_state.threshold_k(),
                            },
                        ));

                        // Dismiss modal after sending invitation (mirrors contact invitation flow)
                        state.modal_queue.dismiss();
                    }
                }
                _ => {}
            }
        }
        GuardianSetupStep::CeremonyInProgress
        | GuardianSetupStep::Complete
        | GuardianSetupStep::Error => {
            // During ceremony, allow escape to cancel once the ceremony has started.
            if key.code == KeyCode::Esc {
                if let Some(ceremony_id) = modal_state.ceremony_id().cloned() {
                    commands.push(TuiCommand::Dispatch(
                        DispatchCommand::CancelKeyRotationCeremony { ceremony_id },
                    ));
                    state.modal_queue.dismiss();
                } else {
                    state.modal_queue.dismiss();
                    state.next_toast_id += 1;
                    state.toast_queue.enqueue(QueuedToast::new(
                        state.next_toast_id,
                        "Guardian ceremony is still starting.",
                        ToastLevel::Info,
                    ));
                }
            }
        }
    }
}

/// Handle MFA setup modal keys (queue-based)
fn handle_mfa_setup_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: GuardianSetupModalState,
) {
    let is_ctrl_m =
        key.modifiers.ctrl() && matches!(key.code, KeyCode::Char('m') | KeyCode::Char('M'));
    match modal_state.step() {
        GuardianSetupStep::SelectContacts => {
            if is_ctrl_m {
                let mobile_id = state.settings.demo_mobile_device_id.clone();
                if !mobile_id.is_empty() {
                    let mut found = false;
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::MfaSetup(ref mut s) = modal {
                            found = s.select_by_id(&mobile_id);
                        }
                    });
                    if !found {
                        state.next_toast_id += 1;
                        state.toast_queue.enqueue(QueuedToast::new(
                            state.next_toast_id,
                            "Mobile device not found in the list yet.",
                            ToastLevel::Warning,
                        ));
                    }
                } else {
                    state.next_toast_id += 1;
                    state.toast_queue.enqueue(QueuedToast::new(
                        state.next_toast_id,
                        "Demo Mobile device id unavailable.",
                        ToastLevel::Warning,
                    ));
                }
                return;
            }

            match key.code {
                KeyCode::Esc => {
                    state.modal_queue.dismiss();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::MfaSetup(ref mut s) = modal {
                            s.move_focus_up();
                        }
                    });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::MfaSetup(ref mut s) = modal {
                            s.move_focus_down();
                        }
                    });
                }
                KeyCode::Char(' ') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::MfaSetup(ref mut s) = modal {
                            s.toggle_selection();
                        }
                    });
                }
                KeyCode::Enter => {
                    if modal_state.can_proceed_to_threshold() {
                        state.modal_queue.update_active(|modal| {
                            if let QueuedModal::MfaSetup(ref mut s) = modal {
                                s.advance_to_threshold();
                            }
                        });
                    }
                }
                _ => {}
            }
        }
        GuardianSetupStep::ChooseThreshold => match key.code {
            KeyCode::Esc => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::MfaSetup(ref mut s) = modal {
                        s.back_to_selection();
                    }
                });
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::MfaSetup(ref mut s) = modal {
                        s.increment_k();
                    }
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::MfaSetup(ref mut s) = modal {
                        s.decrement_k();
                    }
                });
            }
            KeyCode::Enter => {
                if modal_state.can_start_ceremony() {
                    commands.push(TuiCommand::Dispatch(DispatchCommand::StartMfaCeremony {
                        device_ids: modal_state.selected_contact_ids(),
                        threshold_k: modal_state.threshold_k(),
                    }));

                    // Dismiss modal after sending invitation (mirrors guardian invitation flow)
                    state.modal_queue.dismiss();
                }
            }
            _ => {}
        },
        GuardianSetupStep::CeremonyInProgress
        | GuardianSetupStep::Complete
        | GuardianSetupStep::Error => {
            if key.code == KeyCode::Esc {
                if let Some(ceremony_id) = modal_state.ceremony_id().cloned() {
                    commands.push(TuiCommand::Dispatch(
                        DispatchCommand::CancelKeyRotationCeremony { ceremony_id },
                    ));
                    state.modal_queue.dismiss();
                } else {
                    state.modal_queue.dismiss();
                    state.next_toast_id += 1;
                    state.toast_queue.enqueue(QueuedToast::new(
                        state.next_toast_id,
                        "Multifactor ceremony is still starting.",
                        ToastLevel::Info,
                    ));
                }
            }
        }
    }
}

/// Handle settings nickname suggestion modal keys (queue-based)
fn handle_settings_nickname_suggestion_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: NicknameSuggestionModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter => {
            if modal_state.can_submit() {
                commands.push(TuiCommand::Dispatch(
                    DispatchCommand::UpdateNicknameSuggestion {
                        nickname_suggestion: modal_state.value,
                    },
                ));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsNicknameSuggestion(ref mut s) = modal {
                    s.value.push(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsNicknameSuggestion(ref mut s) = modal {
                    s.value.pop();
                }
            });
        }
        _ => {}
    }
}

/// Handle settings add device modal keys (queue-based)
///
/// Supports two-step exchange: Tab switches between Name and InviteeAuthority fields.
/// If invitee_authority_id is provided, uses DeviceEnrollment choreography.
fn handle_settings_add_device_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: AddDeviceModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Tab => {
            // Switch between Name and InviteeAuthority fields
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsAddDevice(ref mut s) = modal {
                    s.focused_field = match s.focused_field {
                        AddDeviceField::Name => AddDeviceField::InviteeAuthority,
                        AddDeviceField::InviteeAuthority => AddDeviceField::Name,
                    };
                }
            });
        }
        KeyCode::Enter => {
            if modal_state.can_submit() {
                let invitee_authority_id = modal_state.invitee_authority().map(|s| s.to_string());
                commands.push(TuiCommand::Dispatch(DispatchCommand::AddDevice {
                    name: modal_state.name,
                    invitee_authority_id,
                }));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsAddDevice(ref mut s) = modal {
                    match s.focused_field {
                        AddDeviceField::Name => s.name.push(c),
                        AddDeviceField::InviteeAuthority => s.invitee_authority_id.push(c),
                    }
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsAddDevice(ref mut s) = modal {
                    match s.focused_field {
                        AddDeviceField::Name => {
                            s.name.pop();
                        }
                        AddDeviceField::InviteeAuthority => {
                            s.invitee_authority_id.pop();
                        }
                    }
                }
            });
        }
        _ => {}
    }
}

/// Handle settings remove device modal keys (queue-based)
fn handle_settings_remove_device_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ConfirmRemoveModalState,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
            state.modal_queue.dismiss();
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsRemoveDevice(ref mut s) = modal {
                    s.toggle_focus();
                }
            });
        }
        KeyCode::Enter => {
            if modal_state.confirm_focused {
                commands.push(TuiCommand::Dispatch(DispatchCommand::RemoveDevice {
                    device_id: modal_state.device_id,
                }));
            }
            state.modal_queue.dismiss();
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            commands.push(TuiCommand::Dispatch(DispatchCommand::RemoveDevice {
                device_id: modal_state.device_id,
            }));
            state.modal_queue.dismiss();
        }
        _ => {}
    }
}

/// Handle authority picker modal keys (queue-based)
///
/// Similar to contact select but dispatches SwitchAuthority on selection.
fn handle_authority_picker_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ContactSelectModalState,
) {
    let item_count = modal_state.contacts.len();
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::AuthorityPicker(ref mut s) = modal {
                    s.selected_index =
                        navigate_list(s.selected_index, s.contacts.len(), NavKey::Up);
                }
            });
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::AuthorityPicker(ref mut s) = modal {
                    s.selected_index =
                        navigate_list(s.selected_index, s.contacts.len(), NavKey::Down);
                }
            });
        }
        KeyCode::Enter => {
            if item_count > 0 {
                if let Some((authority_id, _)) =
                    modal_state.contacts.get(modal_state.selected_index)
                {
                    commands.push(TuiCommand::Dispatch(DispatchCommand::SwitchAuthority {
                        authority_id: authority_id.clone(),
                    }));
                }
            }
            state.modal_queue.dismiss();
        }
        _ => {}
    }
}

/// Handle device selection modal keys (for device removal)
///
/// - Esc: Cancel
/// - Up/Down/j/k: Navigate list (skips current device)
/// - Enter: Select device and show confirmation modal
fn handle_device_select_key_queue(
    state: &mut TuiState,
    _commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: DeviceSelectModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsDeviceSelect(ref mut s) = modal {
                    s.select_prev();
                }
            });
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsDeviceSelect(ref mut s) = modal {
                    s.select_next();
                }
            });
        }
        KeyCode::Enter => {
            // Get selected device and show confirmation modal
            if let Some(device) = modal_state.selected_device() {
                let device_id = device.id.clone();
                let display_name = device.name.clone();

                // Dismiss device select modal
                state.modal_queue.dismiss();

                // Enqueue confirmation modal
                use super::super::views::ConfirmRemoveModalState;
                state.modal_queue.enqueue(QueuedModal::SettingsRemoveDevice(
                    ConfirmRemoveModalState::for_device(&device_id, &display_name),
                ));
            }
        }
        _ => {}
    }
}

fn handle_device_enrollment_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: DeviceEnrollmentCeremonyModalState,
) {
    match key.code {
        KeyCode::Esc => {
            // If still in progress, Esc cancels the ceremony; otherwise, it just closes.
            if !modal_state.ceremony.is_complete && !modal_state.ceremony.has_failed {
                if let Some(ceremony_id) = modal_state.ceremony.ceremony_id {
                    commands.push(TuiCommand::Dispatch(
                        DispatchCommand::CancelKeyRotationCeremony { ceremony_id },
                    ));
                }
            }
            state.modal_queue.dismiss();
        }
        KeyCode::Char('c') => {
            // Copy enrollment code to clipboard (c or Cmd+C)
            if !modal_state.enrollment_code.is_empty()
                && copy_to_clipboard(&modal_state.enrollment_code).is_ok()
            {
                // Update state to show "copied" feedback
                state.modal_queue.update_active(|m| {
                    if let QueuedModal::SettingsDeviceEnrollment(s) = m {
                        s.set_copied();
                    }
                });
                state.toast_success("Copied to clipboard");
            }
        }
        KeyCode::Char('m' | 'M') if key.modifiers.ctrl() => {
            // Demo mode only: simulate mobile device importing this enrollment code
            let is_demo = !state.settings.demo_mobile_device_id.is_empty();
            let is_pending = !modal_state.ceremony.is_complete && !modal_state.ceremony.has_failed;
            if is_demo && is_pending && !modal_state.enrollment_code.is_empty() {
                commands.push(TuiCommand::Dispatch(
                    DispatchCommand::ImportDeviceEnrollmentOnMobile {
                        code: modal_state.enrollment_code.clone(),
                    },
                ));
                state.toast_info("Sending code to Mobile agent...");
            }
        }
        _ => {}
    }
}
