use super::set_toast;
use super::wizard::{
    can_open_guardian_setup_wizard, can_open_mfa_setup_wizard, open_add_device_wizard,
    open_guardian_setup_wizard, open_mfa_setup_wizard, open_remove_device_selection,
};
use crate::model::{ActiveModal, SettingsSection, TextModalState, UiModel};

pub(super) fn handle_settings_char(model: &mut UiModel, ch: char) {
    match ch {
        'e' if matches!(model.settings_section, SettingsSection::Profile) => {
            model.modal_hint = "Edit Nickname".to_string();
            model.active_modal = Some(ActiveModal::EditNickname(TextModalState {
                value: model.profile_nickname.clone(),
            }));
        }
        't' if matches!(model.settings_section, SettingsSection::GuardianThreshold) => {
            if can_open_guardian_setup_wizard(model) {
                open_guardian_setup_wizard(model);
            }
        }
        'a' if matches!(model.settings_section, SettingsSection::Devices) => {
            open_add_device_wizard(model);
        }
        'i' if matches!(model.settings_section, SettingsSection::Devices) => {
            model.modal_hint = "Import Device Enrollment Code".to_string();
            model.active_modal = Some(ActiveModal::ImportDeviceEnrollmentCode(
                TextModalState::default(),
            ));
        }
        'r' if matches!(model.settings_section, SettingsSection::Devices) => {
            if model.has_secondary_device {
                open_remove_device_selection(model);
            } else {
                set_toast(model, 'ℹ', "Cannot remove the current device");
            }
        }
        's' if matches!(model.settings_section, SettingsSection::RequestRecovery) => {
            model.modal_hint = "Request Recovery".to_string();
            model.active_modal = Some(ActiveModal::RequestRecovery);
        }
        's' if matches!(model.settings_section, SettingsSection::Authority) => {
            if model.authorities.len() <= 1 {
                set_toast(model, 'ℹ', "Only one authority available");
            } else {
                model.modal_hint = "Switch Authority".to_string();
                model.active_modal = Some(ActiveModal::SwitchAuthority);
            }
        }
        'm' if matches!(model.settings_section, SettingsSection::Authority) => {
            if can_open_mfa_setup_wizard(model) {
                open_mfa_setup_wizard(model);
            }
        }
        _ => {}
    }
}
