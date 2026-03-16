//! Neighborhood modal handlers
//!
//! Handles moderator assignment, access override, and capability config modals.

use aura_core::effects::terminal::{KeyCode, KeyEvent};

use crate::tui::navigation::navigate_list;

use super::super::super::commands::{DispatchCommand, TuiCommand};
use super::super::super::modal_queue::QueuedModal;
use super::super::super::views::{
    AccessOverrideModalState, HomeCapabilityConfigModalState, ModeratorAssignmentModalState,
};
use super::super::super::TuiState;
use super::{dismiss_on_escape, list_nav_from_key, parse_authority_id, warn_no_selection};

pub(super) fn handle_neighborhood_moderator_modal_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ModeratorAssignmentModalState,
) {
    if dismiss_on_escape(state, &key.code) {
        return;
    }

    if let Some(nav) = list_nav_from_key(&key.code) {
        state.modal_queue.update_active(|modal| {
            if let QueuedModal::NeighborhoodModeratorAssignment(ref mut s) = modal {
                s.selected_index = navigate_list(s.selected_index, s.contacts.len(), nav);
            }
        });
        return;
    }

    match key.code {
        KeyCode::Tab => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::NeighborhoodModeratorAssignment(ref mut s) = modal {
                    s.toggle_mode();
                }
            });
        }
        KeyCode::Enter => {
            if let Some(target_id) = modal_state.selected_contact_id() {
                let Some(target_id) = parse_authority_id(state, target_id, "moderator assignment")
                else {
                    return;
                };
                commands.push(TuiCommand::Dispatch(
                    DispatchCommand::SubmitModeratorAssignment {
                        target_id,
                        assign: modal_state.assign,
                    },
                ));
            } else {
                warn_no_selection(state, "contact");
            }
        }
        _ => {}
    }
}

pub(super) fn handle_neighborhood_access_override_modal_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: AccessOverrideModalState,
) {
    if dismiss_on_escape(state, &key.code) {
        return;
    }

    if let Some(nav) = list_nav_from_key(&key.code) {
        state.modal_queue.update_active(|modal| {
            if let QueuedModal::NeighborhoodAccessOverride(ref mut s) = modal {
                s.selected_index = navigate_list(s.selected_index, s.contacts.len(), nav);
            }
        });
        return;
    }

    match key.code {
        KeyCode::Tab => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::NeighborhoodAccessOverride(ref mut s) = modal {
                    s.toggle_access_level();
                }
            });
        }
        KeyCode::Enter => {
            if let Some(target_id) = modal_state.selected_contact_id() {
                let Some(target_id) = parse_authority_id(state, target_id, "access override")
                else {
                    return;
                };
                commands.push(TuiCommand::Dispatch(
                    DispatchCommand::SubmitAccessOverride {
                        target_id,
                        access_level: modal_state.access_level,
                    },
                ));
            } else {
                warn_no_selection(state, "contact");
            }
        }
        _ => {}
    }
}

pub(super) fn handle_neighborhood_capability_config_modal_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: HomeCapabilityConfigModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Tab => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::NeighborhoodCapabilityConfig(ref mut s) = modal {
                    s.next_field();
                }
            });
        }
        KeyCode::Enter => {
            if modal_state.can_submit() {
                let config = match super::super::super::commands::HomeCapabilityConfig::parse(
                    &modal_state.full_caps,
                    &modal_state.partial_caps,
                    &modal_state.limited_caps,
                ) {
                    Ok(config) => config,
                    Err(error) => {
                        state.toast_error(error);
                        return;
                    }
                };
                commands.push(TuiCommand::Dispatch(
                    DispatchCommand::SubmitHomeCapabilityConfig { config },
                ));
            } else {
                state.toast_warning("All capability fields must be non-empty");
            }
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::NeighborhoodCapabilityConfig(ref mut s) = modal {
                    s.push_char(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::NeighborhoodCapabilityConfig(ref mut s) = modal {
                    s.pop_char();
                }
            });
        }
        _ => {}
    }
}
