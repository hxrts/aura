use super::chat::ensure_named_channel;
use super::set_toast;
use super::wizard::{
    handle_add_device_modal_char, parse_wizard_value, save_capability_config_buffer,
    save_create_channel_details_buffer, toggle_create_channel_member, toggle_guardian_selection,
    toggle_mfa_selection,
};
use crate::model::{
    ActiveModal, AddDeviceWizardStep, CreateChannelDetailsField, CreateChannelModalState,
    CreateChannelWizardStep, ModalState, ScreenId, SelectDeviceModalState,
    ThresholdWizardModalState, ThresholdWizardStep, ToastState, UiModel,
};
use aura_app::frontend_primitives::ClipboardPort;

pub(super) fn handle_modal_enter(
    model: &mut UiModel,
    modal: ModalState,
    clipboard: &dyn ClipboardPort,
) {
    match modal {
        ModalState::Help | ModalState::ChannelInfo => {
            dismiss_modal(model);
        }
        ModalState::CreateInvitation => {
            model.invite_counter = model.invite_counter.saturating_add(1);
            let code = format!("INVITE-{}", model.invite_counter);
            model.last_invite_code = Some(code.clone());
            clipboard.write(&code);
            model.ensure_contact("Bob");
            model.toast = Some(ToastState {
                icon: '✓',
                message: format!("Invitation Created {code}"),
            });
            dismiss_modal(model);
        }
        ModalState::AcceptInvitation => {
            let value = match model.active_modal.as_ref() {
                Some(ActiveModal::AcceptInvitation(state)) => state.value.trim(),
                _ => "",
            };
            if !value.is_empty() {
                let contact_name = match value {
                    "1" => "Alice",
                    "2" => "Carol",
                    _ => "Alice",
                };
                model.ensure_contact(contact_name);
                model.toast = Some(ToastState {
                    icon: '✓',
                    message: "membership updated".to_string(),
                });
            }
            dismiss_modal(model);
        }
        ModalState::CreateHome => {
            let name = match model.active_modal.as_ref() {
                Some(ActiveModal::CreateHome(state)) => state.value.trim().to_string(),
                _ => String::new(),
            };
            if !name.is_empty() {
                model.select_home(
                    format!("home-{}", name.to_lowercase().replace(' ', "-")),
                    name.clone(),
                );
                model.toast = Some(ToastState {
                    icon: '✓',
                    message: format!("Home '{name}' created"),
                });
            }
            dismiss_modal(model);
        }
        ModalState::CreateChannel => {
            if let Some(ActiveModal::CreateChannel(state)) = model.active_modal.as_mut() {
                match state.step {
                    CreateChannelWizardStep::Details => {
                        let channel = state.name.trim().trim_start_matches('#').to_string();
                        if channel.is_empty() {
                            set_toast(model, '✗', "Channel name is required");
                            return;
                        }
                        state.name = channel;
                        state.step = CreateChannelWizardStep::Members;
                        state.member_focus = 0;
                        model.modal_hint = "New Chat Group — Step 2 of 3".to_string();
                    }
                    CreateChannelWizardStep::Members => {
                        let selected_count = state.selected_members.len();
                        let participants = selected_count.saturating_add(1);
                        state.threshold = participants.max(1) as u8;
                        state.step = CreateChannelWizardStep::Threshold;
                        model.modal_hint = "New Chat Group — Step 3 of 3".to_string();
                    }
                    CreateChannelWizardStep::Threshold => {
                        let max_threshold = (state.selected_members.len().saturating_add(1)) as u8;
                        state.threshold = state.threshold.clamp(1, max_threshold.max(1));
                        let channel = state.name.trim().to_string();
                        let topic = state.topic.clone();
                        let channel_id = ensure_named_channel(model, &channel, topic.clone());
                        model.select_channel_id(Some(&channel_id));
                        if !topic.trim().is_empty() {
                            model.set_selected_channel_topic(topic);
                        }
                        model.toast = Some(ToastState {
                            icon: '✓',
                            message: format!("Created '{channel}'."),
                        });
                        dismiss_modal(model);
                    }
                }
            }
        }
        ModalState::SetChannelTopic => {
            let value = match model.active_modal.as_ref() {
                Some(ActiveModal::SetChannelTopic(state)) => state.value.trim().to_string(),
                _ => String::new(),
            };
            model.set_selected_channel_topic(value);
            model.toast = Some(ToastState {
                icon: '✓',
                message: "Topic updated".to_string(),
            });
            dismiss_modal(model);
        }
        ModalState::EditNickname => {
            let value = match model.active_modal.as_ref() {
                Some(ActiveModal::EditNickname(state)) => state.value.trim().to_string(),
                _ => String::new(),
            };
            if model.screen == ScreenId::Settings {
                if !value.is_empty() {
                    model.profile_nickname = value;
                }
            } else if !value.is_empty() {
                model.set_selected_contact_name(value);
            }
            dismiss_modal(model);
        }
        ModalState::RemoveContact => {
            if !model.contacts.is_empty() {
                if let Some(selected_index) = model.selected_contact_index() {
                    model.contacts.remove(selected_index);
                    if model.contacts.is_empty() {
                        model.selected_contact_id = None;
                    } else {
                        model.set_selected_contact_index(selected_index.saturating_sub(1));
                    }
                }
            }
            model.toast = Some(ToastState {
                icon: '✓',
                message: "membership updated".to_string(),
            });
            dismiss_modal(model);
        }
        ModalState::GuardianSetup => {
            if let Some(ActiveModal::GuardianSetup(state)) = model.active_modal.as_mut() {
                match state.step {
                    ThresholdWizardStep::Selection => {
                        let available = model.contacts.len();
                        if available < 2 {
                            set_toast(
                                model,
                                '✗',
                                format!(
                                    "Need at least 2 contacts for this threshold, but only {available} available"
                                ),
                            );
                            return;
                        }
                        if state.selected_indices.len() < 2 {
                            set_toast(model, '✗', "Select at least 2 guardians");
                            return;
                        }
                        let selected = state.selected_indices.len() as u8;
                        state.selected_count = selected;
                        state.threshold_k = state.threshold_k.clamp(1, selected.max(1));
                        state.threshold_input = state.threshold_k.to_string();
                        state.step = ThresholdWizardStep::Threshold;
                    }
                    ThresholdWizardStep::Threshold => {
                        let k = parse_wizard_value(&state.threshold_input, state.threshold_k)
                            .clamp(1, state.selected_count);
                        state.threshold_k = k;
                        state.threshold_input.clear();
                        state.step = ThresholdWizardStep::Ceremony;
                    }
                    ThresholdWizardStep::Ceremony => {
                        let threshold_k = state.threshold_k;
                        let selected_count = state.selected_count;
                        set_toast(
                            model,
                            'ℹ',
                            format!(
                                "Guardian ceremony started! Waiting for {threshold_k}-of-{selected_count} guardians to respond"
                            ),
                        );
                        dismiss_modal(model);
                    }
                }
            }
        }
        ModalState::RequestRecovery => {
            let required = match model.active_modal.as_ref() {
                Some(ActiveModal::GuardianSetup(state)) => state.threshold_k.max(1) as usize,
                _ => 1,
            };
            let available = model.contacts.len();
            if available == 0 {
                set_toast(
                    model,
                    '✗',
                    "Set up guardians first before requesting recovery",
                );
                dismiss_modal(model);
                return;
            }
            if available < required {
                set_toast(
                    model,
                    '✗',
                    format!(
                        "Need {required} guardians for recovery, but only {available} configured"
                    ),
                );
                dismiss_modal(model);
                return;
            }
            set_toast(model, 'ℹ', "Recovery process started");
            dismiss_modal(model);
        }
        ModalState::AddDeviceStep1 => {
            if let Some(ActiveModal::AddDevice(state)) = model.active_modal.as_mut() {
                match state.step {
                    AddDeviceWizardStep::Name => {
                        let name = state.name_input.trim().to_string();
                        if name.is_empty() {
                            set_toast(model, '✗', "Device name is required");
                            return;
                        }

                        state.device_name = name;
                        model.device_enrollment_counter =
                            model.device_enrollment_counter.saturating_add(1);
                        state.enrollment_code =
                            format!("DEVICE-ENROLL-{}", model.device_enrollment_counter);
                        state.name_input.clear();
                        state.step = AddDeviceWizardStep::ShareCode;
                        model.modal_hint = "Add Device — Step 2 of 3".to_string();
                    }
                    AddDeviceWizardStep::ShareCode => {
                        state.step = AddDeviceWizardStep::Confirm;
                        model.modal_hint = "Add Device — Step 3 of 3".to_string();
                    }
                    AddDeviceWizardStep::Confirm => {
                        set_toast(model, 'ℹ', "Device enrollment started");
                        dismiss_modal(model);
                    }
                }
            }
        }
        ModalState::ImportDeviceEnrollmentCode => {
            let code = match model.active_modal.as_ref() {
                Some(ActiveModal::ImportDeviceEnrollmentCode(state)) => state.value.trim(),
                _ => "",
            };
            if code.is_empty() {
                set_toast(model, '✗', "Enrollment code is required");
                return;
            }
            model.has_secondary_device = true;
            if model.secondary_device_name().is_none() {
                let fallback = match model.active_modal.as_ref() {
                    Some(ActiveModal::AddDevice(state)) if !state.device_name.trim().is_empty() => {
                        state.device_name.clone()
                    }
                    _ => "Mobile".to_string(),
                };
                model.set_secondary_device_name(Some(fallback));
            }
            set_toast(model, '✓', "Device enrollment complete");
            dismiss_modal(model);
        }
        ModalState::SelectDeviceToRemove => {
            let candidate_name = match model.active_modal.as_ref() {
                Some(ActiveModal::SelectDeviceToRemove(state)) => state.candidate_name.clone(),
                _ => model
                    .secondary_device_name()
                    .unwrap_or("Secondary device")
                    .to_string(),
            };
            model.modal_hint = "Confirm Device Removal".to_string();
            model.active_modal = Some(ActiveModal::ConfirmRemoveDevice(SelectDeviceModalState {
                candidate_name,
            }));
        }
        ModalState::ConfirmRemoveDevice => {
            if model.has_secondary_device {
                model.has_secondary_device = false;
                model.set_secondary_device_name(None);
                set_toast(model, '✓', "Device removal complete");
            } else {
                set_toast(model, 'ℹ', "Cannot remove the current device");
            }
            dismiss_modal(model);
        }
        ModalState::MfaSetup => {
            if let Some(ActiveModal::MfaSetup(state)) = model.active_modal.as_mut() {
                match state.step {
                    ThresholdWizardStep::Selection => {
                        if state.selected_indices.is_empty() {
                            set_toast(model, '✗', "Select at least 1 device");
                            return;
                        }
                        let selected = state.selected_indices.len() as u8;
                        state.selected_count = selected;
                        state.threshold_k = state.threshold_k.clamp(1, state.selected_count);
                        state.threshold_input = state.threshold_k.to_string();
                        state.step = ThresholdWizardStep::Threshold;
                    }
                    ThresholdWizardStep::Threshold => {
                        let k = parse_wizard_value(&state.threshold_input, state.threshold_k)
                            .clamp(1, state.selected_count);
                        state.threshold_k = k;
                        state.threshold_input.clear();
                        state.step = ThresholdWizardStep::Ceremony;
                    }
                    ThresholdWizardStep::Ceremony => {
                        let threshold_k = state.threshold_k;
                        let selected_count = state.selected_count;
                        set_toast(
                            model,
                            'ℹ',
                            format!(
                                "Multifactor ceremony started ({threshold_k}-of-{selected_count})"
                            ),
                        );
                        dismiss_modal(model);
                    }
                }
            }
        }
        ModalState::AssignModerator => {
            if model.contacts.is_empty() {
                model.toast = Some(ToastState {
                    icon: '✗',
                    message: "Cannot designate non-member".to_string(),
                });
            } else {
                model.toast = Some(ToastState {
                    icon: '✓',
                    message: "moderator granted".to_string(),
                });
            }
            dismiss_modal(model);
        }
        ModalState::SwitchAuthority => {
            model.toast = Some(ToastState {
                icon: '✓',
                message: "Authority switch requested".to_string(),
            });
            dismiss_modal(model);
        }
        ModalState::AccessOverride => {
            model.toast = Some(ToastState {
                icon: '✓',
                message: "Access override updated".to_string(),
            });
            dismiss_modal(model);
        }
        ModalState::CapabilityConfig => {
            model.toast = Some(ToastState {
                icon: '✓',
                message: "Capability config saved".to_string(),
            });
            dismiss_modal(model);
        }
    }
}

