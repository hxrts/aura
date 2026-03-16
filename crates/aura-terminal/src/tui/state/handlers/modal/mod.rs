//! Modal keyboard handlers
//!
//! All queue-based modal key event handlers, organized by domain:
//! - `account`: Account setup modal
//! - `ceremony`: Guardian setup and MFA setup ceremony modals
//! - `chat`: Chat create, topic, member select, help, and confirm modals
//! - `contacts`: Contact select, guardian select, nickname, invitation modals
//! - `neighborhood`: Moderator, access override, and capability config modals
//! - `settings`: Nickname suggestion, device management, authority picker modals
//!
//! Note: Modal state types are passed by value from the dispatcher's match arms.
//! This is intentional for the queue-based modal system where states are moved
//! out of the enum for handling.

#![allow(clippy::needless_pass_by_value)]

mod account;
mod ceremony;
mod chat;
mod contacts;
mod neighborhood;
mod settings;

use aura_core::effects::terminal::{KeyCode, KeyEvent};
use aura_core::AuthorityId;

use crate::tui::components::copy_to_clipboard;
use crate::tui::navigation::NavKey;
use crate::tui::screens::Screen;

use super::super::commands::{DispatchCommand, TuiCommand};
use super::super::modal_queue::QueuedModal;
use super::super::toast::{QueuedToast, ToastLevel};
use super::super::TuiState;

use account::handle_account_setup_key_queue;
use ceremony::{handle_guardian_setup_key_queue, handle_mfa_setup_key_queue};
use chat::{
    handle_chat_create_key_queue, handle_chat_member_select_key_queue,
    handle_chat_topic_key_queue, handle_confirm_modal_key_queue, handle_help_modal_key_queue,
};
use contacts::{
    handle_contact_select_key_queue, handle_create_invitation_key_queue,
    handle_guardian_select_key_queue, handle_import_invitation_key_queue,
    handle_nickname_key_queue,
};
use neighborhood::{
    handle_neighborhood_access_override_modal_key_queue,
    handle_neighborhood_capability_config_modal_key_queue,
    handle_neighborhood_moderator_modal_key_queue,
};
use settings::{
    handle_authority_picker_key_queue, handle_device_enrollment_key_queue,
    handle_device_import_key_queue, handle_device_select_key_queue,
    handle_settings_add_device_key_queue, handle_settings_nickname_suggestion_key_queue,
    handle_settings_remove_device_key_queue,
};

// ── Shared helpers ──────────────────────────────────────────────────────────

fn parse_authority_id(state: &mut TuiState, raw: &str, action: &str) -> Option<AuthorityId> {
    match raw.parse::<AuthorityId>() {
        Ok(id) => Some(id),
        Err(_) => {
            state.toast_error(format!("Invalid authority ID for {action}"));
            None
        }
    }
}

fn dismiss_on_escape(state: &mut TuiState, code: &KeyCode) -> bool {
    if matches!(code, KeyCode::Esc) {
        state.modal_queue.dismiss();
        true
    } else {
        false
    }
}

fn list_nav_from_key(code: &KeyCode) -> Option<NavKey> {
    match code {
        KeyCode::Up | KeyCode::Char('k') => Some(NavKey::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(NavKey::Down),
        _ => None,
    }
}

fn digit_alias_from_key(code: &KeyCode) -> Option<char> {
    match code {
        // Some PTY paths surface these as control bytes in raw mode.
        KeyCode::Char('\u{1}') => Some('1'),
        KeyCode::Char('\u{2}') => Some('2'),
        _ => None,
    }
}

fn modal_text_char_from_key(code: &KeyCode) -> Option<char> {
    if let Some(alias) = digit_alias_from_key(code) {
        return Some(alias);
    }
    match code {
        KeyCode::Char(c) if !c.is_control() => Some(*c),
        _ => None,
    }
}

fn warn_no_selection(state: &mut TuiState, entity: &str) {
    state.toast_warning(format!("No {entity} selected"));
}

fn handle_ceremony_escape(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    ceremony_id: Option<String>,
    pending_message: &str,
) {
    if let Some(ceremony_id) = ceremony_id {
        commands.push(TuiCommand::Dispatch(
            DispatchCommand::CancelKeyRotationCeremony {
                ceremony_id: ceremony_id.into(),
            },
        ));
        state.modal_queue.dismiss();
    } else {
        state.modal_queue.dismiss();
        state.next_toast_id += 1;
        state.toast_queue.enqueue(QueuedToast::new(
            state.next_toast_id,
            pending_message,
            ToastLevel::Info,
        ));
    }
}

// ── Main dispatch ───────────────────────────────────────────────────────────

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
            // Code display modal: Esc/Enter to dismiss, c to copy
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
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
        QueuedModal::NeighborhoodModeratorAssignment(modal_state) => {
            handle_neighborhood_moderator_modal_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::NeighborhoodAccessOverride(modal_state) => {
            handle_neighborhood_access_override_modal_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::NeighborhoodCapabilityConfig(modal_state) => {
            handle_neighborhood_capability_config_modal_key_queue(
                state,
                commands,
                key,
                modal_state,
            );
        }
    }
}
