//! Settings modal handlers
//!
//! Handles nickname suggestion, add device, remove device, authority picker,
//! device select, device enrollment, and device import modals.

use aura_core::effects::terminal::{KeyCode, KeyEvent};

use crate::tui::components::copy_to_clipboard;
use crate::tui::navigation::navigate_list;

use super::super::super::commands::{DispatchCommand, TuiCommand};
use super::super::super::modal_queue::{ContactSelectModalState, QueuedModal};
use super::super::super::views::{
    validate_nickname_suggestion, AddDeviceField, AddDeviceModalState, ConfirmRemoveModalState,
    DeviceEnrollmentCeremonyModalState, DeviceSelectModalState, ImportInvitationModalState,
    NicknameSuggestionError, NicknameSuggestionModalState,
};
use super::super::super::TuiState;
use super::{dismiss_on_escape, list_nav_from_key, modal_text_char_from_key, parse_authority_id};

/// Handle settings nickname suggestion modal keys (queue-based)
pub(super) fn handle_settings_nickname_suggestion_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: NicknameSuggestionModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter => match validate_nickname_suggestion(&modal_state.value) {
            Ok(nickname_suggestion) => {
                commands.push(TuiCommand::Dispatch(
                    DispatchCommand::UpdateNicknameSuggestion {
                        nickname_suggestion,
                    },
                ));
                state.modal_queue.dismiss();
            }
            Err(error) => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::SettingsNicknameSuggestion(ref mut s) = modal {
                        s.error = Some(error.to_string());
                    }
                });
            }
        },
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsNicknameSuggestion(ref mut s) = modal {
                    let mut candidate = s.value.clone();
                    candidate.push(c);
                    // Refuse input past the maximum length instead of failing on submit.
                    match validate_nickname_suggestion(&candidate) {
                        Err(error @ NicknameSuggestionError::TooLong { .. }) => {
                            s.error = Some(error.to_string());
                        }
                        _ => {
                            s.value = candidate;
                            s.error = None;
                        }
                    }
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsNicknameSuggestion(ref mut s) = modal {
                    s.value.pop();
                    s.error = None;
                }
            });
        }
        _ => {}
    }
}

/// Handle settings add device modal keys (queue-based)
///
/// Supports two-step exchange: Tab switches between Name and SetupCode fields.
/// A user-transferred setup code is required before issuing an enrollment invitation.
pub(super) fn handle_settings_add_device_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: AddDeviceModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Tab => {
            // Switch between Name and SetupCode fields
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsAddDevice(ref mut s) = modal {
                    s.focused_field = match s.focused_field {
                        AddDeviceField::Name => AddDeviceField::SetupCode,
                        AddDeviceField::SetupCode => AddDeviceField::Name,
                    };
                }
            });
        }
        KeyCode::Enter => {
            if let Some(missing) = modal_state.missing_field_error() {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::SettingsAddDevice(ref mut s) = modal {
                        s.error = Some(missing.to_string());
                    }
                });
                return;
            }
            if modal_state.can_submit() {
                let setup_code = modal_state.setup_code().to_owned();
                commands.push(TuiCommand::Dispatch(DispatchCommand::AddDevice {
                    name: modal_state.name,
                    setup_code,
                }));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsAddDevice(ref mut s) = modal {
                    s.error = None;
                    match s.focused_field {
                        AddDeviceField::Name => s.name.push(c),
                        AddDeviceField::SetupCode => s.setup_code.push(c),
                    }
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsAddDevice(ref mut s) = modal {
                    s.error = None;
                    match s.focused_field {
                        AddDeviceField::Name => {
                            s.name.pop();
                        }
                        AddDeviceField::SetupCode => {
                            s.setup_code.pop();
                        }
                    }
                }
            });
        }
        _ => {}
    }
}

/// Handle settings remove device modal keys (queue-based)
pub(super) fn handle_settings_remove_device_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ConfirmRemoveModalState,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
            state.modal_queue.dismiss();
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsRemoveDevice(ref mut s) = modal {
                    s.toggle_focus();
                }
            });
        }
        KeyCode::Enter => {
            if modal_state.confirm_focused {
                commands.push(TuiCommand::Dispatch(DispatchCommand::RemoveDevice {
                    device_id: modal_state.device_id,
                }));
            }
            state.modal_queue.dismiss();
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            commands.push(TuiCommand::Dispatch(DispatchCommand::RemoveDevice {
                device_id: modal_state.device_id,
            }));
            state.modal_queue.dismiss();
        }
        _ => {}
    }
}

