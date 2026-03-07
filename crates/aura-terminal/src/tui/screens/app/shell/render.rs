use super::*;

pub(super) fn build_global_modals(
    current_screen: Screen,
    tui_snapshot: &TuiState,
) -> GlobalModalProps {
    let mut global_modals = GlobalModalProps::default();
    global_modals.current_screen_name = current_screen.name().to_string();

    if let Some(modal) = tui_snapshot.modal_queue.current() {
        match modal {
            QueuedModal::AccountSetup(state) => {
                global_modals.account_setup_visible = true;
                global_modals.account_setup_nickname_suggestion = state.nickname_suggestion.clone();
                global_modals.account_setup_creating = state.creating;
                global_modals.account_setup_show_spinner = state.should_show_spinner();
                global_modals.account_setup_success = state.success;
                global_modals.account_setup_error = state.error.clone();
            }
            QueuedModal::GuardianSelect(state) => {
                global_modals.guardian_modal_visible = true;
                global_modals.guardian_modal_title = state.title.clone();
                global_modals.guardian_modal_contacts = state
                    .contacts
                    .iter()
                    .map(|(id, name)| Contact::new(id.to_string(), name.clone()))
                    .collect();
                global_modals.guardian_modal_selected = state.selected_index;
                global_modals.guardian_modal_selected_ids =
                    state.selected_ids.iter().map(ToString::to_string).collect();
                global_modals.guardian_modal_multi_select = state.multi_select;
            }
            QueuedModal::ContactSelect(state) => {
                global_modals.contact_modal_visible = true;
                global_modals.contact_modal_title = state.title.clone();
                global_modals.contact_modal_contacts = state
                    .contacts
                    .iter()
                    .map(|(id, name)| Contact::new(id.to_string(), name.clone()))
                    .collect();
                global_modals.contact_modal_selected = state.selected_index;
                global_modals.contact_modal_selected_ids =
                    state.selected_ids.iter().map(ToString::to_string).collect();
                global_modals.contact_modal_multi_select = state.multi_select;
            }
            QueuedModal::ChatMemberSelect(state) => {
                global_modals.contact_modal_visible = true;
                global_modals.contact_modal_title = state.picker.title.clone();
                global_modals.contact_modal_contacts = state
                    .picker
                    .contacts
                    .iter()
                    .map(|(id, name)| Contact::new(id.to_string(), name.clone()))
                    .collect();
                global_modals.contact_modal_selected = state.picker.selected_index;
                global_modals.contact_modal_selected_ids = state
                    .picker
                    .selected_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect();
                global_modals.contact_modal_multi_select = state.picker.multi_select;
            }
            QueuedModal::Confirm {
                title,
                message,
                on_confirm: _,
            } => {
                global_modals.confirm_visible = true;
                global_modals.confirm_title = title.clone();
                global_modals.confirm_message = message.clone();
            }
            QueuedModal::Help { current_screen } => {
                global_modals.help_modal_visible = true;
                if let Some(help_screen) = current_screen {
                    global_modals.current_screen_name = help_screen.name().to_string();
                }
            }
            _ => {}
        }
    }

    global_modals
}

pub(super) fn state_indicator_label(tui_snapshot: &TuiState) -> String {
    let pending_actions = usize::from(tui_snapshot.modal_queue.is_active())
        + tui_snapshot.modal_queue.pending_count()
        + usize::from(tui_snapshot.toast_queue.is_active())
        + tui_snapshot.toast_queue.pending_count();
    let depth_label = match tui_snapshot.neighborhood.enter_depth {
        AccessLevel::Limited => "Lim",
        AccessLevel::Partial => "Par",
        AccessLevel::Full => "Full",
    };
    let moderator_label = if tui_snapshot.neighborhood.moderator_actions_enabled {
        "On"
    } else {
        "Off"
    };
    format!("D:{depth_label} M:{moderator_label} P:{pending_actions}")
}
