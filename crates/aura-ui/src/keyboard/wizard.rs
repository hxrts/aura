use super::navigation::bounded_step;
use super::set_toast;
use crate::model::{
    ActiveModal, AddDeviceModalState, AddDeviceWizardStep, CapabilityTier,
    CreateChannelDetailsField, CreateChannelModalState, CreateChannelWizardStep, ModalState,
    SelectDeviceModalState, ThresholdWizardModalState, ThresholdWizardStep, UiModel,
};
use aura_app::frontend_primitives::ClipboardPort;

pub(super) fn open_create_channel_wizard(model: &mut UiModel) {
    model.modal_hint = "New Chat Group — Step 1 of 3".to_string();
    model.active_modal = Some(ActiveModal::CreateChannel(
        CreateChannelModalState::default(),
    ));
}

pub(super) fn save_create_channel_details_buffer(model: &mut UiModel) {
    let value = model.modal_text_value().unwrap_or_default();
    if let Some(ActiveModal::CreateChannel(state)) = model.active_modal.as_mut() {
        match state.active_field {
            CreateChannelDetailsField::Name => state.name = value,
            CreateChannelDetailsField::Topic => state.topic = value,
        }
    }
}

pub(super) fn toggle_create_channel_member(model: &mut UiModel) {
    if model.contacts.is_empty() {
        return;
    }
    if let Some(ActiveModal::CreateChannel(state)) = model.active_modal.as_mut() {
        let idx = state
            .member_focus
            .min(model.contacts.len().saturating_sub(1));
        if let Some(position) = state
            .selected_members
            .iter()
            .position(|selected| *selected == idx)
        {
            state.selected_members.remove(position);
        } else {
            state.selected_members.push(idx);
            state.selected_members.sort_unstable();
        }
    }
}

pub(super) fn open_add_device_wizard(model: &mut UiModel) {
    model.modal_hint = "Add Device — Step 1 of 3".to_string();
    model.active_modal = Some(ActiveModal::AddDevice(AddDeviceModalState::default()));
}

pub(super) fn open_guardian_setup_wizard(model: &mut UiModel) {
    let mut selected_indices: Vec<usize> = model
        .contacts
        .iter()
        .enumerate()
        .filter(|(_, contact)| contact.is_guardian)
        .map(|(idx, _)| idx)
        .collect();
    if selected_indices.is_empty() {
        let selected = model.contacts.len().min(2);
        selected_indices = (0..selected).collect();
    }
    let selected_count = selected_indices.len().max(1) as u8;
    let threshold_k = selected_count.clamp(1, 2);
    model.modal_hint = "Guardian Setup — Step 1 of 3".to_string();
    let mut state = ThresholdWizardModalState::with_defaults(selected_count, threshold_k);
    state.selected_indices = selected_indices;
    model.active_modal = Some(ActiveModal::GuardianSetup(state));
}

pub(super) fn can_open_guardian_setup_wizard(model: &mut UiModel) -> bool {
    if model.contacts.is_empty() {
        set_toast(model, '✗', "Add contacts first before setting up guardians");
        return false;
    }
    true
}

pub(super) fn open_remove_device_selection(model: &mut UiModel) {
    let candidate_name = model
        .secondary_device_name()
        .unwrap_or("Secondary device")
        .to_string();
    model.modal_hint = "Select Device to Remove".to_string();
    model.active_modal = Some(ActiveModal::SelectDeviceToRemove(SelectDeviceModalState {
        candidate_name,
    }));
}

pub(super) fn open_mfa_setup_wizard(model: &mut UiModel) {
    let selected_indices = (0..available_device_count(model) as usize).collect::<Vec<_>>();
    let selected_count = selected_indices.len().max(1) as u8;
    let threshold_k = selected_count.clamp(1, 2);
    model.modal_hint = "Multifactor Setup — Step 1 of 3".to_string();
    let mut state = ThresholdWizardModalState::with_defaults(selected_count, threshold_k);
    state.selected_indices = selected_indices;
    model.active_modal = Some(ActiveModal::MfaSetup(state));
}

