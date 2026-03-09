//! Structured TUI state export for harness observation.

use crate::tui::screens::Screen;
use crate::tui::state::modal_queue::QueuedModal;
use crate::tui::state::toast::ToastLevel;
use crate::tui::state::CreateInvitationField;
use crate::tui::types::{
    Contact as TuiContact, Device as TuiDevice, Message as TuiMessage, SettingsSection,
};
use crate::tui::TuiState;
use aura_app::ui::contract::{
    ConfirmationState, ControlId, ListId, ListItemSnapshot, ListSnapshot, MessageSnapshot, ModalId,
    OperationId, OperationInstanceId, OperationSnapshot, OperationState, ScreenId,
    SelectionSnapshot, ToastId, ToastKind, ToastSnapshot, UiReadiness, UiSnapshot,
};
use aura_app::ui::types::StateSnapshot;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const UI_STATE_FILE_ENV: &str = "AURA_TUI_UI_STATE_FILE";

static UI_STATE_FILE: OnceLock<Option<PathBuf>> = OnceLock::new();
static LAST_WRITTEN_SNAPSHOT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static CONTACTS_OVERRIDE: OnceLock<Mutex<Option<Vec<TuiContact>>>> = OnceLock::new();
static DEVICES_OVERRIDE: OnceLock<Mutex<Option<Vec<TuiDevice>>>> = OnceLock::new();
static MESSAGES_OVERRIDE: OnceLock<Mutex<Option<Vec<TuiMessage>>>> = OnceLock::new();

fn configured_ui_state_file() -> Option<&'static PathBuf> {
    UI_STATE_FILE
        .get_or_init(|| std::env::var_os(UI_STATE_FILE_ENV).map(PathBuf::from))
        .as_ref()
}

fn last_written_snapshot() -> &'static Mutex<Option<String>> {
    LAST_WRITTEN_SNAPSHOT.get_or_init(|| Mutex::new(None))
}

fn contacts_override() -> &'static Mutex<Option<Vec<TuiContact>>> {
    CONTACTS_OVERRIDE.get_or_init(|| Mutex::new(None))
}

fn devices_override() -> &'static Mutex<Option<Vec<TuiDevice>>> {
    DEVICES_OVERRIDE.get_or_init(|| Mutex::new(None))
}

fn messages_override() -> &'static Mutex<Option<Vec<TuiMessage>>> {
    MESSAGES_OVERRIDE.get_or_init(|| Mutex::new(None))
}

fn map_screen(screen: Screen) -> ScreenId {
    match screen {
        Screen::Neighborhood => ScreenId::Neighborhood,
        Screen::Chat => ScreenId::Chat,
        Screen::Contacts => ScreenId::Contacts,
        Screen::Notifications => ScreenId::Notifications,
        Screen::Settings => ScreenId::Settings,
    }
}

fn screen_item_id(screen: ScreenId) -> String {
    match screen {
        ScreenId::Onboarding => "onboarding",
        ScreenId::Neighborhood => "neighborhood",
        ScreenId::Chat => "chat",
        ScreenId::Contacts => "contacts",
        ScreenId::Notifications => "notifications",
        ScreenId::Settings => "settings",
    }
    .to_string()
}

