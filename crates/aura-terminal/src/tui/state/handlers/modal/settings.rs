//! Settings modal handlers
//!
//! Handles nickname suggestion, add device, remove device, authority picker,
//! device select, device enrollment, and device import modals.

use aura_core::effects::terminal::{KeyCode, KeyEvent};

use crate::tui::components::copy_to_clipboard;
use crate::tui::navigation::navigate_list;

use super::super::super::commands::{DispatchCommand, TuiCommand};
use super::super::super::modal_queue::{ContactSelectModalState, QueuedModal};
use super::super::super::toast::{QueuedToast, ToastLevel};
use super::super::super::views::{
    AddDeviceField, AddDeviceModalState, ConfirmRemoveModalState,
    DeviceEnrollmentCeremonyModalState, DeviceSelectModalState, ImportInvitationModalState,
    NicknameSuggestionModalState,
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
        KeyCode::Enter => {
            if modal_state.can_submit() {
                commands.push(TuiCommand::Dispatch(
                    DispatchCommand::UpdateNicknameSuggestion {
                        nickname_suggestion: modal_state.value,
                    },
                ));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsNicknameSuggestion(ref mut s) = modal {
                    s.value.push(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsNicknameSuggestion(ref mut s) = modal {
                    s.value.pop();
                }
            });
        }
        _ => {}
    }
}

/// Handle settings add device modal keys (queue-based)
///
/// Supports two-step exchange: Tab switches between Name and InviteeAuthority fields.
/// If invitee_authority_id is provided, uses DeviceEnrollment choreography.
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
            // Switch between Name and InviteeAuthority fields
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsAddDevice(ref mut s) = modal {
                    s.focused_field = match s.focused_field {
                        AddDeviceField::Name => AddDeviceField::InviteeAuthority,
                        AddDeviceField::InviteeAuthority => AddDeviceField::Name,
                    };
                }
            });
        }
        KeyCode::Enter => {
            if modal_state.can_submit() {
                let invitee_authority_id = match parse_authority_id(
                    state,
                    modal_state.invitee_authority(),
                    "device enrollment invitee",
                ) {
                    Some(id) => id,
                    None => return,
                };
                commands.push(TuiCommand::Dispatch(DispatchCommand::AddDevice {
                    name: modal_state.name,
                    invitee_authority_id,
                }));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsAddDevice(ref mut s) = modal {
                    match s.focused_field {
                        AddDeviceField::Name => s.name.push(c),
                        AddDeviceField::InviteeAuthority => s.invitee_authority_id.push(c),
                    }
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsAddDevice(ref mut s) = modal {
                    match s.focused_field {
                        AddDeviceField::Name => {
                            s.name.pop();
                        }
                        AddDeviceField::InviteeAuthority => {
                            s.invitee_authority_id.pop();
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
    // Demo shortcut: Ctrl+M fills the Mobile device enrollment code.
    let is_ctrl_m =
        key.modifiers.ctrl() && matches!(key.code, KeyCode::Char('m') | KeyCode::Char('M'));
    let is_enter_autofill = key.code == KeyCode::Enter
        && modal_state.code.is_empty()
        && !state.settings.demo_mobile_device_id.is_empty();

    if is_ctrl_m || is_enter_autofill {
        state.toast_queue.dismiss();
        let code = state.settings.last_device_enrollment_code.clone();
        if !code.is_empty() {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsDeviceImport(ref mut s) = modal {
                    s.code = code.clone();
                }
            });
        } else {
            state.settings.pending_mobile_enrollment_autofill = true;
            if state.settings.demo_mobile_authority_id.is_empty() {
                state.next_toast_id += 1;
                state.toast_queue.enqueue(QueuedToast::new(
                    state.next_toast_id,
                    "Mobile enrollment requires a configured invitee authority ID",
                    ToastLevel::Error,
                ));
                return;
            }
            commands.push(TuiCommand::Dispatch(DispatchCommand::AddDevice {
                name: "Mobile".to_string(),
                invitee_authority_id: state
                    .settings
                    .demo_mobile_authority_id
                    .parse()
                    .unwrap_or_else(|error| {
                        panic!("demo mobile authority id should already be validated: {error}")
                    }),
            }));
            state.next_toast_id += 1;
            state.toast_queue.enqueue(QueuedToast::new(
                state.next_toast_id,
                "Generating Mobile enrollment code…",
                ToastLevel::Info,
            ));
        }
        return;
    }

    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter => {
            if modal_state.can_submit() {
                commands.push(TuiCommand::Dispatch(
                    DispatchCommand::ImportDeviceEnrollmentOnMobile {
                        code: modal_state.code,
                    },
                ));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(_) => {
            let Some(c) = modal_text_char_from_key(&key.code) else {
                return;
            };
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsDeviceImport(ref mut s) = modal {
                    s.code.push(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsDeviceImport(ref mut s) = modal {
                    s.code.pop();
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
        KeyCode::Char('m' | 'M')
            if key.modifiers.ctrl() || !state.settings.demo_mobile_device_id.is_empty() =>
        {
            // Demo mode: allow plain m/M as harness-friendly fallback in addition to Ctrl+M.
            let is_demo = !state.settings.demo_mobile_device_id.is_empty();
            let is_pending = !modal_state.ceremony.is_complete && !modal_state.ceremony.has_failed;
            if is_demo && is_pending && !modal_state.enrollment_code.is_empty() {
                commands.push(TuiCommand::Dispatch(
                    DispatchCommand::ImportDeviceEnrollmentOnMobile {
                        code: modal_state.enrollment_code.clone(),
                    },
                ));
                state.toast_info("Sending code to Mobile agent...");
            }
        }
        _ => {}
    }
}