pub(super) fn save_capability_config_buffer(model: &mut UiModel) {
    let value = model.modal_text_value().unwrap_or_default();
    if let Some(ActiveModal::CapabilityConfig(state)) = model.active_modal.as_mut() {
        match state.active_tier {
            CapabilityTier::Full => state.full_caps = value,
            CapabilityTier::Partial => state.partial_caps = value,
            CapabilityTier::Limited => state.limited_caps = value,
        }
    }
}

pub(super) fn can_open_mfa_setup_wizard(model: &mut UiModel) -> bool {
    let available = available_device_count(model) as usize;
    if available < 2 {
        set_toast(
            model,
            '✗',
            format!("MFA requires at least 2 devices, but only {available} available"),
        );
        return false;
    }
    true
}

pub(super) fn handle_add_device_modal_char(
    model: &mut UiModel,
    ch: char,
    clipboard: &dyn ClipboardPort,
) {
    match model.active_modal.as_mut() {
        Some(ActiveModal::AddDevice(state)) if matches!(state.step, AddDeviceWizardStep::Name) => {
            state.push_draft_name_char(ch);
        }
        Some(ActiveModal::AddDevice(state))
            if matches!(
                state.step,
                AddDeviceWizardStep::ShareCode | AddDeviceWizardStep::Confirm
            ) =>
        {
            if matches!(ch, 'c' | 'y') && !state.enrollment_code.is_empty() {
                clipboard.write(&state.enrollment_code);
                state.code_copied = true;
                set_toast(model, '✓', "Copied to clipboard");
            }
        }
        _ => {}
    }
}

pub(super) fn handle_wizard_named_key(model: &mut UiModel, key_name: &str) -> bool {
    match model.modal_state() {
        Some(ModalState::CreateChannel) => {
            if matches!(key_name, "up" | "down") {
                match model.active_modal.as_mut() {
                    Some(ActiveModal::CreateChannel(state))
                        if matches!(state.step, CreateChannelWizardStep::Members) =>
                    {
                        if model.contacts.is_empty() {
                            return true;
                        }
                        let max = model.contacts.len().saturating_sub(1);
                        let delta = if key_name == "up" { -1 } else { 1 };
                        state.member_focus = bounded_step(state.member_focus, delta, max);
                        return true;
                    }
                    Some(ActiveModal::CreateChannel(state))
                        if matches!(state.step, CreateChannelWizardStep::Threshold) =>
                    {
                        let current =
                            parse_wizard_value(&state.threshold.to_string(), state.threshold);
                        let max = (state.selected_members.len().saturating_add(1)) as u8;
                        let adjusted = if key_name == "up" {
                            current.saturating_add(1).min(max.max(1))
                        } else {
                            current.saturating_sub(1).max(1)
                        };
                        state.threshold = adjusted;
                        return true;
                    }
                    _ => {}
                }
            }
        }
        Some(ModalState::GuardianSetup) => {
            if matches!(key_name, "up" | "down") {
                match model.active_modal.as_mut() {
                    Some(ActiveModal::GuardianSetup(state))
                        if matches!(state.step, ThresholdWizardStep::Selection) =>
                    {
                        if model.contacts.is_empty() {
                            return true;
                        }
                        let max = model.contacts.len().saturating_sub(1);
                        let delta = if key_name == "up" { -1 } else { 1 };
                        state.focus_index = bounded_step(state.focus_index, delta, max);
                    }
                    Some(ActiveModal::GuardianSetup(state))
                        if matches!(state.step, ThresholdWizardStep::Threshold) =>
                    {
                        let delta = if key_name == "up" { 1 } else { -1 };
                        adjust_threshold_wizard_input(model, true, delta);
                    }
                    _ => {}
                }
                return true;
            }
        }
        Some(ModalState::MfaSetup) => {
            if matches!(key_name, "up" | "down") {
                let max = available_device_count(model).saturating_sub(1) as usize;
                match model.active_modal.as_mut() {
                    Some(ActiveModal::MfaSetup(state))
                        if matches!(state.step, ThresholdWizardStep::Selection) =>
                    {
                        let delta = if key_name == "up" { -1 } else { 1 };
                        state.focus_index = bounded_step(state.focus_index, delta, max);
                    }
                    Some(ActiveModal::MfaSetup(state))
                        if matches!(state.step, ThresholdWizardStep::Threshold) =>
                    {
                        let delta = if key_name == "up" { 1 } else { -1 };
                        adjust_threshold_wizard_input(model, false, delta);
                    }
                    _ => {}
                }
                return true;
            }
        }
        Some(ModalState::SelectDeviceToRemove) => {
            if matches!(key_name, "up" | "down") {
                return true;
            }
        }
        Some(ModalState::SwitchAuthority) => {
            if matches!(key_name, "up" | "down") {
                if model.authorities.is_empty() {
                    return true;
                }
                let max = model.authorities.len().saturating_sub(1);
                let selected_index = model.selected_authority_index().unwrap_or_default();
                let delta = if key_name == "up" { -1 } else { 1 };
                model.set_selected_authority_index(bounded_step(selected_index, delta, max));
                return true;
            }
        }
        Some(ModalState::AccessOverride) => {
            if matches!(key_name, "up" | "down") {
                if model.contacts.is_empty() {
                    return true;
                }
                let max = model.contacts.len().saturating_sub(1);
                let selected_index = model.selected_contact_index().unwrap_or_default();
                let delta = if key_name == "up" { -1 } else { 1 };
                model.set_selected_contact_index(bounded_step(selected_index, delta, max));
                return true;
            }
        }
        _ => {}
    }
    false
}

