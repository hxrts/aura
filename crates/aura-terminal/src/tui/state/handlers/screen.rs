//! Screen-specific keyboard handlers
//!
//! Event types (KeyEvent) are passed by value following standard
//! event handler conventions.

#![allow(clippy::needless_pass_by_value)]

use aura_core::effects::terminal::{KeyCode, KeyEvent};

use crate::tui::navigation::{navigate_list, NavKey};
use crate::tui::types::SettingsSection;

use super::super::commands::{DispatchCommand, TuiCommand};
use super::super::modal_queue::QueuedModal;
use super::super::toast::{QueuedToast, ToastLevel};
use super::super::views::{
    AddDeviceModalState, ChatFocus, DetailFocus, ImportInvitationModalState,
    NeighborhoodMode, NicknameSuggestionModalState,
};
use super::super::TuiState;

/// Handle chat screen key events
pub fn handle_chat_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    match key.code {
        KeyCode::Char('i') => {
            state.chat.insert_mode = true;
            state.chat.insert_mode_entry_char = Some('i');
            state.chat.focus = ChatFocus::Input;
        }
        // Left/Right navigation between panels (with wrap-around)
        KeyCode::Left | KeyCode::Char('h') => {
            state.chat.focus = match state.chat.focus {
                ChatFocus::Channels => ChatFocus::Messages, // Wrap to last
                ChatFocus::Messages => ChatFocus::Channels,
                ChatFocus::Input => ChatFocus::Input, // Don't change in input mode
            };
        }
        KeyCode::Right | KeyCode::Char('l') => {
            state.chat.focus = match state.chat.focus {
                ChatFocus::Channels => ChatFocus::Messages,
                ChatFocus::Messages => ChatFocus::Channels, // Wrap to first
                ChatFocus::Input => ChatFocus::Input,       // Don't change in input mode
            };
        }
        KeyCode::Up | KeyCode::Char('k') => match state.chat.focus {
            ChatFocus::Channels => {
                state.chat.selected_channel = navigate_list(
                    state.chat.selected_channel,
                    state.chat.channel_count,
                    NavKey::Up,
                );
            }
            ChatFocus::Messages => {
                // Scroll toward newer messages (reduce offset toward bottom).
                if state.chat.message_scroll > 0 {
                    state.chat.message_scroll = state.chat.message_scroll.saturating_sub(1);
                }
            }
            _ => {}
        },
        KeyCode::Down | KeyCode::Char('j') => match state.chat.focus {
            ChatFocus::Channels => {
                state.chat.selected_channel = navigate_list(
                    state.chat.selected_channel,
                    state.chat.channel_count,
                    NavKey::Down,
                );
            }
            ChatFocus::Messages => {
                // Scroll toward older messages (increase offset away from bottom).
                let max_scroll = state.chat.message_count.saturating_sub(18);
                if state.chat.message_scroll < max_scroll {
                    state.chat.message_scroll =
                        state.chat.message_scroll.saturating_add(1).min(max_scroll);
                }
            }
            _ => {}
        },
        KeyCode::Char('n') => {
            commands.push(TuiCommand::Dispatch(DispatchCommand::OpenChatCreateWizard));
        }
        KeyCode::Char('t') => {
            // Open topic edit modal via dispatch (shell populates selected channel details)
            commands.push(TuiCommand::Dispatch(DispatchCommand::OpenChatTopicModal));
        }
        KeyCode::Char('o') => {
            // Open channel info modal via dispatch (shell populates selected channel details)
            commands.push(TuiCommand::Dispatch(DispatchCommand::OpenChatInfoModal));
        }
        KeyCode::Char('r') => {
            // Retry message (when focused on messages)
            if state.chat.focus == ChatFocus::Messages {
                commands.push(TuiCommand::Dispatch(DispatchCommand::RetryMessage));
            }
        }
        _ => {}
    }
}

