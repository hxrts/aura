use super::*;
use crate::tui::state::views::AccountSetupField;
use aura_app::ui::signals::{DiscoveredPeer, DiscoveredPeerMethod};

pub(super) fn build_global_modals(
    current_screen: Screen,
    tui_snapshot: &TuiState,
    discovered_peers: &[DiscoveredPeer],
) -> GlobalModalProps {
    let mut global_modals = GlobalModalProps::default();
    global_modals.help.current_screen_name = current_screen.name().to_string();

    if let Some(modal) = tui_snapshot.modal_queue.current() {
        match modal {
            QueuedModal::AccountSetup(state) => {
                global_modals.account_setup.visible = true;
                global_modals.account_setup.nickname_suggestion = state.nickname_suggestion.clone();
                global_modals.account_setup.device_import_code = state.device_import_code.clone();
                global_modals.account_setup.name_focused =
                    matches!(state.active_field, AccountSetupField::AccountName);
                global_modals.account_setup.import_code_focused =
                    matches!(state.active_field, AccountSetupField::DeviceImportCode);
                global_modals.account_setup.creating = state.creating;
                global_modals.account_setup.show_spinner = state.should_show_spinner();
                global_modals.account_setup.success = state.success;
                global_modals.account_setup.error = state.error.clone();
                global_modals.account_setup.bootstrap_candidates = discovered_peers
                    .iter()
                    .filter(|peer| peer.method == DiscoveredPeerMethod::BootstrapCandidate)
                    .map(|peer| {
                        if peer.address.is_empty() {
                            peer.authority_id.to_string()
                        } else {
                            format!("{} ({})", peer.address, peer.authority_id)
                        }
                    })
                    .collect();
            }
            QueuedModal::GuardianSelect(state) => {
                global_modals.guardian_picker.visible = true;
                global_modals.guardian_picker.title = state.title.clone();
                global_modals.guardian_picker.contacts = state
                    .contacts
                    .iter()
                    .map(|(id, name)| Contact::new(id.to_string(), name.clone()))
                    .collect();
                global_modals.guardian_picker.selected_index = state.selected_index;
                global_modals.guardian_picker.selected_ids =
                    state.selected_ids.iter().map(ToString::to_string).collect();
                global_modals.guardian_picker.multi_select = state.multi_select;
            }
            QueuedModal::ContactSelect(state) => {
                global_modals.contact_picker.visible = true;
                global_modals.contact_picker.title = state.title.clone();
                global_modals.contact_picker.contacts = state
                    .contacts
                    .iter()
                    .map(|(id, name)| Contact::new(id.to_string(), name.clone()))
                    .collect();
                global_modals.contact_picker.selected_index = state.selected_index;
                global_modals.contact_picker.selected_ids =
                    state.selected_ids.iter().map(ToString::to_string).collect();
                global_modals.contact_picker.multi_select = state.multi_select;
            }
            QueuedModal::ChatMemberSelect(state) => {
                global_modals.contact_picker.visible = true;
                global_modals.contact_picker.title = state.picker.title.clone();
                global_modals.contact_picker.contacts = state
                    .picker
                    .contacts
                    .iter()
                    .map(|(id, name)| Contact::new(id.to_string(), name.clone()))
                    .collect();
                global_modals.contact_picker.selected_index = state.picker.selected_index;
                global_modals.contact_picker.selected_ids = state
                    .picker
                    .selected_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect();
                global_modals.contact_picker.multi_select = state.picker.multi_select;
            }
            QueuedModal::Confirm {
                title,
                message,
                on_confirm: _,
            } => {
                global_modals.confirm.visible = true;
                global_modals.confirm.title = title.clone();
                global_modals.confirm.message = message.clone();
            }
            QueuedModal::Help { current_screen } => {
                global_modals.help.visible = true;
                if let Some(help_screen) = current_screen {
                    global_modals.help.current_screen_name = help_screen.name().to_string();
                }
            }
            _ => {}
        }
    }

    global_modals
}
