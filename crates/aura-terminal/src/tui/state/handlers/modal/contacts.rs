//! Contact and invitation modal handlers
//!
//! Handles contact select, guardian select, nickname, import invitation,
//! create invitation, and code display modals.

use aura_core::effects::terminal::{KeyCode, KeyEvent};

use crate::tui::navigation::navigate_list;
use crate::tui::screens::Screen;

use super::super::super::commands::{DispatchCommand, TuiCommand};
use super::super::super::modal_queue::{ContactSelectModalState, QueuedModal};
use super::super::super::views::{
    CreateInvitationField, CreateInvitationModalState, ImportInvitationModalState,
    NicknameModalState,
};
use super::super::super::TuiState;
use super::{dismiss_on_escape, list_nav_from_key, modal_text_char_from_key};

/// Handle guardian select modal keys (queue-based)
pub(super) fn handle_guardian_select_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ContactSelectModalState,
) {
    if dismiss_on_escape(state, &key.code) {
        return;
    }

    if let Some(nav) = list_nav_from_key(&key.code) {
        state.modal_queue.update_active(|modal| {
            if let QueuedModal::GuardianSelect(ref mut s) = modal {
                s.selected_index = navigate_list(s.selected_index, s.contacts.len(), nav);
            }
        });
        return;
    }

    match key.code {
        KeyCode::Enter => {
            if let Some((contact_id, _)) = modal_state.contacts.get(modal_state.selected_index) {
                commands.push(TuiCommand::Dispatch(DispatchCommand::AddGuardian {
                    contact_id: contact_id.to_string().into(),
                }));
                state.modal_queue.dismiss();
            }
        }
        _ => {}
    }
}

/// Handle contact select modal keys (queue-based)
pub(super) fn handle_contact_select_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ContactSelectModalState,
) {
    if dismiss_on_escape(state, &key.code) {
        return;
    }

    if let Some(nav) = list_nav_from_key(&key.code) {
        state.modal_queue.update_active(|modal| {
            if let QueuedModal::ContactSelect(ref mut s) = modal {
                s.selected_index = navigate_list(s.selected_index, s.contacts.len(), nav);
            }
        });
        return;
    }

    let contact_count = modal_state.contacts.len();
    match key.code {
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

/// Handle nickname edit modal keys (queue-based)
pub(super) fn handle_nickname_key_queue(
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
                    contact_id: modal_state.contact_id.into(),
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
pub(super) fn handle_import_invitation_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ImportInvitationModalState,
    _source_screen: Screen,
) {
    let has_demo_alice_code = !state.contacts.demo_alice_code.is_empty();
    let has_demo_carol_code = !state.contacts.demo_carol_code.is_empty();
    let empty_code = modal_state.code.is_empty();

    // Demo shortcuts: a/l, A/L, or Ctrl+A / Ctrl+L fill Alice/Carol invite codes.
    //
    // These are handled at the state machine layer so they work consistently
    // for the Contacts invitation import workflow.
    let is_ctrl_a = (key.modifiers.ctrl()
        && matches!(key.code, KeyCode::Char('a') | KeyCode::Char('A')))
        // Some terminals report Ctrl+a as the control character (SOH, 0x01) with no modifiers.
        || (empty_code && has_demo_alice_code && matches!(key.code, KeyCode::Char('\u{1}')));
    let is_ctrl_l = (key.modifiers.ctrl()
        && matches!(key.code, KeyCode::Char('l') | KeyCode::Char('L')))
        // Some terminals report Ctrl+l as the control character (FF, 0x0c) with no modifiers.
        || (empty_code && has_demo_carol_code && matches!(key.code, KeyCode::Char('\u{c}')));
    // Harness fallback: some PTY paths do not surface Ctrl modifiers reliably.
    // In demo mode, allow uppercase A/L on an empty field as deterministic autofill.
    let is_demo_shift_a =
        empty_code && has_demo_alice_code && matches!(key.code, KeyCode::Char('A'));
    let is_demo_shift_l =
        empty_code && has_demo_carol_code && matches!(key.code, KeyCode::Char('L'));
    let is_demo_lower_a =
        empty_code && has_demo_alice_code && matches!(key.code, KeyCode::Char('a'));
    let is_demo_lower_l =
        empty_code && has_demo_carol_code && matches!(key.code, KeyCode::Char('l'));
    if is_ctrl_a
        || is_ctrl_l
        || is_demo_lower_a
        || is_demo_lower_l
        || is_demo_shift_a
        || is_demo_shift_l
    {
        // Dismiss the demo hint toast since the user used a shortcut
        state.toast_queue.dismiss();

        let code = if is_ctrl_a || is_demo_lower_a || is_demo_shift_a {
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
        KeyCode::Char(_) => {
            let Some(c) = modal_text_char_from_key(&key.code) else {
                return;
            };
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
/// - Up/Down: Navigate between text fields and TTL
/// - Left/Right: Change value (TTL field only)
/// - Typing: Edit text fields when focused
/// - Enter: Create invitation from any field
/// - Esc: Cancel
pub(super) fn handle_create_invitation_key_queue(
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
            // Contacts page always creates contact invitations
            let invitation_type = super::super::super::commands::InvitationKind::Contact;

            commands.push(TuiCommand::Dispatch(DispatchCommand::CreateInvitation {
                receiver_id: None,
                invitation_type,
                nickname: (!modal_state.nickname.trim().is_empty())
                    .then(|| modal_state.nickname.clone()),
                receiver_nickname: (!modal_state.receiver_nickname.trim().is_empty())
                    .then(|| modal_state.receiver_nickname.clone()),
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
            // Change value: cycle backward for TTL field
            match modal_state.focused_field {
                CreateInvitationField::Nickname => {}
                CreateInvitationField::ReceiverNickname => {}
                CreateInvitationField::Message => {}
                CreateInvitationField::Ttl => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::ContactsCreate(ref mut s) = modal {
                            s.ttl_prev();
                        }
                    });
                }
            }
        }
        KeyCode::Right => {
            // Change value: cycle forward for TTL field
            match modal_state.focused_field {
                CreateInvitationField::Nickname => {}
                CreateInvitationField::ReceiverNickname => {}
                CreateInvitationField::Message => {}
                CreateInvitationField::Ttl => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::ContactsCreate(ref mut s) = modal {
                            s.ttl_next();
                        }
                    });
                }
            }
        }
        KeyCode::Char(c) => match modal_state.focused_field {
            CreateInvitationField::Nickname => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ContactsCreate(ref mut s) = modal {
                        s.nickname.push(c);
                    }
                });
            }
            CreateInvitationField::ReceiverNickname => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ContactsCreate(ref mut s) = modal {
                        s.receiver_nickname.push(c);
                    }
                });
            }
            CreateInvitationField::Message => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ContactsCreate(ref mut s) = modal {
                        s.message.push(c);
                    }
                });
            }
            CreateInvitationField::Ttl => {}
        },
        KeyCode::Backspace => match modal_state.focused_field {
            CreateInvitationField::Nickname => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ContactsCreate(ref mut s) = modal {
                        s.nickname.pop();
                    }
                });
            }
            CreateInvitationField::ReceiverNickname => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ContactsCreate(ref mut s) = modal {
                        s.receiver_nickname.pop();
                    }
                });
            }
            CreateInvitationField::Message => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::ContactsCreate(ref mut s) = modal {
                        s.message.pop();
                    }
                });
            }
            CreateInvitationField::Ttl => {}
        },
        _ => {}
    }
}