/// Handle contacts screen key events
pub fn handle_contacts_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    match key.code {
        // Panel navigation (h/l or arrows)
        KeyCode::Left | KeyCode::Char('h') => {
            state.contacts.focus = state.contacts.focus.toggle();
        }
        KeyCode::Right | KeyCode::Char('l') => {
            state.contacts.focus = state.contacts.focus.toggle();
        }
        // List navigation (j/k or arrows) - only when list is focused
        KeyCode::Up | KeyCode::Char('k') => {
            if state.contacts.focus.is_list() {
                state.contacts.selected_index = navigate_list(
                    state.contacts.selected_index,
                    state.contacts.contact_count,
                    NavKey::Up,
                );
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.contacts.focus.is_list() {
                state.contacts.selected_index = navigate_list(
                    state.contacts.selected_index,
                    state.contacts.contact_count,
                    NavKey::Down,
                );
            }
        }
        KeyCode::Char('e') => {
            // Open nickname edit modal via dispatch (shell populates selected contact details)
            commands.push(TuiCommand::Dispatch(
                DispatchCommand::OpenContactNicknameModal,
            ));
        }
        KeyCode::Char('c') => {
            // Start chat with selected contact
            commands.push(TuiCommand::Dispatch(DispatchCommand::StartChat));
        }
        KeyCode::Char('a') => {
            // Open accept invitation modal via queue
            state.modal_queue.enqueue(QueuedModal::ContactsImport(
                ImportInvitationModalState::default(),
            ));

            // In demo mode, show a toast with shortcut hints
            if !state.contacts.demo_alice_code.is_empty() {
                state.next_toast_id += 1;
                state.toast_queue.enqueue(QueuedToast::new(
                    state.next_toast_id,
                    "[DEMO] Auto-fill: Ctrl+a for Alice, Ctrl+l for Carol",
                    ToastLevel::Info,
                ));
            }
        }
        KeyCode::Char('n') => {
            // Open create invitation modal via dispatch (shell will populate receiver details)
            commands.push(TuiCommand::Dispatch(
                DispatchCommand::OpenCreateInvitationModal,
            ));
        }
        KeyCode::Enter => {
            commands.push(TuiCommand::Dispatch(DispatchCommand::StartChat));
        }
        _ => {}
    }
}

