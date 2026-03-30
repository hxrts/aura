//! Chat modal handlers
//!
//! Handles chat create, topic edit, member select, help, and confirm modals.

use aura_core::effects::terminal::{KeyCode, KeyEvent};

use crate::tui::navigation::{navigate_list, NavKey};

use super::super::super::commands::{DispatchCommand, TuiCommand};
use super::super::super::modal_queue::{ChatMemberSelectModalState, ConfirmAction, QueuedModal};
use super::super::super::views::{CreateChannelModalState, TopicModalState};
use super::super::super::TuiState;
use super::parse_authority_id;

/// Handle help modal keys (queue-based)
pub(super) fn handle_help_modal_key_queue(state: &mut TuiState, key: KeyEvent) {
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
pub(super) fn handle_confirm_modal_key_queue(
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
                    ConfirmAction::RevokeFriendship => {
                        commands.push(TuiCommand::Dispatch(
                            DispatchCommand::RevokeSelectedFriendship,
                        ));
                    }
                    ConfirmAction::DeclineFriendRequest => {
                        commands.push(TuiCommand::Dispatch(
                            DispatchCommand::DeclineSelectedFriendRequest,
                        ));
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

/// Handle chat create channel modal keys (queue-based)
pub(super) fn handle_chat_create_key_queue(
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
                    let mut parsed_members = Vec::with_capacity(members.len());
                    for member in members {
                        let Some(parsed) = parse_authority_id(state, &member, "channel creation")
                        else {
                            return;
                        };
                        parsed_members.push(parsed);
                    }
                    let channel_name = modal_state.name.clone();

                    commands.push(TuiCommand::Dispatch(DispatchCommand::CreateChannel {
                        name: channel_name,
                        topic,
                        members: parsed_members,
                        threshold_k: match super::super::super::commands::ThresholdK::try_from(
                            modal_state.threshold_k,
                        ) {
                            Ok(value) => value,
                            Err(error) => {
                                state.toast_error(error);
                                return;
                            }
                        },
                    }));

                    // Dismiss modal; success/error toasts are emitted from async callbacks.
                    state.modal_queue.dismiss();
                }
            }
            _ => {}
        },
    }
}

/// Handle chat member selection modal keys (queue-based)
pub(super) fn handle_chat_member_select_key_queue(
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
            draft.member_ids = modal_state
                .picker
                .selected_ids
                .into_iter()
                .map(|id| id.to_string())
                .collect();
            state.modal_queue.update_active(|modal| {
                *modal = QueuedModal::ChatCreate(draft);
            });
        }
        _ => {}
    }
}

/// Handle chat topic edit modal keys (queue-based)
pub(super) fn handle_chat_topic_key_queue(
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
                channel_id: modal_state.channel_id.clone().into(),
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
