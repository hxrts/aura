//! Input handlers for mouse, paste, and insert mode events
//!
//! Event types (KeyEvent, MouseEvent) are passed by value following standard
//! event handler conventions.

#![allow(clippy::needless_pass_by_value)]

use aura_core::effects::terminal::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};

use crate::tui::screens::Screen;

use super::super::commands::{DispatchCommand, TuiCommand};
use super::super::modal_queue::QueuedModal;
use super::super::views::{ChatFocus, DetailFocus, NeighborhoodMode};
use super::super::TuiState;

/// Handle a mouse event
///
/// Primarily handles scroll events for navigation in lists and message views.
pub fn handle_mouse_event(
    state: &mut TuiState,
    _commands: &mut Vec<TuiCommand>,
    mouse: MouseEvent,
) {
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            // Scroll up in the current list/view
            match state.screen() {
                Screen::Chat => {
                    // Scroll messages up (show older messages)
                    if state.chat.focus == ChatFocus::Messages && state.chat.message_scroll > 0 {
                        state.chat.message_scroll = state.chat.message_scroll.saturating_sub(3);
                    }
                }
                Screen::Contacts => {
                    // Navigate up in contacts list
                    if state.contacts.selected_index > 0 {
                        state.contacts.selected_index =
                            state.contacts.selected_index.saturating_sub(1);
                    }
                }
                Screen::Neighborhood => {
                    if state.neighborhood.mode == NeighborhoodMode::Detail
                        && state.neighborhood.detail_focus == DetailFocus::Messages
                        && state.neighborhood.message_scroll > 0
                    {
                        state.neighborhood.message_scroll =
                            state.neighborhood.message_scroll.saturating_sub(3);
                    }
                }
                Screen::Settings => {
                    // Navigate up in settings list
                    if state.settings.selected_index > 0 {
                        state.settings.selected_index =
                            state.settings.selected_index.saturating_sub(1);
                    }
                }
                _ => {}
            }
        }
        MouseEventKind::ScrollDown => {
            // Scroll down in the current list/view
            match state.screen() {
                Screen::Chat => {
                    // Scroll messages down (show newer messages)
                    if state.chat.focus == ChatFocus::Messages {
                        state.chat.message_scroll = state.chat.message_scroll.saturating_add(3);
                    }
                }
                Screen::Contacts => {
                    // Navigate down in contacts list
                    state.contacts.selected_index = state.contacts.selected_index.saturating_add(1);
                }
                Screen::Neighborhood => {
                    if state.neighborhood.mode == NeighborhoodMode::Detail
                        && state.neighborhood.detail_focus == DetailFocus::Messages
                    {
                        state.neighborhood.message_scroll =
                            state.neighborhood.message_scroll.saturating_add(3);
                    }
                }
                Screen::Settings => {
                    // Navigate down in settings list
                    state.settings.selected_index = state.settings.selected_index.saturating_add(1);
                }
                _ => {}
            }
        }
        // Mouse clicks and drags are not handled in this TUI
        // as keyboard navigation is the primary interaction mode
        _ => {}
    }
}

