use super::*;
use crate::model::AddDeviceModalState;

#[derive(Clone)]
pub(super) struct ShellFooterState {
    pub(super) network_status: String,
    pub(super) peer_count: String,
    pub(super) online_count: String,
}

#[derive(Clone)]
pub(super) struct ShellRuntimeSnapshots {
    pub(super) neighborhood: NeighborhoodRuntimeView,
    pub(super) chat: ChatRuntimeView,
    pub(super) contacts: ContactsRuntimeView,
    pub(super) settings: SettingsRuntimeView,
    pub(super) notifications: NotificationsRuntimeView,
}

impl ShellRuntimeSnapshots {
    pub(super) fn from_signals(
        neighborhood_runtime: Signal<NeighborhoodRuntimeView>,
        chat_runtime: Signal<ChatRuntimeView>,
        contacts_runtime: Signal<ContactsRuntimeView>,
        settings_runtime: Signal<SettingsRuntimeView>,
        notifications_runtime: Signal<NotificationsRuntimeView>,
    ) -> Self {
        Self {
            neighborhood: neighborhood_runtime(),
            chat: chat_runtime(),
            contacts: contacts_runtime(),
            settings: settings_runtime(),
            notifications: notifications_runtime(),
        }
    }

    pub(super) fn footer(&self) -> ShellFooterState {
        if self.neighborhood.loaded {
            ShellFooterState {
                network_status: self.neighborhood.network_status.clone(),
                peer_count: self.neighborhood.reachable_peers.to_string(),
                online_count: self.neighborhood.online_contacts.to_string(),
            }
        } else {
            ShellFooterState {
                network_status: format_network_status_with_severity(
                    &NetworkStatus::Disconnected,
                    None,
                )
                .0,
                peer_count: "0".to_string(),
                online_count: "0".to_string(),
            }
        }
    }

    pub(super) fn publish_semantic_snapshot(&self, controller: &UiController, model: &UiModel) {
        controller.publish_ui_snapshot(runtime_semantic_snapshot(
            model,
            &self.neighborhood,
            &self.chat,
            &self.contacts,
            &self.settings,
            &self.notifications,
        ));
    }
}

#[derive(Clone)]
pub(super) struct ShellRenderState {
    pub(super) runtime: ShellRuntimeSnapshots,
    pub(super) footer: ShellFooterState,
    pub(super) modal: Option<ModalView>,
    pub(super) modal_state: Option<ModalState>,
    pub(super) add_device_modal_state: Option<AddDeviceModalState>,
    pub(super) cancel_add_device_ceremony_id: Option<CeremonyId>,
    pub(super) selected_member_key: Option<NeighborhoodMemberSelectionKey>,
    pub(super) should_exit_insert_mode: bool,
}

impl ShellRenderState {
    pub(super) fn from_runtime(
        model: &UiModel,
        neighborhood_runtime: Signal<NeighborhoodRuntimeView>,
        chat_runtime: Signal<ChatRuntimeView>,
        contacts_runtime: Signal<ContactsRuntimeView>,
        settings_runtime: Signal<SettingsRuntimeView>,
        notifications_runtime: Signal<NotificationsRuntimeView>,
    ) -> Self {
        let runtime = ShellRuntimeSnapshots::from_signals(
            neighborhood_runtime,
            chat_runtime,
            contacts_runtime,
            settings_runtime,
            notifications_runtime,
        );
        let add_device_modal_state = model.add_device_modal().cloned();
        Self {
            footer: runtime.footer(),
            modal: modal_view(model, &runtime.chat),
            modal_state: model.modal_state(),
            cancel_add_device_ceremony_id: add_device_modal_state
                .as_ref()
                .and_then(|state| state.ceremony_id.clone()),
            selected_member_key: model.selected_neighborhood_member_key.clone(),
            should_exit_insert_mode: matches!(model.screen, ScreenId::Chat) && model.input_mode,
            runtime,
            add_device_modal_state,
        }
    }
}
