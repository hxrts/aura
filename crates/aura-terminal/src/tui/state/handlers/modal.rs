//! Modal keyboard handlers
//!
//! All queue-based modal key event handlers.

use aura_core::effects::terminal::{KeyCode, KeyEvent};

use crate::tui::navigation::{navigate_list, NavKey};
use crate::tui::screens::Screen;

use super::super::commands::{DispatchCommand, TuiCommand};
use super::super::modal_queue::{
    ChatMemberSelectModalState, ConfirmAction, ContactSelectModalState, QueuedModal,
};
use super::super::toast::ToastLevel;
use super::super::views::{
    AccountSetupModalState, AddDeviceModalState, ConfirmRemoveModalState, CreateChannelModalState,
    CreateInvitationField, CreateInvitationModalState, DeviceEnrollmentCeremonyModalState,
    DisplayNameModalState, GuardianSetupModalState, GuardianSetupStep, ImportInvitationModalState,
    NicknameModalState, TopicModalState,
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
        // Block screen modals
        QueuedModal::BlockInvite(modal_state) => {
            handle_block_invite_key_queue(state, commands, key, modal_state);
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
        QueuedModal::ContactsCode(_) => {
            // Code display modal is read-only - just Esc to dismiss
            if key.code == KeyCode::Esc {
                state.modal_queue.dismiss();
            }
        }
        QueuedModal::GuardianSetup(modal_state) => {
            handle_guardian_setup_key_queue(state, commands, key, modal_state);
        }
        // Settings screen modals
        QueuedModal::SettingsDisplayName(modal_state) => {
            handle_settings_display_name_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::SettingsAddDevice(modal_state) => {
            handle_settings_add_device_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::SettingsDeviceEnrollment(modal_state) => {
            handle_device_enrollment_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::SettingsRemoveDevice(modal_state) => {
            handle_settings_remove_device_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::ChatMemberSelect(modal_state) => {
            handle_chat_member_select_key_queue(state, commands, key, modal_state);
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
                    s.display_name.push(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::AccountSetup(ref mut s) = modal {
                    s.display_name.pop();
                }
            });
        }
        KeyCode::Enter => {
            if current_state.can_submit() {
                let name = current_state.display_name.clone();
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

/// Handle block invite modal keys (queue-based)
///
/// This modal is fully driven by the queued modal state. The contacts list is snapshotted
/// when the modal is opened.
fn handle_block_invite_key_queue(
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
                if let QueuedModal::BlockInvite(ref mut s) = modal {
                    s.selected_index =
                        navigate_list(s.selected_index, s.contacts.len(), NavKey::Up);
                }
            });
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::BlockInvite(ref mut s) = modal {
                    s.selected_index =
                        navigate_list(s.selected_index, s.contacts.len(), NavKey::Down);
                }
            });
        }
        KeyCode::Enter => {
            if let Some(contact_id) = modal_state.focused_contact_id() {
                commands.push(TuiCommand::Dispatch(DispatchCommand::InviteToBlock {
                    contact_id: contact_id.to_string(),
                }));
                state.modal_queue.dismiss();
            }
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
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Char('m') | KeyCode::Char('M') => {
            commands.push(TuiCommand::Dispatch(DispatchCommand::OpenChatMemberSelect));
        }
        KeyCode::Tab => {
            // Toggle between name and topic fields
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ChatCreate(ref mut s) = modal {
                    s.active_field = (s.active_field + 1) % 2;
                }
            });
        }
        KeyCode::Enter => {
            if modal_state.can_submit() {
                let topic = if modal_state.topic.trim().is_empty() {
                    None
                } else {
                    Some(modal_state.topic.clone())
                };

                commands.push(TuiCommand::Dispatch(DispatchCommand::CreateChannel {
                    name: modal_state.name.clone(),
                    topic,
                    members: modal_state.member_ids.clone(),
                }));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(c) => {
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
        KeyCode::Backspace => {
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
            draft.member_ids = modal_state.picker.selected_ids.clone();
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
                topic: modal_state.value.clone(),
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
                    contact_id: modal_state.contact_id.clone(),
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
                    code: modal_state.code.clone(),
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
    match modal_state.step {
        GuardianSetupStep::SelectContacts => {
            match key.code {
                KeyCode::Esc => {
                    state.modal_queue.dismiss();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            if s.focused_index > 0 {
                                s.focused_index -= 1;
                            }
                        }
                    });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            if s.focused_index + 1 < s.contacts.len() {
                                s.focused_index += 1;
                            }
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
                                s.step = GuardianSetupStep::ChooseThreshold;
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
                            s.step = GuardianSetupStep::SelectContacts;
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
                                threshold_k: modal_state.threshold_k,
                            },
                        ));

                        // Keep the modal queued and transition into the in-progress step.
                        // `ceremony_id` is filled asynchronously by the shell.
                        state.modal_queue.update_active(|modal| {
                            if let QueuedModal::GuardianSetup(ref mut s) = modal {
                                s.begin_ceremony();
                            }
                        });
                    }
                }
                _ => {}
            }
        }
        GuardianSetupStep::CeremonyInProgress => {
            // During ceremony, allow escape to cancel once the ceremony has started.
            if key.code == KeyCode::Esc {
                if let Some(ceremony_id) = modal_state.ceremony.ceremony_id.clone() {
                    commands.push(TuiCommand::Dispatch(
                        DispatchCommand::CancelKeyRotationCeremony { ceremony_id },
                    ));
                    state.modal_queue.dismiss();
                } else {
                    // Ceremony is still starting; keep the modal open and show a hint.
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.error = Some("Starting guardian ceremony…".to_string());
                        }
                    });
                }
            }
        }
    }
}

/// Handle settings display name modal keys (queue-based)
fn handle_settings_display_name_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: DisplayNameModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter => {
            if modal_state.can_submit() {
                commands.push(TuiCommand::Dispatch(DispatchCommand::UpdateDisplayName {
                    display_name: modal_state.value.clone(),
                }));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsDisplayName(ref mut s) = modal {
                    s.value.push(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsDisplayName(ref mut s) = modal {
                    s.value.pop();
                }
            });
        }
        _ => {}
    }
}

/// Handle settings add device modal keys (queue-based)
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
        KeyCode::Enter => {
            if modal_state.can_submit() {
                commands.push(TuiCommand::Dispatch(DispatchCommand::AddDevice {
                    name: modal_state.name.clone(),
                }));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsAddDevice(ref mut s) = modal {
                    s.name.push(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsAddDevice(ref mut s) = modal {
                    s.name.pop();
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
                    device_id: modal_state.device_id.clone(),
                }));
            }
            state.modal_queue.dismiss();
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            commands.push(TuiCommand::Dispatch(DispatchCommand::RemoveDevice {
                device_id: modal_state.device_id.clone(),
            }));
            state.modal_queue.dismiss();
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
                if let Some(ceremony_id) = modal_state.ceremony.ceremony_id.clone() {
                    commands.push(TuiCommand::Dispatch(
                        DispatchCommand::CancelKeyRotationCeremony { ceremony_id },
                    ));
                }
            }
            state.modal_queue.dismiss();
        }
        _ => {}
    }
}
