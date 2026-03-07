//! Structured TUI state export for harness observation.

use crate::tui::screens::Screen;
use crate::tui::state::modal_queue::QueuedModal;
use crate::tui::state::toast::ToastLevel;
use crate::tui::types::SettingsSection;
use crate::tui::TuiState;
use aura_app::ui::contract::{
    ConfirmationState, ControlId, ListId, ListItemSnapshot, ListSnapshot, ModalId, OperationId,
    OperationSnapshot, OperationState, ScreenId, SelectionSnapshot, ToastId, ToastKind,
    ToastSnapshot, UiReadiness, UiSnapshot,
};
use aura_app::ui::types::StateSnapshot;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const UI_STATE_FILE_ENV: &str = "AURA_TUI_UI_STATE_FILE";

static UI_STATE_FILE: OnceLock<Option<PathBuf>> = OnceLock::new();
static LAST_WRITTEN_SNAPSHOT: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn configured_ui_state_file() -> Option<&'static PathBuf> {
    UI_STATE_FILE
        .get_or_init(|| std::env::var_os(UI_STATE_FILE_ENV).map(PathBuf::from))
        .as_ref()
}

fn last_written_snapshot() -> &'static Mutex<Option<String>> {
    LAST_WRITTEN_SNAPSHOT.get_or_init(|| Mutex::new(None))
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
        | QueuedModal::ChatMemberSelect(_)
        | QueuedModal::ContactsCode(_) => None,
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

pub fn semantic_ui_snapshot(state: &TuiState, app_snapshot: &StateSnapshot) -> UiSnapshot {
    let screen = map_screen(state.screen());
    let open_modal = state.modal_queue.current().and_then(map_modal);
    let readiness = if matches!(state.modal_queue.current(), Some(QueuedModal::AccountSetup(_))) {
        UiReadiness::Loading
    } else {
        UiReadiness::Ready
    };

    let focused_control = if matches!(state.modal_queue.current(), Some(QueuedModal::AccountSetup(_)))
    {
        Some(ControlId::OnboardingRoot)
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

    let contact_ids = app_snapshot
        .contacts
        .all_contacts()
        .map(|contact| contact.id.to_string())
        .collect::<Vec<_>>();
    let contact_items = app_snapshot
        .contacts
        .all_contacts()
        .enumerate()
        .map(|(idx, contact)| ListItemSnapshot {
            id: contact.id.to_string(),
            selected: idx == state.contacts.selected_index,
            confirmation: ConfirmationState::Confirmed,
        })
        .collect::<Vec<_>>();
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
        .enumerate()
        .map(|(idx, section)| ListItemSnapshot {
            id: section.title().to_ascii_lowercase().replace(' ', "_"),
            selected: idx == state.settings.selected_index,
            confirmation: ConfirmationState::Confirmed,
        })
        .collect::<Vec<_>>();
    push_list(
        &mut lists,
        &mut selections,
        ListId::SettingsSections,
        settings_items,
        selected_by_index(&settings_section_ids, state.settings.selected_index),
    );

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

    let operations = match state.modal_queue.current() {
        Some(QueuedModal::SettingsDeviceEnrollment(modal_state)) => {
            let operation_state = if modal_state.ceremony.has_failed {
                OperationState::Failed
            } else if modal_state.ceremony.is_complete {
                OperationState::Succeeded
            } else {
                OperationState::Submitting
            };
            vec![OperationSnapshot {
                id: OperationId("device_enrollment".to_string()),
                state: operation_state,
            }]
        }
        _ => Vec::new(),
    };

    UiSnapshot {
        screen,
        focused_control,
        open_modal,
        readiness,
        selections,
        lists,
        operations,
        toasts,
    }
}

pub fn maybe_export_ui_snapshot(state: &TuiState, app_snapshot: &StateSnapshot) {
    let Some(path) = configured_ui_state_file() else {
        return;
    };

    let snapshot = semantic_ui_snapshot(state, app_snapshot);
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

#[cfg(test)]
mod tests {
    use super::semantic_ui_snapshot;
    use crate::tui::screens::Screen;
    use crate::tui::state::modal_queue::QueuedModal;
    use crate::tui::state::views::AccountSetupModalState;
    use crate::tui::TuiState;
    use aura_app::ui::contract::{ControlId, ListId, UiReadiness};
    use aura_app::ui::types::StateSnapshot;

    #[test]
    fn account_setup_maps_to_onboarding_state() {
        let mut state = TuiState::new();
        state.show_modal(QueuedModal::AccountSetup(AccountSetupModalState::default()));

        let snapshot = semantic_ui_snapshot(&state, &StateSnapshot::default());
        assert_eq!(snapshot.readiness, UiReadiness::Loading);
        assert_eq!(snapshot.focused_control, Some(ControlId::OnboardingRoot));
        assert_eq!(snapshot.open_modal, None);
    }

    #[test]
    fn navigation_list_marks_current_screen() {
        let mut state = TuiState::new();
        state.router.go_to(Screen::Contacts);

        let snapshot = semantic_ui_snapshot(&state, &StateSnapshot::default());
        let nav = snapshot
            .lists
            .iter()
            .find(|list| list.id == ListId::Navigation)
            .expect("navigation list should exist");
        assert!(nav.items.iter().any(|item| item.selected));
    }
}
