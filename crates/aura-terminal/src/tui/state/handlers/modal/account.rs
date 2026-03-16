//! Account setup modal handler

use aura_core::effects::terminal::{KeyCode, KeyEvent};

use super::super::super::commands::{DispatchCommand, TuiCommand};
use super::super::super::modal_queue::QueuedModal;
use super::super::super::views::{AccountSetupField, AccountSetupModalState};
use super::super::super::TuiState;

/// Handle account setup modal keys (queue-based)
pub(super) fn handle_account_setup_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    current_state: AccountSetupModalState,
) {
    // If we're in success state, Enter dismisses
    if current_state.success {
        if key.code == KeyCode::Enter {
            state.modal_queue.dismiss();
        }
        return;
    }

    // If we're in error state, Enter resets to input
    if current_state.error.is_some() {
        if key.code == KeyCode::Enter {
            // Reset to input state
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::AccountSetup(ref mut s) = modal {
                    s.reset_to_input();
                }
            });
        }
        return;
    }

    // If we're creating, don't process input
    if current_state.creating {
        return;
    }

    // Normal input handling
    match key.code {
        KeyCode::Tab => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::AccountSetup(ref mut s) = modal {
                    s.focus_next_field();
                }
            });
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::AccountSetup(ref mut s) = modal {
                    match s.active_field {
                        AccountSetupField::AccountName => s.nickname_suggestion.push(c),
                        AccountSetupField::DeviceImportCode => s.device_import_code.push(c),
                    }
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::AccountSetup(ref mut s) = modal {
                    match s.active_field {
                        AccountSetupField::AccountName => {
                            s.nickname_suggestion.pop();
                        }
                        AccountSetupField::DeviceImportCode => {
                            s.device_import_code.pop();
                        }
                    }
                }
            });
        }
        KeyCode::Enter => match current_state.active_field {
            AccountSetupField::AccountName if current_state.can_create_account() => {
                let name = current_state.nickname_suggestion;
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::AccountSetup(ref mut s) = modal {
                        s.start_submitting();
                    }
                });
                commands.push(TuiCommand::Dispatch(DispatchCommand::CreateAccount {
                    name,
                }));
            }
            AccountSetupField::DeviceImportCode if current_state.can_import_device() => {
                let code = current_state.device_import_code;
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::AccountSetup(ref mut s) = modal {
                        s.start_submitting();
                    }
                });
                commands.push(TuiCommand::Dispatch(
                    DispatchCommand::ImportDeviceEnrollmentDuringOnboarding { code },
                ));
            }
            _ => {}
        },
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        _ => {}
    }
}