/// Handle neighborhood screen key events
pub fn handle_neighborhood_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
) {
    match state.neighborhood.mode {
        NeighborhoodMode::Map => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                state.neighborhood.grid.navigate(NavKey::Up);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                state.neighborhood.grid.navigate(NavKey::Down);
            }
            KeyCode::Left | KeyCode::Char('h') => {
                state.neighborhood.grid.navigate(NavKey::Left);
            }
            KeyCode::Right | KeyCode::Char('l') => {
                state.neighborhood.grid.navigate(NavKey::Right);
            }
            KeyCode::Char('d') => {
                state.neighborhood.enter_depth = state.neighborhood.enter_depth.next();
                state.toast_info(format!(
                    "Enter as: {}",
                    state.neighborhood.enter_depth.label()
                ));
            }
            KeyCode::Enter => {
                if state.neighborhood.home_count > 0 {
                    state.neighborhood.mode = NeighborhoodMode::Detail;
                    state.neighborhood.entered_home_id =
                        Some(state.neighborhood.selected_home.to_string());
                    commands.push(TuiCommand::Dispatch(DispatchCommand::EnterHome));
                }
            }
            KeyCode::Char('a') => {
                // Open accept invitation modal
                state.modal_queue.enqueue(QueuedModal::ContactsImport(
                    ImportInvitationModalState::default(),
                ));
            }
            KeyCode::Char('i') => {
                // Enter home in insert mode (like chat screen)
                state.neighborhood.mode = NeighborhoodMode::Detail;
                state.neighborhood.entered_home_id =
                    Some(state.neighborhood.selected_home.to_string());
                state.neighborhood.insert_mode = true;
                state.neighborhood.insert_mode_entry_char = Some('i');
                state.neighborhood.detail_focus = DetailFocus::Input;
                commands.push(TuiCommand::Dispatch(DispatchCommand::EnterHome));
            }
            KeyCode::Char('n') => {
                // Create a new home
                commands.push(TuiCommand::Dispatch(DispatchCommand::OpenHomeCreate));
            }
            KeyCode::Char('g') | KeyCode::Char('H') => {
                commands.push(TuiCommand::Dispatch(DispatchCommand::GoHome));
            }
            KeyCode::Char('b') | KeyCode::Esc | KeyCode::Backspace => {
                commands.push(TuiCommand::Dispatch(DispatchCommand::BackToStreet));
            }
            _ => {}
        },
        NeighborhoodMode::Detail => match key.code {
            KeyCode::Esc => {
                state.neighborhood.mode = NeighborhoodMode::Map;
                state.neighborhood.insert_mode = false;
                state.neighborhood.insert_mode_entry_char = None;
                state.neighborhood.entered_home_id = None;
                state.neighborhood.detail_focus = DetailFocus::Channels;
            }
            KeyCode::Left | KeyCode::Char('h') => {
                state.neighborhood.detail_focus = match state.neighborhood.detail_focus {
                    DetailFocus::Input | DetailFocus::Messages => DetailFocus::Residents,
                    DetailFocus::Residents => DetailFocus::Channels,
                    DetailFocus::Channels => DetailFocus::Channels,
                };
            }
            KeyCode::Right | KeyCode::Char('l') => {
                state.neighborhood.detail_focus = match state.neighborhood.detail_focus {
                    DetailFocus::Channels => DetailFocus::Residents,
                    DetailFocus::Residents => DetailFocus::Messages,
                    DetailFocus::Messages | DetailFocus::Input => DetailFocus::Messages,
                };
            }
            KeyCode::Up | KeyCode::Char('k') => match state.neighborhood.detail_focus {
                DetailFocus::Channels => {
                    state.neighborhood.selected_channel = navigate_list(
                        state.neighborhood.selected_channel,
                        state.neighborhood.channel_count,
                        NavKey::Up,
                    );
                }
                DetailFocus::Residents => {
                    state.neighborhood.selected_resident = navigate_list(
                        state.neighborhood.selected_resident,
                        state.neighborhood.resident_count,
                        NavKey::Up,
                    );
                }
                DetailFocus::Messages => {
                    // Scroll up = increase offset (show older messages)
                    // scroll_offset: 0 = at bottom (latest), higher = scrolled up (older)
                    let max_scroll = state.neighborhood.message_count.saturating_sub(18);
                    if state.neighborhood.message_scroll < max_scroll {
                        state.neighborhood.message_scroll = state
                            .neighborhood
                            .message_scroll
                            .saturating_add(1)
                            .min(max_scroll);
                    }
                }
                DetailFocus::Input => {}
            },
            KeyCode::Down | KeyCode::Char('j') => match state.neighborhood.detail_focus {
                DetailFocus::Channels => {
                    state.neighborhood.selected_channel = navigate_list(
                        state.neighborhood.selected_channel,
                        state.neighborhood.channel_count,
                        NavKey::Down,
                    );
                }
                DetailFocus::Residents => {
                    state.neighborhood.selected_resident = navigate_list(
                        state.neighborhood.selected_resident,
                        state.neighborhood.resident_count,
                        NavKey::Down,
                    );
                }
                DetailFocus::Messages => {
                    // Scroll down = decrease offset (show newer messages, toward bottom)
                    // scroll_offset: 0 = at bottom (latest), higher = scrolled up (older)
                    if state.neighborhood.message_scroll > 0 {
                        state.neighborhood.message_scroll =
                            state.neighborhood.message_scroll.saturating_sub(1);
                    }
                }
                DetailFocus::Input => {}
            },
            KeyCode::Char('i') => {
                state.neighborhood.insert_mode = true;
                state.neighborhood.insert_mode_entry_char = Some('i');
                state.neighborhood.detail_focus = DetailFocus::Input;
            }
            _ => {}
        },
    }

    state.neighborhood.selected_home = state.neighborhood.grid.current();
}