pub(super) fn adjust_threshold_wizard_input(model: &mut UiModel, guardian: bool, delta: i8) {
    match model.active_modal.as_mut() {
        Some(ActiveModal::GuardianSetup(state)) if guardian => {
            let current = parse_wizard_value(&state.threshold_input, state.threshold_k);
            let adjusted =
                (current as i16 + delta as i16).clamp(1, state.selected_count.max(1) as i16) as u8;
            state.threshold_input = adjusted.to_string();
        }
        Some(ActiveModal::MfaSetup(state)) if !guardian => {
            let current = parse_wizard_value(&state.threshold_input, state.threshold_k);
            let adjusted =
                (current as i16 + delta as i16).clamp(1, state.selected_count.max(1) as i16) as u8;
            state.threshold_input = adjusted.to_string();
        }
        _ => {}
    }
}

pub(super) fn parse_wizard_value(value: &str, fallback: u8) -> u8 {
    value.trim().parse::<u8>().unwrap_or(fallback.max(1))
}

pub(super) fn available_device_count(model: &UiModel) -> u8 {
    if model.has_secondary_device() {
        2
    } else {
        1
    }
}

pub(super) fn toggle_guardian_selection(model: &mut UiModel) {
    if model.contacts.is_empty() {
        return;
    }
    if let Some(ActiveModal::GuardianSetup(state)) = model.active_modal.as_mut() {
        let idx = state
            .focus_index
            .min(model.contacts.len().saturating_sub(1));
        if let Some(position) = state
            .selected_indices
            .iter()
            .position(|selected| *selected == idx)
        {
            state.selected_indices.remove(position);
        } else {
            state.selected_indices.push(idx);
            state.selected_indices.sort_unstable();
        }
    }
}

pub(super) fn toggle_mfa_selection(model: &mut UiModel) {
    let available = available_device_count(model) as usize;
    if available == 0 {
        return;
    }
    if let Some(ActiveModal::MfaSetup(state)) = model.active_modal.as_mut() {
        let idx = state.focus_index.min(available.saturating_sub(1));
        if let Some(position) = state
            .selected_indices
            .iter()
            .position(|selected| *selected == idx)
        {
            state.selected_indices.remove(position);
        } else {
            state.selected_indices.push(idx);
            state.selected_indices.sort_unstable();
        }
    }
}