fn map_modal(modal: &QueuedModal) -> Option<ModalId> {
    match modal {
        QueuedModal::Help { .. } => Some(ModalId::Help),
        QueuedModal::ChatCreate(_) => Some(ModalId::CreateChannel),
        QueuedModal::ChatTopic(_) => Some(ModalId::SetChannelTopic),
        QueuedModal::ChatInfo(_) => Some(ModalId::ChannelInfo),
        QueuedModal::ContactsNickname(_) => Some(ModalId::EditNickname),
        QueuedModal::ContactsImport(_) => Some(ModalId::AcceptInvitation),
        QueuedModal::ContactsCreate(_) => Some(ModalId::CreateInvitation),
        QueuedModal::ContactsCode(_) => Some(ModalId::InvitationCode),
        QueuedModal::GuardianSetup(_) => Some(ModalId::GuardianSetup),
        QueuedModal::MfaSetup(_) => Some(ModalId::MfaSetup),
        QueuedModal::SettingsNicknameSuggestion(_) => Some(ModalId::EditNickname),
        QueuedModal::SettingsAddDevice(_) => Some(ModalId::AddDevice),
        QueuedModal::SettingsDeviceImport(_) => Some(ModalId::ImportDeviceEnrollmentCode),
        QueuedModal::SettingsDeviceEnrollment(_) => Some(ModalId::AddDevice),
        QueuedModal::SettingsDeviceSelect(_) => Some(ModalId::SelectDeviceToRemove),
        QueuedModal::SettingsRemoveDevice(_) => Some(ModalId::ConfirmRemoveDevice),
        QueuedModal::AuthorityPicker(_) => Some(ModalId::SwitchAuthority),
        QueuedModal::NeighborhoodHomeCreate(_) => Some(ModalId::CreateHome),
        QueuedModal::NeighborhoodModeratorAssignment(_) => Some(ModalId::AssignModerator),
        QueuedModal::NeighborhoodAccessOverride(_) => Some(ModalId::AccessOverride),
        QueuedModal::NeighborhoodCapabilityConfig(_) => Some(ModalId::CapabilityConfig),
        QueuedModal::AccountSetup(_)
        | QueuedModal::Confirm { .. }
        | QueuedModal::GuardianSelect(_)
        | QueuedModal::ContactSelect(_)
        | QueuedModal::ChatMemberSelect(_) => None,
    }
}

fn map_toast_kind(level: ToastLevel) -> ToastKind {
    match level {
        ToastLevel::Success => ToastKind::Success,
        ToastLevel::Info | ToastLevel::Warning => ToastKind::Info,
        ToastLevel::Error => ToastKind::Error,
    }
}

fn push_list(
    lists: &mut Vec<ListSnapshot>,
    selections: &mut Vec<SelectionSnapshot>,
    list_id: ListId,
    items: Vec<ListItemSnapshot>,
    selected_id: Option<String>,
) {
    if items.is_empty() {
        return;
    }
    lists.push(ListSnapshot { id: list_id, items });
    if let Some(item_id) = selected_id {
        selections.push(SelectionSnapshot {
            list: list_id,
            item_id,
        });
    }
}

fn selected_by_index(items: &[String], index: usize) -> Option<String> {
    items.get(index).cloned()
}

fn write_snapshot_file(path: &Path, snapshot_json: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, snapshot_json)?;
    fs::rename(temp_path, path)?;
    Ok(())
}

fn cached_snapshot(path: &Path) -> Option<UiSnapshot> {
    let last_written = last_written_snapshot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(snapshot_json) = last_written.as_deref() {
        return serde_json::from_str(snapshot_json).ok();
    }
    let snapshot_json = fs::read_to_string(path).ok()?;
    serde_json::from_str(&snapshot_json).ok()
}

