//! Input handlers for mouse, paste, and insert mode events
//!
//! Event types (KeyEvent, MouseEvent) are passed by value following standard
//! event handler conventions.

#![allow(clippy::needless_pass_by_value)]

use aura_core::effects::terminal::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};

use crate::tui::layout::dim;
use crate::tui::screens::Screen;

use super::super::commands::{DispatchCommand, TuiCommand};
use super::super::modal_queue::QueuedModal;
use super::super::views::{ChatFocus, DetailFocus};
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
            // For message-oriented screens, mouse scroll always scrolls messages
            // (unlike keyboard which respects focus for panel navigation)
            match state.screen() {
                Screen::Chat => {
                    // Scroll messages up (show older messages)
                    // scroll_offset: 0 = at bottom (latest), higher = scrolled up (older)
                    // Mouse scroll always affects messages regardless of keyboard focus
                    let max_scroll = state
                        .chat
                        .message_count
                        .saturating_sub(dim::VISIBLE_MESSAGE_ROWS as usize);
                    if state.chat.message_scroll < max_scroll {
                        state.chat.message_scroll =
                            state.chat.message_scroll.saturating_add(3).min(max_scroll);
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
                    // No dedicated scroll region on Neighborhood; keep mouse wheel a no-op here.
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
            // For message-oriented screens, mouse scroll always scrolls messages
            match state.screen() {
                Screen::Chat => {
                    // Scroll messages down (show newer messages, toward bottom)
                    // scroll_offset: 0 = at bottom (latest), higher = scrolled up (older)
                    // Mouse scroll always affects messages regardless of keyboard focus
                    if state.chat.message_scroll > 0 {
                        state.chat.message_scroll = state.chat.message_scroll.saturating_sub(3);
                    }
                }
                Screen::Contacts => {
                    // Navigate down in contacts list
                    state.contacts.selected_index = state.contacts.selected_index.saturating_add(1);
                }
                Screen::Neighborhood => {
                    // No dedicated scroll region on Neighborhood; keep mouse wheel a no-op here.
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
/// Inserts pasted text into modal fields or the current input buffer.
pub fn handle_paste_event(state: &mut TuiState, _commands: &mut Vec<TuiCommand>, text: &str) {
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

            // Settings nickname suggestion modal
            QueuedModal::SettingsNicknameSuggestion(modal_state) => {
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
            QueuedModal::NeighborhoodCapabilityConfig(modal_state) => {
                match modal_state.active_field {
                    0 => modal_state.full_caps.push_str(text),
                    1 => modal_state.partial_caps.push_str(text),
                    _ => modal_state.limited_caps.push_str(text),
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
            | QueuedModal::SettingsDeviceSelect(_)
            | QueuedModal::SettingsRemoveDevice(_)
            | QueuedModal::AuthorityPicker(_)
            | QueuedModal::ChatMemberSelect(_)
            | QueuedModal::NeighborhoodModeratorAssignment(_)
            | QueuedModal::NeighborhoodAccessOverride(_) => {}
        }
    }

    // Screen-level paste requires insert mode.
    if !state.is_insert_mode() {
        return;
    }

    // Handle screen-level input buffers
    match state.screen() {
        Screen::Chat => {
            if state.chat.focus == ChatFocus::Input {
                state.chat.input_buffer.push_str(text);
            }
        }
        Screen::Neighborhood => {
            let _ = text;
        }
        _ => {}
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::tui::screens::Screen;
    use crate::tui::state::views::{ChatFocus, ImportInvitationModalState};

    #[test]
    fn paste_updates_contacts_import_modal_without_insert_mode() {
        let mut state = TuiState::new();
        state.modal_queue.enqueue(QueuedModal::ContactsImport(
            ImportInvitationModalState::default(),
        ));
        assert!(!state.is_insert_mode());

        handle_paste_event(&mut state, &mut Vec::new(), "invite-code");

        match state.modal_queue.current() {
            Some(QueuedModal::ContactsImport(modal_state)) => {
                assert_eq!(modal_state.code, "invite-code");
            }
            _ => panic!("expected contacts import modal to remain active"),
        }
    }

    #[test]
    fn paste_updates_chat_input_only_in_insert_mode() {
        let mut state = TuiState::new();
        state.router.go_to(Screen::Chat);
        state.chat.focus = ChatFocus::Input;

        handle_paste_event(&mut state, &mut Vec::new(), "ignored");
        assert_eq!(state.chat.input_buffer, "");

        state.chat.insert_mode = true;
        handle_paste_event(&mut state, &mut Vec::new(), "hello");
        assert_eq!(state.chat.input_buffer, "hello");
    }

    #[test]
    fn enter_routes_neighborhood_slash_command_through_chat_send() {
        let mut state = TuiState::new();
        state.router.go_to(Screen::Chat);
        state.chat.focus = ChatFocus::Input;
        state.chat.insert_mode = true;
        state.chat.input_buffer = "/nhadd home-123".to_string();

        let mut commands = Vec::new();
        handle_insert_mode_key(&mut state, &mut commands, KeyEvent::press(KeyCode::Enter));

        assert!(matches!(
            commands.first(),
            Some(TuiCommand::Dispatch(DispatchCommand::SendChatMessage { content }))
                if content == "/nhadd home-123"
        ));
    }

    #[test]
    fn enter_reports_unsupported_slash_command() {
        let mut state = TuiState::new();
        state.router.go_to(Screen::Chat);
        state.chat.focus = ChatFocus::Input;
        state.chat.insert_mode = true;
        state.chat.input_buffer = "/kick alice".to_string();

        let mut commands = Vec::new();
        handle_insert_mode_key(&mut state, &mut commands, KeyEvent::press(KeyCode::Enter));

        assert!(matches!(
            commands.first(),
            Some(TuiCommand::Dispatch(DispatchCommand::SendChatMessage { content }))
                if content == "/kick alice"
        ));
    }

    #[test]
    fn enter_help_command_emits_local_help_toast() {
        let mut state = TuiState::new();
        state.router.go_to(Screen::Chat);
        state.chat.focus = ChatFocus::Input;
        state.chat.insert_mode = true;
        state.chat.input_buffer = "/help".to_string();

        let mut commands = Vec::new();
        handle_insert_mode_key(&mut state, &mut commands, KeyEvent::press(KeyCode::Enter));

        assert!(commands.is_empty(), "help should be handled locally");
        let toast = state
            .toast_queue
            .current()
            .unwrap_or_else(|| panic!("help should enqueue a toast"));
        assert!(toast.message.contains("Use ? for TUI help"));
    }

    #[test]
    fn enter_help_with_command_emits_command_help_toast() {
        let mut state = TuiState::new();
        state.router.go_to(Screen::Chat);
        state.chat.focus = ChatFocus::Input;
        state.chat.insert_mode = true;
        state.chat.input_buffer = "/help kick".to_string();

        let mut commands = Vec::new();
        handle_insert_mode_key(&mut state, &mut commands, KeyEvent::press(KeyCode::Enter));

        assert!(commands.is_empty(), "help should be handled locally");
        let toast = state
            .toast_queue
            .current()
            .unwrap_or_else(|| panic!("help should enqueue a toast"));
        assert!(toast.message.contains("/kick <user> [reason]"));
    }

    #[test]
    fn enter_whois_command_dispatches_through_chat_pipeline() {
        let mut state = TuiState::new();
        state.router.go_to(Screen::Chat);
        state.chat.focus = ChatFocus::Input;
        state.chat.insert_mode = true;
        state.chat.input_buffer = "/whois authority-abc".to_string();

        let mut commands = Vec::new();
        handle_insert_mode_key(&mut state, &mut commands, KeyEvent::press(KeyCode::Enter));

        assert_eq!(
            commands.len(),
            1,
            "whois should dispatch through chat pipeline"
        );
        assert!(
            matches!(
                &commands[0],
                TuiCommand::Dispatch(DispatchCommand::SendChatMessage { content })
                    if content == "/whois authority-abc"
            ),
            "whois should be forwarded unchanged as a chat command"
        );
        assert!(
            state.toast_queue.current().is_none(),
            "whois should not enqueue a local placeholder toast"
        );
    }
}

/// Handle insert mode key events
pub fn handle_insert_mode_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    // Capture screen type once to avoid borrow conflicts
    let screen = state.screen();

    // Escape exits insert mode and scrolls to bottom
    if key.code == KeyCode::Esc {
        match screen {
            Screen::Chat => {
                state.chat.insert_mode = false;
                state.chat.insert_mode_entry_char = None;
                state.chat.focus = ChatFocus::Channels;
                // Auto-scroll to bottom (show latest messages)
                state.chat.message_scroll = 0;
            }
            Screen::Neighborhood => {
                state.neighborhood.insert_mode = false;
                state.neighborhood.insert_mode_entry_char = None;
                state.neighborhood.detail_focus = DetailFocus::Channels;
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
                        let _ = c;
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
                // Neighborhood insert mode is disabled; ignore character edits.
            }
            _ => {}
        },
        KeyCode::Enter => match screen {
            Screen::Chat => {
                if !state.chat.input_buffer.is_empty() {
                    let content = state.chat.input_buffer.clone();
                    state.chat.input_buffer.clear();
                    if content.starts_with('/') {
                        match crate::tui::commands::parse_chat_command(&content) {
                            // Handle /help locally so deterministic UI guidance is shown even
                            // when command callbacks are unavailable or delayed.
                            Ok(crate::tui::commands::IrcCommand::Help { command }) => {
                                let message = if let Some(raw_name) = command
                                    .as_deref()
                                    .map(str::trim)
                                    .filter(|value| !value.is_empty())
                                {
                                    let normalized =
                                        raw_name.trim_start_matches('/').to_lowercase();
                                    if let Some(help) =
                                        crate::tui::commands::command_help(&normalized)
                                    {
                                        format!("{} — {}", help.syntax, help.description)
                                    } else {
                                        format!("Unknown command: /{normalized}")
                                    }
                                } else {
                                    "Use ? for TUI help. Run /help <command> for details. Core commands: /msg /me /nick /who /whois /join /leave /topic /invite /homeinvite /homeaccept /kick /ban /unban /mute /unmute /pin /unpin /op /deop /mode /neighborhood /nhadd /nhlink".to_string()
                                };
                                state.toast_info(message);
                            }
                            _ => {
                                // Route other slash commands through the chat callback
                                // strong-command pipeline (`ParsedCommand -> ResolvedCommand
                                // -> CommandPlan`).
                                commands.push(TuiCommand::Dispatch(
                                    DispatchCommand::SendChatMessage { content },
                                ));
                            }
                        }
                    } else {
                        commands.push(TuiCommand::Dispatch(DispatchCommand::SendChatMessage {
                            content,
                        }));
                    }
                    state.chat.insert_mode = false;
                    state.chat.insert_mode_entry_char = None;
                    state.chat.focus = ChatFocus::Channels;
                    // Auto-scroll to bottom (show latest messages)
                    state.chat.message_scroll = 0;
                }
            }
            Screen::Neighborhood => {
                // Neighborhood insert mode is disabled; Enter does not dispatch messaging.
            }
            _ => {}
        },
        _ => {}
    }
}
