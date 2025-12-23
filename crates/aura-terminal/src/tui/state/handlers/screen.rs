//! Screen-specific keyboard handlers

use aura_core::effects::terminal::{KeyCode, KeyEvent};

use crate::tui::navigation::{navigate_list, NavKey};
use crate::tui::screens::Screen;
use crate::tui::types::{RecoveryTab, SettingsSection};

use super::super::commands::{DispatchCommand, TuiCommand};
use super::super::modal_queue::{ContactSelectModalState, QueuedModal};
use super::super::toast::ToastLevel;
use super::super::views::{
    AddDeviceModalState, BlockFocus, ChatFocus, CreateChannelModalState, DisplayNameModalState,
    ImportInvitationModalState,
};
use super::super::TuiState;

/// Handle block screen key events
pub fn handle_block_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    // Block invite modal is now handled via queue system
    match key.code {
        KeyCode::Char('i') => {
            state.block.insert_mode = true;
            state.block.insert_mode_entry_char = Some('i');
            state.block.focus = BlockFocus::Input;
        }
        // Left/Right navigation between panels (with wrap-around)
        KeyCode::Left | KeyCode::Char('h') => {
            state.block.focus = match state.block.focus {
                BlockFocus::Residents => BlockFocus::Messages, // Wrap to last
                BlockFocus::Messages => BlockFocus::Residents,
                BlockFocus::Input => BlockFocus::Input, // Don't change in input mode
            };
        }
        KeyCode::Right | KeyCode::Char('l') => {
            state.block.focus = match state.block.focus {
                BlockFocus::Residents => BlockFocus::Messages,
                BlockFocus::Messages => BlockFocus::Residents, // Wrap to first
                BlockFocus::Input => BlockFocus::Input,        // Don't change in input mode
            };
        }
        // Up/Down navigation within panels
        KeyCode::Up | KeyCode::Char('k') => match state.block.focus {
            BlockFocus::Residents => {
                state.block.selected_resident = navigate_list(
                    state.block.selected_resident,
                    state.block.resident_count,
                    NavKey::Up,
                );
            }
            BlockFocus::Messages => {
                state.block.message_scroll = navigate_list(
                    state.block.message_scroll,
                    state.block.message_count,
                    NavKey::Up,
                );
            }
            BlockFocus::Input => {}
        },
        KeyCode::Down | KeyCode::Char('j') => match state.block.focus {
            BlockFocus::Residents => {
                state.block.selected_resident = navigate_list(
                    state.block.selected_resident,
                    state.block.resident_count,
                    NavKey::Down,
                );
            }
            BlockFocus::Messages => {
                state.block.message_scroll = navigate_list(
                    state.block.message_scroll,
                    state.block.message_count,
                    NavKey::Down,
                );
            }
            BlockFocus::Input => {}
        },
        KeyCode::Char('v') => {
            // Request block invite modal open (shell populates contacts snapshot)
            commands.push(TuiCommand::Dispatch(DispatchCommand::OpenBlockInvite));
        }
        KeyCode::Char('g') => {
            // Grant steward to selected resident
            commands.push(TuiCommand::Dispatch(DispatchCommand::GrantStewardSelected));
        }
        KeyCode::Char('R') => {
            // Revoke steward from selected resident (uppercase R to not conflict with toggle residents)
            commands.push(TuiCommand::Dispatch(DispatchCommand::RevokeStewardSelected));
        }
        KeyCode::Char('r') => {
            state.block.show_residents = !state.block.show_residents;
        }
        KeyCode::Char('n') => {
            // Navigate to neighborhood
            commands.push(TuiCommand::Dispatch(DispatchCommand::NavigateTo(
                Screen::Neighborhood,
            )));
        }
        _ => {}
    }
}

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
                state.chat.message_scroll = navigate_list(
                    state.chat.message_scroll,
                    state.chat.message_count,
                    NavKey::Up,
                );
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
                state.chat.message_scroll = navigate_list(
                    state.chat.message_scroll,
                    state.chat.message_count,
                    NavKey::Down,
                );
            }
            _ => {}
        },
        KeyCode::Char('n') => {
            // Open create channel modal via queue
            let modal_state = CreateChannelModalState::new();
            state
                .modal_queue
                .enqueue(QueuedModal::ChatCreate(modal_state));
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
        KeyCode::Char('g') => {
            // Open guardian setup modal via dispatch (shell will populate contacts)
            if state.is_guardian_setup_modal_active() {
                commands.push(TuiCommand::ShowToast {
                    message: "Guardian setup is already open".to_string(),
                    level: ToastLevel::Info,
                });
            } else {
                commands.push(TuiCommand::Dispatch(DispatchCommand::OpenGuardianSetup));
            }
        }
        KeyCode::Char('c') => {
            // Start chat with selected contact
            commands.push(TuiCommand::Dispatch(DispatchCommand::StartChat));
        }
        KeyCode::Char('i') => {
            // Open import invitation modal via queue (accept an invitation code)
            state.modal_queue.enqueue(QueuedModal::ContactsImport(
                ImportInvitationModalState::default(),
            ));
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
    match key.code {
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
            commands.push(TuiCommand::Dispatch(DispatchCommand::EnterBlock));
        }
        KeyCode::Char('g') | KeyCode::Char('H') => {
            // Go home
            commands.push(TuiCommand::Dispatch(DispatchCommand::GoHome));
        }
        KeyCode::Char('b') | KeyCode::Esc | KeyCode::Backspace => {
            // Back to street
            commands.push(TuiCommand::Dispatch(DispatchCommand::BackToStreet));
        }
        _ => {}
    }
}

