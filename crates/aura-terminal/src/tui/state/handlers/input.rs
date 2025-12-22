//! Input handlers for mouse, paste, and insert mode events

use aura_core::effects::terminal::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};

use crate::tui::screens::Screen;

use super::super::commands::{DispatchCommand, TuiCommand};
use super::super::modal_queue::QueuedModal;
use super::super::views::{BlockFocus, ChatFocus};
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
                Screen::Block => {
                    // Scroll messages up (show older messages)
                    if state.block.focus == BlockFocus::Messages && state.block.message_scroll > 0 {
                        state.block.message_scroll = state.block.message_scroll.saturating_sub(3);
                    }
                }
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
                Screen::Block => {
                    // Scroll messages down (show newer messages)
                    if state.block.focus == BlockFocus::Messages {
                        state.block.message_scroll = state.block.message_scroll.saturating_add(3);
                    }
                }
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
pub fn handle_paste_event(state: &mut TuiState, _commands: &mut Vec<TuiCommand>, text: String) {
    // Only handle paste if we're in insert mode
    if !state.is_insert_mode() {
        return;
    }

    // Handle modal input fields first
    if let Some(modal) = state.modal_queue.current_mut() {
        match modal {
            // Invitation import modals (both contacts and invitations screens)
            QueuedModal::InvitationsImport(modal_state) => {
                modal_state.code.push_str(&text);
                return;
            }
            QueuedModal::ContactsImport(modal_state) => {
                modal_state.code.push_str(&text);
                return;
            }

            // Chat modals with text input
            QueuedModal::ChatCreate(modal_state) => {
                // Paste into active field (name or topic)
                if modal_state.active_field == 0 {
                    modal_state.name.push_str(&text);
                } else {
                    modal_state.topic.push_str(&text);
                }
                return;
            }
            QueuedModal::ChatTopic(modal_state) => {
                modal_state.value.push_str(&text);
                return;
            }

            // Contact nickname modal
            QueuedModal::ContactsNickname(modal_state) => {
                modal_state.value.push_str(&text);
                return;
            }

            // Settings display name modal
            QueuedModal::SettingsDisplayName(modal_state) => {
                modal_state.value.push_str(&text);
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
            | QueuedModal::InvitationsCreate(_)
            | QueuedModal::InvitationsCode(_)
            | QueuedModal::SettingsThreshold(_)
            | QueuedModal::SettingsAddDevice(_)
            | QueuedModal::SettingsRemoveDevice(_)
            | QueuedModal::BlockInvite(_) => {}
        }
    }

    // Handle screen-level input buffers
    match state.screen() {
        Screen::Block => {
            if state.block.focus == BlockFocus::Input {
                state.block.input_buffer.push_str(&text);
            }
        }
        Screen::Chat => {
            if state.chat.focus == ChatFocus::Input {
                state.chat.input_buffer.push_str(&text);
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
            Screen::Block => {
                state.block.insert_mode = false;
                state.block.insert_mode_entry_char = None;
            }
            Screen::Chat => {
                state.chat.insert_mode = false;
                state.chat.insert_mode_entry_char = None;
            }
            _ => {}
        }
        return;
    }

    // Get the entry char to check if we need to consume it
    let entry_char = match screen {
        Screen::Block => state.block.insert_mode_entry_char,
        Screen::Chat => state.chat.insert_mode_entry_char,
        _ => None,
    };

    match key.code {
        KeyCode::Char(c) => {
            // If this char matches the entry char, consume it but don't add to buffer
            if entry_char == Some(c) {
                match screen {
                    Screen::Block => state.block.insert_mode_entry_char = None,
                    Screen::Chat => state.chat.insert_mode_entry_char = None,
                    _ => {}
                }
            } else {
                // Clear entry char and add char to buffer
                match screen {
                    Screen::Block => {
                        state.block.insert_mode_entry_char = None;
                        state.block.input_buffer.push(c);
                    }
                    Screen::Chat => {
                        state.chat.insert_mode_entry_char = None;
                        state.chat.input_buffer.push(c);
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Backspace => match screen {
            Screen::Block => {
                state.block.insert_mode_entry_char = None;
                state.block.input_buffer.pop();
            }
            Screen::Chat => {
                state.chat.insert_mode_entry_char = None;
                state.chat.input_buffer.pop();
            }
            _ => {}
        },
        KeyCode::Enter => {
            match screen {
                Screen::Block => {
                    if !state.block.input_buffer.is_empty() {
                        let content = state.block.input_buffer.clone();
                        state.block.input_buffer.clear();
                        commands.push(TuiCommand::Dispatch(DispatchCommand::SendBlockMessage {
                            content,
                        }));
                        // Exit insert mode after sending
                        state.block.insert_mode = false;
                        state.block.insert_mode_entry_char = None;
                        state.block.focus = BlockFocus::Residents;
                    }
                }
                Screen::Chat => {
                    if !state.chat.input_buffer.is_empty() {
                        let content = state.chat.input_buffer.clone();
                        state.chat.input_buffer.clear();
                        commands.push(TuiCommand::Dispatch(DispatchCommand::SendChatMessage {
                            content,
                        }));
                        // Exit insert mode after sending
                        state.chat.insert_mode = false;
                        state.chat.insert_mode_entry_char = None;
                        state.chat.focus = ChatFocus::Messages;
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}
