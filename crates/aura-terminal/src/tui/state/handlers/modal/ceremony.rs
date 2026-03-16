//! Ceremony modal handlers
//!
//! Handles guardian setup and MFA setup ceremony modals.

use aura_core::effects::terminal::{KeyCode, KeyEvent};

use super::super::super::commands::{DispatchCommand, TuiCommand};
use super::super::super::modal_queue::QueuedModal;
use super::super::super::toast::{QueuedToast, ToastLevel};
use super::super::super::views::{GuardianSetupModalState, GuardianSetupStep};
use super::super::super::TuiState;
use super::{handle_ceremony_escape, parse_authority_id};

/// Handle guardian setup modal keys (queue-based)
pub(super) fn handle_guardian_setup_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: GuardianSetupModalState,
) {
    match modal_state.step() {
        GuardianSetupStep::SelectContacts => {
            match key.code {
                KeyCode::Esc => {
                    state.modal_queue.dismiss();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.move_focus_up();
                        }
                    });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.move_focus_down();
                        }
                    });
                }
                KeyCode::Char(' ') => {
                    // Toggle selection
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.toggle_selection();
                        }
                    });
                }
                KeyCode::Enter => {
                    if modal_state.can_proceed_to_threshold() {
                        state.modal_queue.update_active(|modal| {
                            if let QueuedModal::GuardianSetup(ref mut s) = modal {
                                s.advance_to_threshold();
                            }
                        });
                    }
                }
                _ => {}
            }
        }
        GuardianSetupStep::ChooseThreshold => {
            match key.code {
                KeyCode::Esc => {
                    // Go back to contact selection
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.back_to_selection();
                        }
                    });
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.increment_k();
                        }
                    });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.decrement_k();
                        }
                    });
                }
                KeyCode::Enter => {
                    if modal_state.can_start_ceremony() {
                        let selected_ids = modal_state.selected_contact_ids();
                        let mut contact_ids = Vec::with_capacity(selected_ids.len());
                        for contact in selected_ids {
                            let Some(parsed) =
                                parse_authority_id(state, &contact, "guardian ceremony")
                            else {
                                return;
                            };
                            contact_ids.push(parsed);
                        }
                        // Dispatch command to start guardian setup ceremony
                        commands.push(TuiCommand::Dispatch(
                            DispatchCommand::StartGuardianCeremony {
                                contact_ids,
                                threshold_k:
                                    match super::super::super::commands::ThresholdK::try_from(
                                        modal_state.threshold_k(),
                                    ) {
                                        Ok(value) => value,
                                        Err(error) => {
                                            state.toast_error(error);
                                            return;
                                        }
                                    },
                            },
                        ));

                        // Dismiss modal after sending invitation (mirrors contact invitation flow)
                        state.modal_queue.dismiss();
                    }
                }
                _ => {}
            }
        }
        GuardianSetupStep::CeremonyInProgress
        | GuardianSetupStep::Complete
        | GuardianSetupStep::Error => {
            // During ceremony, allow escape to cancel once the ceremony has started.
            if key.code == KeyCode::Esc {
                handle_ceremony_escape(
                    state,
                    commands,
                    modal_state.ceremony_id().cloned(),
                    "Guardian ceremony is still starting.",
                );
            }
        }
    }
}

/// Handle MFA setup modal keys (queue-based)
pub(super) fn handle_mfa_setup_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: GuardianSetupModalState,
) {
    let is_ctrl_m =
        key.modifiers.ctrl() && matches!(key.code, KeyCode::Char('m') | KeyCode::Char('M'));
    match modal_state.step() {
        GuardianSetupStep::SelectContacts => {
            if is_ctrl_m {
                let mobile_id = state.settings.demo_mobile_device_id.clone();
                if !mobile_id.is_empty() {
                    let mut found = false;
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::MfaSetup(ref mut s) = modal {
                            found = s.select_by_id(&mobile_id);
                        }
                    });
                    if !found {
                        state.next_toast_id += 1;
                        state.toast_queue.enqueue(QueuedToast::new(
                            state.next_toast_id,
                            "Mobile device not found in the list yet.",
                            ToastLevel::Warning,
                        ));
                    }
                } else {
                    state.next_toast_id += 1;
                    state.toast_queue.enqueue(QueuedToast::new(
                        state.next_toast_id,
                        "Demo Mobile device id unavailable.",
                        ToastLevel::Warning,
                    ));
                }
                return;
            }

            match key.code {
                KeyCode::Esc => {
                    state.modal_queue.dismiss();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::MfaSetup(ref mut s) = modal {
                            s.move_focus_up();
                        }
                    });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::MfaSetup(ref mut s) = modal {
                            s.move_focus_down();
                        }
                    });
                }
                KeyCode::Char(' ') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::MfaSetup(ref mut s) = modal {
                            s.toggle_selection();
                        }
                    });
                }
                KeyCode::Enter => {
                    if modal_state.can_proceed_to_threshold() {
                        state.modal_queue.update_active(|modal| {
                            if let QueuedModal::MfaSetup(ref mut s) = modal {
                                s.advance_to_threshold();
                            }
                        });
                    }
                }
                _ => {}
            }
        }
        GuardianSetupStep::ChooseThreshold => match key.code {
            KeyCode::Esc => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::MfaSetup(ref mut s) = modal {
                        s.back_to_selection();
                    }
                });
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::MfaSetup(ref mut s) = modal {
                        s.increment_k();
                    }
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::MfaSetup(ref mut s) = modal {
                        s.decrement_k();
                    }
                });
            }
            KeyCode::Enter => {
                if modal_state.can_start_ceremony() {
                    commands.push(TuiCommand::Dispatch(DispatchCommand::StartMfaCeremony {
                        device_ids: modal_state
                            .selected_contact_ids()
                            .into_iter()
                            .map(Into::into)
                            .collect(),
                        threshold_k: match super::super::super::commands::ThresholdK::try_from(
                            modal_state.threshold_k(),
                        ) {
                            Ok(value) => value,
                            Err(error) => {
                                state.toast_error(error);
                                return;
                            }
                        },
                    }));

                    // Dismiss modal after sending invitation (mirrors guardian invitation flow)
                    state.modal_queue.dismiss();
                }
            }
            _ => {}
        },
        GuardianSetupStep::CeremonyInProgress
        | GuardianSetupStep::Complete
        | GuardianSetupStep::Error => {
            if key.code == KeyCode::Esc {
                handle_ceremony_escape(
                    state,
                    commands,
                    modal_state.ceremony_id().cloned(),
                    "Multifactor ceremony is still starting.",
                );
            }
        }
    }
}
