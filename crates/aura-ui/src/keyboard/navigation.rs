use crate::model::{ScreenId, SettingsSection, UiModel};

pub(super) fn bounded_step(current: usize, delta: i32, max: usize) -> usize {
    if delta.is_negative() {
        current.saturating_sub(delta.unsigned_abs() as usize)
    } else {
        current.saturating_add(delta as usize).min(max)
    }
}

pub(super) fn cycle_screen(model: &mut UiModel) {
    let next = match model.screen {
        ScreenId::Onboarding => ScreenId::Neighborhood,
        ScreenId::Neighborhood => ScreenId::Chat,
        ScreenId::Chat => ScreenId::Contacts,
        ScreenId::Contacts => ScreenId::Notifications,
        ScreenId::Notifications => ScreenId::Settings,
        ScreenId::Settings => ScreenId::Neighborhood,
    };
    model.set_screen(next);
}

pub(super) fn cycle_screen_prev(model: &mut UiModel) {
    let next = match model.screen {
        ScreenId::Onboarding => ScreenId::Settings,
        ScreenId::Neighborhood => ScreenId::Settings,
        ScreenId::Chat => ScreenId::Neighborhood,
        ScreenId::Contacts => ScreenId::Chat,
        ScreenId::Notifications => ScreenId::Contacts,
        ScreenId::Settings => ScreenId::Notifications,
    };
    model.set_screen(next);
}

pub(super) fn move_selection(model: &mut UiModel, delta: i32) {
    match model.screen {
        ScreenId::Onboarding => {}
        ScreenId::Settings => {
            let max = SettingsSection::ALL.len().saturating_sub(1);
            let next = bounded_step(model.settings_section.index(), delta, max);
            model.settings_section = SettingsSection::from_index(next);
        }
        ScreenId::Contacts => {
            if model.contacts.is_empty() {
                return;
            }
            let max = model.contacts.len() as i32 - 1;
            let current = model.selected_contact_index().unwrap_or_default();
            model.set_selected_contact_index(bounded_step(current, delta, max as usize));
        }
        ScreenId::Chat => {
            model.move_channel_selection(delta);
        }
        ScreenId::Notifications => {
            if model.notifications.is_empty() {
                model.selected_notification_id = None;
                return;
            }
            let max = model.notifications.len() as i32 - 1;
            let current = model.selected_notification_index().unwrap_or_default();
            model.set_selected_notification_index(
                bounded_step(current, delta, max as usize),
                model.notifications.len(),
            );
        }
        ScreenId::Neighborhood => {}
    }
}

pub(super) fn handle_horizontal(model: &mut UiModel, _delta: i32) {
    if model.screen == ScreenId::Contacts {
        model.contact_details = !model.contact_details;
    }
}