pub(super) fn handle_escape(model: &mut UiModel) {
    if model.input_mode {
        model.input_mode = false;
        model.input_buffer.clear();
        return;
    }
    if let Some(modal) = model.modal_state() {
        if handle_modal_escape(model, modal) {
            return;
        }
        dismiss_modal(model);
        return;
    }
    if model.screen == ScreenId::Contacts && model.contact_details {
        model.contact_details = false;
        return;
    }
    if model.screen == ScreenId::Neighborhood
        && matches!(
            model.neighborhood_mode,
            crate::model::NeighborhoodMode::Detail
        )
    {
        model.neighborhood_mode = crate::model::NeighborhoodMode::Map;
        return;
    }
    model.toast = None;
}

pub(super) fn dismiss_modal(model: &mut UiModel) {
    model.dismiss_modal();
}

pub(super) fn handle_modal_tab(model: &mut UiModel, reverse: bool) -> bool {
    let channel_details_mode = matches!(
        model.active_modal,
        Some(ActiveModal::CreateChannel(CreateChannelModalState {
            step: CreateChannelWizardStep::Details,
            ..
        }))
    );
    if channel_details_mode {
        save_create_channel_details_buffer(model);
        if let Some(ActiveModal::CreateChannel(state)) = model.active_modal.as_mut() {
            state.active_field = if reverse {
                CreateChannelDetailsField::Name
            } else {
                match state.active_field {
                    CreateChannelDetailsField::Name => CreateChannelDetailsField::Topic,
                    CreateChannelDetailsField::Topic => CreateChannelDetailsField::Name,
                }
            };
        }
        return true;
    }

    if matches!(model.modal_state(), Some(ModalState::CapabilityConfig)) {
        save_capability_config_buffer(model);
        if let Some(ActiveModal::CapabilityConfig(state)) = model.active_modal.as_mut() {
            state.active_tier = if reverse {
                state.active_tier.prev()
            } else {
                state.active_tier.next()
            };
        }
        return true;
    }

    if matches!(model.modal_state(), Some(ModalState::AccessOverride)) {
        if let Some(ActiveModal::AccessOverride(state)) = model.active_modal.as_mut() {
            state.level = state.level.toggle();
        }
        return true;
    }

    false
}