/// Handle a paste event
///
/// Inserts pasted text into the current input buffer if in insert mode.
pub fn handle_paste_event(state: &mut TuiState, _commands: &mut Vec<TuiCommand>, text: &str) {
    // Only handle paste if we're in insert mode
    if !state.is_insert_mode() {
        return;
    }

    // Handle modal input fields first
    if let Some(modal) = state.modal_queue.current_mut() {
        match modal {
            // Invitation import modal (Contacts workflow)
            QueuedModal::ContactsImport(modal_state) => {
                modal_state.code.push_str(text);
                return;
            }

            // Chat modals with text input
            QueuedModal::ChatCreate(modal_state) => {
                // Paste into active field (name or topic)
                if modal_state.active_field == 0 {
                    modal_state.name.push_str(text);
                } else {
                    modal_state.topic.push_str(text);
                }
                return;
            }
            QueuedModal::ChatTopic(modal_state) => {
                modal_state.value.push_str(text);
                return;
            }

            // Contact nickname modal
            QueuedModal::ContactsNickname(modal_state) => {
                modal_state.value.push_str(text);
                return;
            }

            // Settings display name modal
            QueuedModal::SettingsDisplayName(modal_state) => {
                modal_state.value.push_str(text);
                return;
            }
            QueuedModal::NeighborhoodHomeCreate(modal_state) => {
                if modal_state.active_field == 0 {
                    modal_state.name.push_str(text);
                } else {
                    modal_state.description.push_str(text);
                }
                return;
            }

            // These modals don't have direct text input
            QueuedModal::AccountSetup(_)
            | QueuedModal::Help { .. }
            | QueuedModal::Confirm { .. }
            | QueuedModal::GuardianSelect(_)
            | QueuedModal::ContactSelect(_)
            | QueuedModal::ChatInfo(_)
            | QueuedModal::ContactsCreate(_)
            | QueuedModal::ContactsCode(_)
            | QueuedModal::GuardianSetup(_)
            | QueuedModal::MfaSetup(_)
            | QueuedModal::SettingsAddDevice(_)
            | QueuedModal::SettingsDeviceImport(_)
            | QueuedModal::SettingsDeviceEnrollment(_)
            | QueuedModal::SettingsRemoveDevice(_)
            | QueuedModal::AuthorityPicker(_)
            | QueuedModal::ChatMemberSelect(_) => {}
        }
    }

    // Handle screen-level input buffers
    match state.screen() {
        Screen::Chat => {
            if state.chat.focus == ChatFocus::Input {
                state.chat.input_buffer.push_str(text);
            }
        }
        Screen::Neighborhood => {
            if state.neighborhood.mode == NeighborhoodMode::Detail
                && state.neighborhood.detail_focus == DetailFocus::Input
            {
                state.neighborhood.input_buffer.push_str(text);
            }
        }
        _ => {}
    }
}

/// Handle insert mode key events
pub fn handle_insert_mode_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    // Capture screen type once to avoid borrow conflicts
    let screen = state.screen();

    // Escape exits insert mode
    if key.code == KeyCode::Esc {
        match screen {
            Screen::Chat => {
                state.chat.insert_mode = false;
                state.chat.insert_mode_entry_char = None;
            }
            Screen::Neighborhood => {
                state.neighborhood.insert_mode = false;
                state.neighborhood.insert_mode_entry_char = None;
                state.neighborhood.detail_focus = DetailFocus::Messages;
            }
            _ => {}
        }
        return;
    }

    // Get the entry char to check if we need to consume it
    let entry_char = match screen {
        Screen::Chat => state.chat.insert_mode_entry_char,
        Screen::Neighborhood => state.neighborhood.insert_mode_entry_char,
        _ => None,
    };

    match key.code {
        KeyCode::Char(c) => {
            // If this char matches the entry char, consume it but don't add to buffer
            if entry_char == Some(c) {
                match screen {
                    Screen::Chat => state.chat.insert_mode_entry_char = None,
                    Screen::Neighborhood => state.neighborhood.insert_mode_entry_char = None,
                    _ => {}
                }
            } else {
                // Clear entry char and add char to buffer
                match screen {
                    Screen::Chat => {
                        state.chat.insert_mode_entry_char = None;
                        state.chat.input_buffer.push(c);
                    }
                    Screen::Neighborhood => {
                        state.neighborhood.insert_mode_entry_char = None;
                        state.neighborhood.input_buffer.push(c);
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Backspace => match screen {
            Screen::Chat => {
                state.chat.insert_mode_entry_char = None;
                state.chat.input_buffer.pop();
            }
            Screen::Neighborhood => {
                state.neighborhood.insert_mode_entry_char = None;
                state.neighborhood.input_buffer.pop();
            }
            _ => {}
        },
        KeyCode::Enter => match screen {
            Screen::Chat => {
                if !state.chat.input_buffer.is_empty() {
                    let content = state.chat.input_buffer.clone();
                    state.chat.input_buffer.clear();
                    commands.push(TuiCommand::Dispatch(DispatchCommand::SendChatMessage {
                        content,
                    }));
                    // Stay in insert mode so user can continue typing
                }
            }
            Screen::Neighborhood => {
                if !state.neighborhood.input_buffer.is_empty() {
                    let content = state.neighborhood.input_buffer.clone();
                    state.neighborhood.input_buffer.clear();
                    commands.push(TuiCommand::Dispatch(DispatchCommand::SendHomeMessage {
                        content,
                    }));
                    // Stay in insert mode so user can continue typing
                }
            }
            _ => {}
        },
        _ => {}
    }
}