/// Handle settings screen key events
pub fn handle_settings_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    let previous_section = state.settings.section;

    // Handle Authority panel sub-section navigation specially
    let in_authority_detail =
        state.settings.section == SettingsSection::Authority && state.settings.focus.is_detail();

    match key.code {
        KeyCode::Left | KeyCode::Char('h') => {
            if in_authority_detail {
                // Navigate between sub-sections within Authority panel
                state.settings.authority_sub_section = state.settings.authority_sub_section.prev();
            } else {
                state.settings.focus = state.settings.focus.toggle();
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if in_authority_detail {
                // Navigate between sub-sections within Authority panel
                state.settings.authority_sub_section = state.settings.authority_sub_section.next();
            } else {
                state.settings.focus = state.settings.focus.toggle();
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            // Always allow section navigation with Up/Down
            // If in Detail focus, reset to List and navigate
            if state.settings.focus.is_detail() {
                state.settings.focus = state.settings.focus.toggle();
            }
            state.settings.section = state.settings.section.prev();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            // Always allow section navigation with Up/Down
            // If in Detail focus, reset to List and navigate
            if state.settings.focus.is_detail() {
                state.settings.focus = state.settings.focus.toggle();
            }
            state.settings.section = state.settings.section.next();
        }
        KeyCode::Char(' ') => {
            if state.settings.section == SettingsSection::Authority {
                commands.push(TuiCommand::Dispatch(DispatchCommand::OpenMfaSetup));
            }
        }
        KeyCode::Char('m') => {
            if state.settings.section == SettingsSection::Authority {
                commands.push(TuiCommand::Dispatch(DispatchCommand::OpenMfaSetup));
            }
        }
        KeyCode::Char('e') => {
            if state.settings.section == SettingsSection::Profile {
                // Open nickname suggestion edit modal via queue
                state.modal_queue.enqueue(QueuedModal::SettingsNicknameSuggestion(
                    NicknameSuggestionModalState::default(),
                ));
            }
        }
        KeyCode::Enter => {
            match state.settings.section {
                SettingsSection::Profile => {
                    // Open nickname suggestion edit modal via queue
                    state.modal_queue.enqueue(QueuedModal::SettingsNicknameSuggestion(
                        NicknameSuggestionModalState::default(),
                    ));
                }
                SettingsSection::Threshold => {
                    // Open guardian setup modal via dispatch (reuse the same wizard as contacts)
                    // Shell populates contacts and current guardians
                    commands.push(TuiCommand::Dispatch(DispatchCommand::OpenGuardianSetup));
                }
                SettingsSection::Recovery => {
                    commands.push(TuiCommand::Dispatch(DispatchCommand::StartRecovery));
                }
                SettingsSection::Authority => {
                    // Action depends on sub-section
                    use crate::tui::types::AuthoritySubSection;
                    match state.settings.authority_sub_section {
                        AuthoritySubSection::Info => {
                            // Open authority picker if multiple authorities (app-global)
                            if state.authorities.len() > 1 {
                                commands.push(TuiCommand::Dispatch(
                                    DispatchCommand::OpenAuthorityPicker,
                                ));
                            }
                        }
                        AuthoritySubSection::Mfa => {
                            commands.push(TuiCommand::Dispatch(DispatchCommand::OpenMfaSetup));
                        }
                    }
                }
                _ => {}
            }
        }
        KeyCode::Char('t') => {
            if state.settings.section == SettingsSection::Threshold {
                // Open guardian setup modal via dispatch (reuse the same wizard as contacts)
                commands.push(TuiCommand::Dispatch(DispatchCommand::OpenGuardianSetup));
            }
        }
        KeyCode::Char('a') => {
            if state.settings.section == SettingsSection::Devices {
                // Open add device modal via queue
                state.modal_queue.enqueue(QueuedModal::SettingsAddDevice(
                    AddDeviceModalState::default(),
                ));
            }
        }
        KeyCode::Char('i') => {
            if state.settings.section == SettingsSection::Devices {
                state.modal_queue.enqueue(QueuedModal::SettingsDeviceImport(
                    ImportInvitationModalState::default(),
                ));
                if !state.settings.demo_mobile_device_id.is_empty() {
                    state.next_toast_id += 1;
                    state.toast_queue.enqueue(QueuedToast::new(
                        state.next_toast_id,
                        "[DEMO] Press Ctrl+m to auto-fill the Mobile device code",
                        ToastLevel::Info,
                    ));
                }
            }
        }
        KeyCode::Char('s') => {
            if state.settings.section == SettingsSection::Recovery {
                commands.push(TuiCommand::Dispatch(DispatchCommand::StartRecovery));
            } else if state.settings.section == SettingsSection::Authority {
                // Switch authority - open picker if multiple authorities available (app-global)
                if state.authorities.len() > 1 {
                    commands.push(TuiCommand::Dispatch(DispatchCommand::OpenAuthorityPicker));
                }
            }
        }
        _ => {}
    }

    // Demo shortcuts for device enrollment import are handled in the modal.

    if previous_section != state.settings.section
        && state.settings.section == SettingsSection::Devices
        && !state.contacts.demo_alice_code.is_empty()
    {
        // Demo hint now appears when the enrollment code modal opens.
    }
}

/// Handle notifications screen key events
pub fn handle_notifications_key(
    state: &mut TuiState,
    _commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Left | KeyCode::Char('h') => {
            state.notifications.focus = state.notifications.focus.toggle();
        }
        KeyCode::Right | KeyCode::Char('l') => {
            state.notifications.focus = state.notifications.focus.toggle();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if state.notifications.focus.is_list() {
                state.notifications.selected_index = navigate_list(
                    state.notifications.selected_index,
                    state.notifications.item_count,
                    NavKey::Up,
                );
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.notifications.focus.is_list() {
                state.notifications.selected_index = navigate_list(
                    state.notifications.selected_index,
                    state.notifications.item_count,
                    NavKey::Down,
                );
            }
        }
        _ => {}
    }
}