pub fn semantic_ui_snapshot(
    state: &TuiState,
    app_snapshot: &StateSnapshot,
    contacts_override_input: Option<&[TuiContact]>,
) -> UiSnapshot {
    let onboarding_active = matches!(
        state.modal_queue.current(),
        Some(QueuedModal::AccountSetup(_))
    );
    let screen = if onboarding_active {
        ScreenId::Onboarding
    } else {
        map_screen(state.screen())
    };
    let open_modal = state.modal_queue.current().and_then(map_modal);
    let readiness = if onboarding_active {
        UiReadiness::Loading
    } else {
        UiReadiness::Ready
    };

    let focused_control = if onboarding_active {
        Some(ControlId::OnboardingRoot)
    } else if let Some(QueuedModal::ContactsCreate(modal_state)) = state.modal_queue.current() {
        Some(ControlId::Field(match modal_state.focused_field {
            CreateInvitationField::Type => aura_app::ui::contract::FieldId::InvitationType,
            CreateInvitationField::Message => aura_app::ui::contract::FieldId::InvitationMessage,
            CreateInvitationField::Ttl => aura_app::ui::contract::FieldId::InvitationTtl,
        }))
    } else if let Some(modal_id) = open_modal {
        Some(ControlId::Modal(modal_id))
    } else {
        match state.screen() {
            Screen::Neighborhood => {
                if state.neighborhood.insert_mode {
                    Some(ControlId::Field(aura_app::ui::contract::FieldId::ChatInput))
                } else {
                    Some(ControlId::Screen(ScreenId::Neighborhood))
                }
            }
            Screen::Chat => {
                if state.chat.insert_mode {
                    Some(ControlId::Field(aura_app::ui::contract::FieldId::ChatInput))
                } else {
                    Some(ControlId::Screen(ScreenId::Chat))
                }
            }
            Screen::Contacts => Some(ControlId::Screen(ScreenId::Contacts)),
            Screen::Notifications => Some(ControlId::List(ListId::Notifications)),
            Screen::Settings => Some(ControlId::Screen(ScreenId::Settings)),
        }
    };

    let mut lists = Vec::new();
    let mut selections = Vec::new();

    let navigation_ids = Screen::all()
        .iter()
        .map(|candidate| {
            let id = map_screen(*candidate);
            ListItemSnapshot {
                id: screen_item_id(id),
                selected: *candidate == state.screen(),
                confirmation: ConfirmationState::Confirmed,
            }
        })
        .collect::<Vec<_>>();
    let selected_navigation_id = Some(screen_item_id(screen));
    push_list(
        &mut lists,
        &mut selections,
        ListId::Navigation,
        navigation_ids,
        selected_navigation_id,
    );

    let channel_ids = app_snapshot
        .chat
        .all_channels()
        .map(|channel| channel.id.to_string())
        .collect::<Vec<_>>();
    let channel_items = app_snapshot
        .chat
        .all_channels()
        .enumerate()
        .map(|(idx, channel)| ListItemSnapshot {
            id: channel.id.to_string(),
            selected: idx == state.chat.selected_channel,
            confirmation: ConfirmationState::Confirmed,
        })
        .collect::<Vec<_>>();
    push_list(
        &mut lists,
        &mut selections,
        ListId::Channels,
        channel_items,
        selected_by_index(&channel_ids, state.chat.selected_channel),
    );

    let effective_contacts = contacts_override_input
        .filter(|contacts| !contacts.is_empty())
        .map(|contacts| contacts.to_vec())
        .unwrap_or_else(|| {
            contacts_override()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
                .unwrap_or_default()
        });
    let contact_ids = if !effective_contacts.is_empty() {
        effective_contacts
            .iter()
            .map(|contact| contact.id.clone())
            .collect::<Vec<_>>()
    } else {
        app_snapshot
            .contacts
            .all_contacts()
            .map(|contact| contact.id.to_string())
            .collect::<Vec<_>>()
    };
    let contact_count = if !contact_ids.is_empty() {
        contact_ids.len()
    } else {
        state.contacts.contact_count
    };
    let contact_ids = if contact_ids.is_empty() && contact_count > 0 {
        (0..contact_count)
            .map(|idx| format!("contact-{idx}"))
            .collect::<Vec<_>>()
    } else {
        contact_ids
    };
    let contact_items = if !effective_contacts.is_empty() {
        effective_contacts
            .iter()
            .enumerate()
            .map(|(idx, contact)| ListItemSnapshot {
                id: contact.id.clone(),
                selected: idx == state.contacts.selected_index,
                confirmation: ConfirmationState::Confirmed,
            })
            .collect::<Vec<_>>()
    } else {
        app_snapshot
            .contacts
            .all_contacts()
            .enumerate()
            .map(|(idx, contact)| ListItemSnapshot {
                id: contact.id.to_string(),
                selected: idx == state.contacts.selected_index,
                confirmation: ConfirmationState::Confirmed,
            })
            .collect::<Vec<_>>()
    };
    let contact_items = if contact_items.is_empty() && contact_count > 0 {
        contact_ids
            .iter()
            .enumerate()
            .map(|(idx, id)| ListItemSnapshot {
                id: id.clone(),
                selected: idx == state.contacts.selected_index,
                confirmation: ConfirmationState::Confirmed,
            })
            .collect::<Vec<_>>()
    } else {
        contact_items
    };
    push_list(
        &mut lists,
        &mut selections,
        ListId::Contacts,
        contact_items,
        selected_by_index(&contact_ids, state.contacts.selected_index),
    );

    let notification_ids = app_snapshot
        .invitations
        .all_pending()
        .iter()
        .chain(app_snapshot.invitations.all_sent().iter())
        .chain(app_snapshot.invitations.all_history().iter())
        .map(|invitation| invitation.id.clone())
        .collect::<Vec<_>>();
    let notification_items = notification_ids
        .iter()
        .enumerate()
        .map(|(idx, id)| ListItemSnapshot {
            id: id.clone(),
            selected: idx == state.notifications.selected_index,
            confirmation: ConfirmationState::Confirmed,
        })
        .collect::<Vec<_>>();
    push_list(
        &mut lists,
        &mut selections,
        ListId::Notifications,
        notification_items,
        selected_by_index(&notification_ids, state.notifications.selected_index),
    );

    if let Some(QueuedModal::ContactsCreate(modal_state)) = state.modal_queue.current() {
        let invitation_type_ids = vec![
            "guardian".to_string(),
            "contact".to_string(),
            "channel".to_string(),
        ];
        let invitation_type_items = invitation_type_ids
            .iter()
            .enumerate()
            .map(|(idx, id)| ListItemSnapshot {
                id: id.clone(),
                selected: idx == modal_state.type_index,
                confirmation: ConfirmationState::Confirmed,
            })
            .collect::<Vec<_>>();
        push_list(
            &mut lists,
            &mut selections,
            ListId::InvitationTypes,
            invitation_type_items,
            selected_by_index(&invitation_type_ids, modal_state.type_index),
        );
    }

    let mut home_ids = vec![app_snapshot.neighborhood.home_home_id.to_string()];
    home_ids.extend(
        app_snapshot
            .neighborhood
            .all_neighbors()
            .map(|home| home.id.to_string()),
    );
    let home_items = home_ids
        .iter()
        .enumerate()
        .map(|(idx, id)| ListItemSnapshot {
            id: id.clone(),
            selected: idx == state.neighborhood.selected_home,
            confirmation: ConfirmationState::Confirmed,
        })
        .collect::<Vec<_>>();
    push_list(
        &mut lists,
        &mut selections,
        ListId::Homes,
        home_items,
        selected_by_index(&home_ids, state.neighborhood.selected_home),
    );

    let member_ids = app_snapshot
        .homes
        .current_home()
        .map(|home| {
            home.members
                .iter()
                .map(|member| member.id.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let member_items = member_ids
        .iter()
        .enumerate()
        .map(|(idx, id)| ListItemSnapshot {
            id: id.clone(),
            selected: idx == state.neighborhood.selected_member,
            confirmation: ConfirmationState::Confirmed,
        })
        .collect::<Vec<_>>();
    push_list(
        &mut lists,
        &mut selections,
        ListId::NeighborhoodMembers,
        member_items,
        selected_by_index(&member_ids, state.neighborhood.selected_member),
    );

    let authority_ids = state
        .authorities
        .iter()
        .map(|authority| authority.id.clone())
        .collect::<Vec<_>>();
    let authority_items = state
        .authorities
        .iter()
        .enumerate()
        .map(|(idx, authority)| ListItemSnapshot {
            id: authority.id.clone(),
            selected: idx == state.current_authority_index,
            confirmation: ConfirmationState::Confirmed,
        })
        .collect::<Vec<_>>();
    push_list(
        &mut lists,
        &mut selections,
        ListId::Authorities,
        authority_items,
        selected_by_index(&authority_ids, state.current_authority_index),
    );

    let settings_section_ids = SettingsSection::all()
        .iter()
        .map(|section| section.title().to_ascii_lowercase().replace(' ', "_"))
        .collect::<Vec<_>>();
    let settings_items = SettingsSection::all()
        .iter()
        .map(|section| ListItemSnapshot {
            id: section.title().to_ascii_lowercase().replace(' ', "_"),
            selected: *section == state.settings.section,
            confirmation: ConfirmationState::Confirmed,
        })
        .collect::<Vec<_>>();
    push_list(
        &mut lists,
        &mut selections,
        ListId::SettingsSections,
        settings_items,
        settings_section_ids
            .get(state.settings.section.index())
            .cloned(),
    );

    let effective_devices = devices_override()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .unwrap_or_default();
    if state.settings.section == SettingsSection::Devices {
        let device_items = effective_devices
            .iter()
            .map(|device| ListItemSnapshot {
                id: device.id.clone(),
                selected: false,
                confirmation: ConfirmationState::Confirmed,
            })
            .collect::<Vec<_>>();
        push_list(
            &mut lists,
            &mut selections,
            ListId::Devices,
            device_items,
            None,
        );
    }

    let toasts = state
        .toast_queue
        .current()
        .map(|toast| {
            vec![ToastSnapshot {
                id: ToastId(format!("toast-{}", toast.id)),
                kind: map_toast_kind(toast.level),
                message: toast.message.clone(),
            }]
        })
        .unwrap_or_default();

    let effective_messages = messages_override()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .unwrap_or_default();
    let selected_channel_id = channel_ids.get(state.chat.selected_channel).cloned();
    let messages = if !effective_messages.is_empty() {
        effective_messages
            .iter()
            .map(|message| MessageSnapshot {
                id: message.id.clone(),
                content: message.content.clone(),
            })
            .collect::<Vec<_>>()
    } else {
        selected_channel_id
            .as_ref()
            .and_then(|channel_id| {
                app_snapshot
                    .chat
                    .all_channels()
                    .find(|channel| channel.id.to_string() == *channel_id)
                    .map(|channel| {
                        app_snapshot
                            .chat
                            .messages_for_channel(&channel.id)
                            .iter()
                            .map(|message| MessageSnapshot {
                                id: message.id.clone(),
                                content: message.content.clone(),
                            })
                            .collect::<Vec<_>>()
                    })
            })
            .unwrap_or_default()
    };

    let mut operations = state.exported_operation_snapshots();
    if matches!(state.modal_queue.current(), Some(QueuedModal::ContactsCode(_))) {
        operations.retain(|operation| operation.id != OperationId::invitation_create());
        operations.push(OperationSnapshot {
            id: OperationId::invitation_create(),
            instance_id: OperationInstanceId("tui-invitation-create".to_string()),
            state: OperationState::Succeeded,
        });
    }
    if let Some(QueuedModal::SettingsDeviceEnrollment(modal_state)) = state.modal_queue.current() {
        let operation_state = if modal_state.ceremony.has_failed {
            OperationState::Failed
        } else if modal_state.ceremony.is_complete {
            OperationState::Succeeded
        } else {
            OperationState::Submitting
        };
        operations.retain(|operation| operation.id != OperationId::device_enrollment());
        operations.push(OperationSnapshot {
            id: OperationId::device_enrollment(),
            instance_id: OperationInstanceId("tui-device-enrollment".to_string()),
            state: operation_state,
        });
    }

    UiSnapshot {
        screen,
        focused_control,
        open_modal,
        readiness,
        selections,
        lists,
        messages,
        operations,
        toasts,
        runtime_events: Vec::new(),
    }
}

pub fn maybe_export_ui_snapshot(
    state: &TuiState,
    app_snapshot: &StateSnapshot,
    contacts_override: Option<&[TuiContact]>,
) {
    let Some(path) = configured_ui_state_file() else {
        return;
    };

    let snapshot = semantic_ui_snapshot(state, app_snapshot, contacts_override);
    let Ok(snapshot_json) = serde_json::to_string_pretty(&snapshot) else {
        return;
    };

    let mut last_written = last_written_snapshot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if last_written.as_deref() == Some(snapshot_json.as_str()) {
        return;
    }

    if write_snapshot_file(path, &snapshot_json).is_ok() {
        *last_written = Some(snapshot_json);
    }
}

pub fn publish_contacts_list_override(contacts: &[TuiContact], selected_index: usize) {
    let Some(path) = configured_ui_state_file() else {
        return;
    };
    *contacts_override()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(contacts.to_vec());
    let Some(mut snapshot) = cached_snapshot(path) else {
        return;
    };

    let items = contacts
        .iter()
        .enumerate()
        .map(|(idx, contact)| ListItemSnapshot {
            id: contact.id.clone(),
            selected: idx == selected_index,
            confirmation: ConfirmationState::Confirmed,
        })
        .collect::<Vec<_>>();

    let selected_id = contacts
        .get(selected_index)
        .map(|contact| contact.id.clone());
    snapshot.lists.retain(|list| list.id != ListId::Contacts);
    if !items.is_empty() {
        snapshot.lists.push(ListSnapshot {
            id: ListId::Contacts,
            items,
        });
    }
    snapshot
        .selections
        .retain(|selection| selection.list != ListId::Contacts);
    if let Some(item_id) = selected_id {
        snapshot.selections.push(SelectionSnapshot {
            list: ListId::Contacts,
            item_id,
        });
    }

    let Ok(snapshot_json) = serde_json::to_string_pretty(&snapshot) else {
        return;
    };

    let mut last_written = last_written_snapshot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if write_snapshot_file(path, &snapshot_json).is_ok() {
        *last_written = Some(snapshot_json);
    }
}

pub fn publish_devices_list_override(devices: &[TuiDevice]) {
    let Some(path) = configured_ui_state_file() else {
        return;
    };
    *devices_override()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(devices.to_vec());
    let Some(mut snapshot) = cached_snapshot(path) else {
        return;
    };

    snapshot.lists.retain(|list| list.id != ListId::Devices);
    if snapshot
        .selections
        .iter()
        .any(|selection| selection.list == ListId::SettingsSections && selection.item_id == "devices")
    {
        let items = devices
            .iter()
            .map(|device| ListItemSnapshot {
                id: device.id.clone(),
                selected: false,
                confirmation: ConfirmationState::Confirmed,
            })
            .collect::<Vec<_>>();
        if !items.is_empty() {
            snapshot.lists.push(ListSnapshot {
                id: ListId::Devices,
                items,
            });
        }
    }

    let Ok(snapshot_json) = serde_json::to_string_pretty(&snapshot) else {
        return;
    };

    let mut last_written = last_written_snapshot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if write_snapshot_file(path, &snapshot_json).is_ok() {
        *last_written = Some(snapshot_json);
    }
}

pub fn publish_messages_override(messages: &[TuiMessage]) {
    let Some(path) = configured_ui_state_file() else {
        return;
    };
    *messages_override()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(messages.to_vec());
    let Some(mut snapshot) = cached_snapshot(path) else {
        return;
    };

    if snapshot.screen == ScreenId::Chat {
        snapshot.messages = messages
            .iter()
            .map(|message| MessageSnapshot {
                id: message.id.clone(),
                content: message.content.clone(),
            })
            .collect::<Vec<_>>();
    }

    let Ok(snapshot_json) = serde_json::to_string_pretty(&snapshot) else {
        return;
    };

    let mut last_written = last_written_snapshot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if write_snapshot_file(path, &snapshot_json).is_ok() {
        *last_written = Some(snapshot_json);
    }
}

#[cfg(test)]
mod tests {
    use super::semantic_ui_snapshot;
    use crate::tui::screens::Screen;
    use crate::tui::state::modal_queue::QueuedModal;
    use crate::tui::state::views::{AccountSetupModalState, DeviceEnrollmentCeremonyModalState};
    use crate::tui::TuiState;
    use aura_app::ui::contract::{ControlId, ListId, OperationId, OperationState, UiReadiness};
    use aura_app::ui::types::StateSnapshot;

    #[test]
    fn account_setup_maps_to_onboarding_state() {
        let mut state = TuiState::new();
        state.show_modal(QueuedModal::AccountSetup(AccountSetupModalState::default()));

        let snapshot = semantic_ui_snapshot(&state, &StateSnapshot::default(), None);
        assert_eq!(snapshot.readiness, UiReadiness::Loading);
        assert_eq!(snapshot.focused_control, Some(ControlId::OnboardingRoot));
        assert_eq!(snapshot.open_modal, None);
    }

    #[test]
    fn navigation_list_marks_current_screen() {
        let mut state = TuiState::new();
        state.router.go_to(Screen::Contacts);

        let snapshot = semantic_ui_snapshot(&state, &StateSnapshot::default(), None);
        let nav = snapshot
            .lists
            .iter()
            .find(|list| list.id == ListId::Navigation)
            .unwrap_or_else(|| panic!("navigation list should exist"));
        assert!(nav.items.iter().any(|item| item.selected));
    }

    #[test]
    fn device_enrollment_modal_exports_operation_state() {
        let mut state = TuiState::new();
        let mut modal = DeviceEnrollmentCeremonyModalState::started(
            "ceremony-1".to_string(),
            "Mobile".to_string(),
            "code-123".to_string(),
        );
        modal.update_from_status(
            1,
            2,
            2,
            false,
            false,
            None,
            None,
            aura_core::threshold::AgreementMode::CoordinatorSoftSafe,
            false,
        );
        state.show_modal(QueuedModal::SettingsDeviceEnrollment(modal));

        let snapshot = semantic_ui_snapshot(&state, &StateSnapshot::default(), None);
        let operation_state = snapshot
            .operations
            .iter()
            .find(|operation| operation.id == OperationId::device_enrollment())
            .map(|operation| operation.state);

        assert_eq!(operation_state, Some(OperationState::Submitting));
    }
}