fn modal_accepts_text(model: &UiModel, modal: ModalState) -> bool {
    let _ = modal;
    model.modal_accepts_text()
}

pub(super) fn backspace(model: &mut UiModel) {
    if model.input_mode {
        model.input_buffer.pop();
    } else if let Some(modal) = model.modal_state() {
        if modal_accepts_text(model, modal) {
            model.pop_modal_text_char();
        }
    }
}

pub(super) fn handle_modal_char(
    model: &mut UiModel,
    modal: ModalState,
    ch: char,
    clipboard: &dyn ClipboardPort,
) -> bool {
    match modal {
        ModalState::AddDeviceStep1 => {
            handle_add_device_modal_char(model, ch, clipboard);
            true
        }
        ModalState::CreateChannel => match model.active_modal.as_mut() {
            Some(ActiveModal::CreateChannel(state))
                if matches!(state.step, CreateChannelWizardStep::Members) =>
            {
                if matches!(ch, ' ') {
                    toggle_create_channel_member(model);
                    true
                } else if matches!(ch, 'j' | 'k') {
                    if model.contacts.is_empty() {
                        return true;
                    }
                    let max = model.contacts.len().saturating_sub(1);
                    if ch == 'k' {
                        state.member_focus = state.member_focus.saturating_sub(1);
                    } else {
                        state.member_focus = (state.member_focus + 1).min(max);
                    }
                    true
                } else {
                    false
                }
            }
            _ => false,
        },
        ModalState::GuardianSetup => {
            if matches!(
                model.active_modal,
                Some(ActiveModal::GuardianSetup(ThresholdWizardModalState {
                    step: ThresholdWizardStep::Selection,
                    ..
                }))
            ) {
                if matches!(ch, ' ') {
                    toggle_guardian_selection(model);
                    true
                } else if matches!(ch, 'j' | 'k') {
                    if model.contacts.is_empty() {
                        return true;
                    }
                    let max = model.contacts.len().saturating_sub(1);
                    if let Some(ActiveModal::GuardianSetup(state)) = model.active_modal.as_mut() {
                        if ch == 'k' {
                            state.focus_index = state.focus_index.saturating_sub(1);
                        } else {
                            state.focus_index = (state.focus_index + 1).min(max);
                        }
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            }
        }
        ModalState::MfaSetup => {
            if matches!(
                model.active_modal,
                Some(ActiveModal::MfaSetup(ThresholdWizardModalState {
                    step: ThresholdWizardStep::Selection,
                    ..
                }))
            ) {
                if matches!(ch, ' ') {
                    toggle_mfa_selection(model);
                    true
                } else if matches!(ch, 'j' | 'k') {
                    let max =
                        super::wizard::available_device_count(model).saturating_sub(1) as usize;
                    if let Some(ActiveModal::MfaSetup(state)) = model.active_modal.as_mut() {
                        if ch == 'k' {
                            state.focus_index = state.focus_index.saturating_sub(1);
                        } else {
                            state.focus_index = (state.focus_index + 1).min(max);
                        }
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            }
        }
        ModalState::ConfirmRemoveDevice => {
            if matches!(ch, 'y' | 'Y') {
                handle_modal_enter(model, ModalState::ConfirmRemoveDevice, clipboard);
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

fn handle_modal_escape(model: &mut UiModel, modal: ModalState) -> bool {
    match modal {
        ModalState::CreateChannel => match model.active_modal.as_mut() {
            Some(ActiveModal::CreateChannel(state)) => match state.step {
                CreateChannelWizardStep::Details => false,
                CreateChannelWizardStep::Members => {
                    state.step = CreateChannelWizardStep::Details;
                    state.active_field = CreateChannelDetailsField::Name;
                    model.modal_hint = "New Chat Group — Step 1 of 3".to_string();
                    true
                }
                CreateChannelWizardStep::Threshold => {
                    state.step = CreateChannelWizardStep::Members;
                    model.modal_hint = "New Chat Group — Step 2 of 3".to_string();
                    true
                }
            },
            _ => false,
        },
        ModalState::GuardianSetup => match model.active_modal.as_mut() {
            Some(ActiveModal::GuardianSetup(state)) => match state.step {
                ThresholdWizardStep::Selection => false,
                ThresholdWizardStep::Threshold => {
                    state.step = ThresholdWizardStep::Selection;
                    state.threshold_input.clear();
                    model.modal_hint = "Guardian Setup — Step 1 of 3".to_string();
                    true
                }
                ThresholdWizardStep::Ceremony => false,
            },
            _ => false,
        },
        ModalState::MfaSetup => match model.active_modal.as_mut() {
            Some(ActiveModal::MfaSetup(state)) => match state.step {
                ThresholdWizardStep::Selection => false,
                ThresholdWizardStep::Threshold => {
                    state.step = ThresholdWizardStep::Selection;
                    state.threshold_input.clear();
                    model.modal_hint = "Multifactor Setup — Step 1 of 3".to_string();
                    true
                }
                ThresholdWizardStep::Ceremony => false,
            },
            _ => false,
        },
        _ => false,
    }
}