/// Handle settings screen key events
pub fn handle_settings_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.settings.section = state.settings.section.prev();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.settings.section = state.settings.section.next();
        }
        KeyCode::Char(' ') => {
            // Space cycles MFA policy when on MFA section
            if state.settings.section == SettingsSection::Mfa {
                state.settings.mfa_policy = state.settings.mfa_policy.next();
                commands.push(TuiCommand::Dispatch(DispatchCommand::UpdateMfaPolicy {
                    policy: state.settings.mfa_policy,
                }));
            }
        }
        KeyCode::Char('m') => {
            state.settings.mfa_policy = state.settings.mfa_policy.next();
            commands.push(TuiCommand::Dispatch(DispatchCommand::UpdateMfaPolicy {
                policy: state.settings.mfa_policy,
            }));
        }
        KeyCode::Char('e') => {
            if state.settings.section == SettingsSection::Profile {
                // Open display name edit modal via queue
                state.modal_queue.enqueue(QueuedModal::SettingsDisplayName(
                    DisplayNameModalState::default(),
                ));
            }
        }
        KeyCode::Enter => {
            match state.settings.section {
                SettingsSection::Profile => {
                    // Open display name edit modal via queue
                    state.modal_queue.enqueue(QueuedModal::SettingsDisplayName(
                        DisplayNameModalState::default(),
                    ));
                }
                SettingsSection::Threshold => {
                    // Open threshold edit modal via dispatch (shell populates current values)
                    commands.push(TuiCommand::Dispatch(DispatchCommand::OpenThresholdModal));
                }
                _ => {}
            }
        }
        KeyCode::Char('t') => {
            if state.settings.section == SettingsSection::Threshold {
                // Open threshold edit modal via dispatch (shell populates current values)
                commands.push(TuiCommand::Dispatch(DispatchCommand::OpenThresholdModal));
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
        _ => {}
    }
}

/// Handle recovery screen key events
pub fn handle_recovery_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    match key.code {
        KeyCode::Left | KeyCode::Char('h') => {
            state.recovery.tab = state.recovery.tab.prev();
            state.recovery.selected_index = 0;
        }
        KeyCode::Right | KeyCode::Char('l') => {
            state.recovery.tab = state.recovery.tab.next();
            state.recovery.selected_index = 0;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.recovery.selected_index = navigate_list(
                state.recovery.selected_index,
                state.recovery.item_count,
                NavKey::Up,
            );
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.recovery.selected_index = navigate_list(
                state.recovery.selected_index,
                state.recovery.item_count,
                NavKey::Down,
            );
        }
        KeyCode::Char('a') => {
            if state.recovery.tab == RecoveryTab::Guardians {
                // Show guardian select modal via queue (contacts will be filled by shell)
                state.modal_queue.enqueue(QueuedModal::GuardianSelect(
                    ContactSelectModalState::single("Select Guardian", Vec::new()),
                ));
            }
        }
        KeyCode::Enter => {
            // Enter approves request on Requests tab
            if state.recovery.tab == RecoveryTab::Requests {
                commands.push(TuiCommand::Dispatch(DispatchCommand::ApproveRecovery));
            }
        }
        KeyCode::Char('s') | KeyCode::Char('r') => {
            // Start recovery on Recovery tab
            if state.recovery.tab == RecoveryTab::Recovery {
                commands.push(TuiCommand::Dispatch(DispatchCommand::StartRecovery));
            }
        }
        _ => {}
    }
}
