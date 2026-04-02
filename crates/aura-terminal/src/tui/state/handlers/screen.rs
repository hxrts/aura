//! Screen-specific keyboard handlers
//!
//! Event types (KeyEvent) are passed by value following standard
//! event handler conventions.

#![allow(clippy::needless_pass_by_value)]

use aura_core::effects::terminal::{KeyCode, KeyEvent};

use crate::tui::layout::dim;
use crate::tui::navigation::{navigate_list, NavKey, TwoPanelFocus};
use crate::tui::state::ContactsListFocus;
use crate::tui::types::{AccessLevel, SettingsSection};

use super::super::commands::{DispatchCommand, TuiCommand};
use super::super::modal_queue::QueuedModal;
use super::super::toast::{QueuedToast, ToastLevel};
use super::super::views::{
    AddDeviceModalState, ChatFocus, DetailFocus, ImportInvitationModalState, NeighborhoodMode,
    NicknameSuggestionModalState,
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
                // Scroll up = increase offset (show older messages)
                // scroll_offset: 0 = at bottom (latest), higher = scrolled up (older)
                let max_scroll = state
                    .chat
                    .message_count
                    .saturating_sub(dim::VISIBLE_MESSAGE_ROWS as usize);
                if state.chat.message_scroll < max_scroll {
                    state.chat.message_scroll =
                        state.chat.message_scroll.saturating_add(1).min(max_scroll);
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
                // Scroll down = decrease offset (show newer messages, toward bottom)
                // scroll_offset: 0 = at bottom (latest), higher = scrolled up (older)
                if state.chat.message_scroll > 0 {
                    state.chat.message_scroll = state.chat.message_scroll.saturating_sub(1);
                }
            }
            _ => {}
        },
        KeyCode::Char('n') => {
            commands.push(TuiCommand::Dispatch(DispatchCommand::OpenChatCreateWizard));
        }
        KeyCode::Char('e') => {
            // Open channel edit modal via dispatch (shell populates selected channel details)
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
                match state.contacts.list_focus {
                    ContactsListFocus::LanPeers => {
                        if state.contacts.lan_peer_count > 0 {
                            state.contacts.lan_selected_index = navigate_list(
                                state.contacts.lan_selected_index,
                                state.contacts.lan_peer_count,
                                NavKey::Up,
                            );
                        } else {
                            state.contacts.list_focus = ContactsListFocus::Contacts;
                        }
                    }
                    ContactsListFocus::Contacts => {
                        state.contacts.selected_index = navigate_list(
                            state.contacts.selected_index,
                            state.contacts.contact_count,
                            NavKey::Up,
                        );
                    }
                }
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.contacts.focus.is_list() {
                match state.contacts.list_focus {
                    ContactsListFocus::LanPeers => {
                        if state.contacts.lan_peer_count > 0 {
                            state.contacts.lan_selected_index = navigate_list(
                                state.contacts.lan_selected_index,
                                state.contacts.lan_peer_count,
                                NavKey::Down,
                            );
                        } else {
                            state.contacts.list_focus = ContactsListFocus::Contacts;
                        }
                    }
                    ContactsListFocus::Contacts => {
                        state.contacts.selected_index = navigate_list(
                            state.contacts.selected_index,
                            state.contacts.contact_count,
                            NavKey::Down,
                        );
                    }
                }
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
                    "[DEMO] Auto-fill: a/Ctrl+a for Alice, l/Ctrl+l for Carol",
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
        KeyCode::Char('f') => {
            commands.push(TuiCommand::Dispatch(
                DispatchCommand::SendSelectedFriendRequest,
            ));
        }
        KeyCode::Char('y') => {
            commands.push(TuiCommand::Dispatch(
                DispatchCommand::AcceptSelectedFriendRequest,
            ));
        }
        KeyCode::Char('x') => {
            commands.push(TuiCommand::Dispatch(
                DispatchCommand::DeclineSelectedFriendRequest,
            ));
        }
        KeyCode::Char('i') => {
            commands.push(TuiCommand::Dispatch(
                DispatchCommand::InviteSelectedContactToChannel,
            ));
        }
        KeyCode::Char('p') => {
            if state.contacts.focus.is_list() {
                if state.contacts.lan_peer_count > 0 {
                    state.contacts.list_focus = state.contacts.list_focus.toggle();
                } else {
                    state.toast_info("No bootstrap candidates discovered yet.");
                }
            }
        }
        KeyCode::Char('r') => {
            // Open remove contact confirmation modal via dispatch (shell populates selected contact)
            commands.push(TuiCommand::Dispatch(
                DispatchCommand::OpenRemoveContactModal,
            ));
        }
        KeyCode::Esc => {
            if state.contacts.focus.is_detail() {
                state.contacts.focus = TwoPanelFocus::List;
            }
        }
        KeyCode::Enter => {
            if state.contacts.focus.is_list() {
                if state.contacts.list_focus.is_lan() && state.contacts.lan_peer_count > 0 {
                    commands.push(TuiCommand::Dispatch(DispatchCommand::InviteLanPeer));
                } else {
                    // Show detail panel for selected contact
                    state.contacts.focus = TwoPanelFocus::Detail;
                }
            }
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
            KeyCode::Enter => {
                if state.neighborhood.home_count > 0 {
                    state.neighborhood.mode = NeighborhoodMode::Detail;
                    state.neighborhood.enter_depth = AccessLevel::Full;
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
            KeyCode::Char('n') => {
                // Create a new home
                commands.push(TuiCommand::Dispatch(DispatchCommand::OpenHomeCreate));
            }
            KeyCode::Char('m') => {
                // Create/select active neighborhood with a default label.
                commands.push(TuiCommand::Dispatch(DispatchCommand::CreateNeighborhood {
                    name: "Neighborhood".to_string(),
                }));
            }
            KeyCode::Char('v') => {
                // Add selected home as a neighborhood member.
                commands.push(TuiCommand::Dispatch(
                    DispatchCommand::AddSelectedHomeToNeighborhood,
                ));
            }
            KeyCode::Char('L') => {
                // Force direct one_hop_link for selected home.
                commands.push(TuiCommand::Dispatch(
                    DispatchCommand::LinkSelectedHomeOneHopLink,
                ));
            }
            KeyCode::Char('g') | KeyCode::Char('H') => {
                commands.push(TuiCommand::Dispatch(DispatchCommand::GoHome));
            }
            KeyCode::Char('b') | KeyCode::Esc | KeyCode::Backspace => {
                state.neighborhood.enter_depth = AccessLevel::Limited;
                commands.push(TuiCommand::Dispatch(DispatchCommand::BackToLimited));
            }
            _ => {}
        },
        NeighborhoodMode::Detail => match key.code {
            KeyCode::Esc => {
                state.neighborhood.mode = NeighborhoodMode::Map;
                state.neighborhood.enter_depth = AccessLevel::Limited;
                state.neighborhood.entered_home_id = None;
                state.neighborhood.detail_focus = DetailFocus::Channels;
            }
            KeyCode::Left | KeyCode::Char('h') => {
                state.neighborhood.detail_focus = match state.neighborhood.detail_focus {
                    DetailFocus::Messages | DetailFocus::Input => DetailFocus::Members,
                    DetailFocus::Members => DetailFocus::Channels,
                    DetailFocus::Channels => DetailFocus::Channels,
                };
            }
            KeyCode::Right | KeyCode::Char('l') => {
                state.neighborhood.detail_focus = match state.neighborhood.detail_focus {
                    DetailFocus::Channels => DetailFocus::Members,
                    DetailFocus::Members => DetailFocus::Members,
                    DetailFocus::Messages | DetailFocus::Input => DetailFocus::Members,
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
                DetailFocus::Members => {
                    state.neighborhood.selected_member = navigate_list(
                        state.neighborhood.selected_member,
                        state.neighborhood.member_count,
                        NavKey::Up,
                    );
                }
                DetailFocus::Messages | DetailFocus::Input => {}
            },
            KeyCode::Down | KeyCode::Char('j') => match state.neighborhood.detail_focus {
                DetailFocus::Channels => {
                    state.neighborhood.selected_channel = navigate_list(
                        state.neighborhood.selected_channel,
                        state.neighborhood.channel_count,
                        NavKey::Down,
                    );
                }
                DetailFocus::Members => {
                    state.neighborhood.selected_member = navigate_list(
                        state.neighborhood.selected_member,
                        state.neighborhood.member_count,
                        NavKey::Down,
                    );
                }
                DetailFocus::Messages | DetailFocus::Input => {}
            },
            KeyCode::Char('o') => {
                if state.neighborhood.moderator_actions_enabled {
                    commands.push(TuiCommand::Dispatch(
                        DispatchCommand::OpenModeratorAssignmentModal,
                    ));
                } else {
                    state.toast_warning("Moderator permissions required");
                }
            }
            KeyCode::Char('x') => {
                if state.neighborhood.moderator_actions_enabled {
                    commands.push(TuiCommand::Dispatch(
                        DispatchCommand::OpenAccessOverrideModal,
                    ));
                } else {
                    state.toast_warning("Moderator permissions required");
                }
            }
            KeyCode::Char('p') => {
                if state.neighborhood.moderator_actions_enabled {
                    commands.push(TuiCommand::Dispatch(
                        DispatchCommand::OpenHomeCapabilityConfigModal,
                    ));
                } else {
                    state.toast_warning("Moderator permissions required");
                }
            }
            _ => {}
        },
    }

    state.neighborhood.selected_home = state.neighborhood.grid.current();
}

/// Handle settings screen key events
pub fn handle_settings_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    let previous_section = state.settings.section;

    match key.code {
        KeyCode::Left | KeyCode::Char('h') => {
            state.settings.focus = state.settings.focus.toggle();
        }
        KeyCode::Right | KeyCode::Char('l') => {
            state.settings.focus = state.settings.focus.toggle();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            // Always allow section navigation with Up/Down
            // If in Detail focus, reset to List and navigate
            if state.settings.focus.is_detail() {
                state.settings.focus = state.settings.focus.toggle();
            }
            state.settings.section = state.settings.section.prev();
            state.settings.selected_index = state.settings.section.index();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            // Always allow section navigation with Up/Down
            // If in Detail focus, reset to List and navigate
            if state.settings.focus.is_detail() {
                state.settings.focus = state.settings.focus.toggle();
            }
            state.settings.section = state.settings.section.next();
            state.settings.selected_index = state.settings.section.index();
        }
        KeyCode::Char('m') => {
            if state.settings.section == SettingsSection::Authority {
                commands.push(TuiCommand::Dispatch(DispatchCommand::OpenMfaSetup));
            }
        }
        KeyCode::Char('e') => {
            if state.settings.section == SettingsSection::Profile {
                // Open nickname suggestion edit modal via queue
                state
                    .modal_queue
                    .enqueue(QueuedModal::SettingsNicknameSuggestion(
                        NicknameSuggestionModalState::default(),
                    ));
            }
        }
        KeyCode::Enter => {
            match state.settings.section {
                SettingsSection::Profile => {
                    // Open nickname suggestion edit modal via queue
                    state
                        .modal_queue
                        .enqueue(QueuedModal::SettingsNicknameSuggestion(
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
                    commands.push(TuiCommand::Dispatch(DispatchCommand::OpenMfaSetup));
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
                let mut modal_state = AddDeviceModalState::default();
                // In demo mode, pre-fill Mobile's authority ID for device enrollment
                if !state.settings.demo_mobile_authority_id.is_empty() {
                    modal_state.invitee_authority_id =
                        state.settings.demo_mobile_authority_id.clone();
                }
                state
                    .modal_queue
                    .enqueue(QueuedModal::SettingsAddDevice(modal_state));
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
        KeyCode::Char('r') => {
            if state.settings.section == SettingsSection::Devices {
                // Open device selection modal via dispatch (shell populates devices)
                commands.push(TuiCommand::Dispatch(DispatchCommand::OpenDeviceSelectModal));
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