/// Handle authority picker modal keys (queue-based)
///
/// Similar to contact select but dispatches SwitchAuthority on selection.
pub(super) fn handle_authority_picker_key_queue(
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
            if let QueuedModal::AuthorityPicker(ref mut s) = modal {
                s.selected_index = navigate_list(s.selected_index, s.contacts.len(), nav);
            }
        });
        return;
    }

    let item_count = modal_state.contacts.len();
    match key.code {
        KeyCode::Enter => {
            if item_count > 0 {
                if let Some((authority_id, _)) =
                    modal_state.contacts.get(modal_state.selected_index)
                {
                    let Some(authority_id) =
                        parse_authority_id(state, authority_id.as_str(), "authority switch")
                    else {
                        return;
                    };
                    commands.push(TuiCommand::Dispatch(DispatchCommand::SwitchAuthority {
                        authority_id,
                    }));
                }
            }
            state.modal_queue.dismiss();
        }
        _ => {}
    }
}

/// Handle device selection modal keys (for device removal)
///
/// - Esc: Cancel
/// - Up/Down/j/k: Navigate list (skips current device)
/// - Enter: Select device and show confirmation modal
pub(super) fn handle_device_select_key_queue(
    state: &mut TuiState,
    _commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: DeviceSelectModalState,
) {
    if dismiss_on_escape(state, &key.code) {
        return;
    }

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsDeviceSelect(ref mut s) = modal {
                    s.select_prev();
                }
            });
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsDeviceSelect(ref mut s) = modal {
                    s.select_next();
                }
            });
        }
        KeyCode::Enter => {
            // Get selected device and show confirmation modal
            if let Some(device) = modal_state.selected_device() {
                let device_id = device.id.clone();
                let display_name = device.name.clone();

                // Dismiss device select modal
                state.modal_queue.dismiss();

                // Enqueue confirmation modal
                use super::super::super::views::ConfirmRemoveModalState;
                state.modal_queue.enqueue(QueuedModal::SettingsRemoveDevice(
                    ConfirmRemoveModalState::for_device(&device_id, &display_name),
                ));
            }
        }
        _ => {}
    }
}

/// Handle device enrollment import modal keys (queue-based)
pub(super) fn handle_device_import_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ImportInvitationModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter => {
            if modal_state.can_submit_enrollment() {
                commands.push(TuiCommand::Dispatch(
                    DispatchCommand::ImportDeviceEnrollmentOnMobile {
                        code: modal_state.code,
                        manifest_transfer: Some(
                            aura_app::ui::contract::EnrollmentManifestTransferInput {
                                manifest_code: modal_state.manifest_code,
                                initiator_verifier_code: modal_state.initiator_verifier_code,
                            },
                        ),
                    },
                ));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Tab => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsDeviceImport(s) = modal {
                    s.focused_input = (s.focused_input + 1) % 3;
                }
            });
        }
        KeyCode::Char(_) => {
            let Some(c) = modal_text_char_from_key(&key.code) else {
                return;
            };
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsDeviceImport(ref mut s) = modal {
                    s.enrollment_input_mut().push(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsDeviceImport(ref mut s) = modal {
                    s.enrollment_input_mut().pop();
                }
            });
        }
        _ => {}
    }
}

pub(super) fn handle_device_enrollment_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: DeviceEnrollmentCeremonyModalState,
) {
    match key.code {
        KeyCode::Esc => {
            // If still in progress, Esc cancels the ceremony; otherwise, it just closes.
            if !modal_state.ceremony.is_complete && !modal_state.ceremony.has_failed {
                if let Some(ceremony_id) = modal_state.ceremony.ceremony_id {
                    commands.push(TuiCommand::Dispatch(
                        DispatchCommand::CancelKeyRotationCeremony {
                            ceremony_id: ceremony_id.into(),
                        },
                    ));
                }
            }
            state.modal_queue.dismiss();
        }
        KeyCode::Char('c') => {
            // Copy enrollment code to clipboard (c or Cmd+C)
            if !modal_state.enrollment_code.is_empty()
                && copy_to_clipboard(&modal_state.enrollment_code).is_ok()
            {
                // Update state to show "copied" feedback
                state.modal_queue.update_active(|m| {
                    if let QueuedModal::SettingsDeviceEnrollment(s) = m {
                        s.set_copied();
                    }
                });
                state.toast_success("Copied to clipboard");
            }
        }
        KeyCode::Char('t' | 'v') => {
            if let Some(transfer) = modal_state.manifest_transfer.as_ref() {
                let code = if key.code == KeyCode::Char('t') {
                    &transfer.manifest_code
                } else {
                    &transfer.initiator_verifier_code
                };
                match copy_to_clipboard(code) {
                    Ok(()) => state.toast_success("Copied actual transfer code"),
                    Err(error) => state.toast_error(format!("Copy failed: {error}")),
                }
            } else {
                state.toast_error("Authenticated manifest transfer unavailable");
            }
        }
        _ => {}
    }
}
