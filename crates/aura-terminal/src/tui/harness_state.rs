//! Structured TUI state export for harness observation.

use crate::tui::screens::Screen;
use crate::tui::state::modal_queue::QueuedModal;
use crate::tui::state::toast::ToastLevel;
use crate::tui::types::{
    Channel as TuiChannel, Contact as TuiContact, Device as TuiDevice, Message as TuiMessage,
    SettingsSection,
};
use crate::tui::TuiState;
use aura_app::ui::contract::{
    ConfirmationState, ControlId, ListId, ListItemSnapshot, ListSnapshot, MessageSnapshot, ModalId,
    ScreenId, SelectionSnapshot, ToastId, ToastKind, ToastSnapshot, UiReadiness, UiSnapshot,
};
use aura_app::ui::types::StateSnapshot;
use aura_app::ui_contract::{next_projection_revision, QuiescenceSnapshot};
use std::fs;
use std::io;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const UI_STATE_FILE_ENV: &str = "AURA_TUI_UI_STATE_FILE";
const UI_STATE_SOCKET_ENV: &str = "AURA_TUI_UI_STATE_SOCKET";

static UI_STATE_FILE: OnceLock<Option<PathBuf>> = OnceLock::new();
static UI_STATE_SOCKET: OnceLock<Option<PathBuf>> = OnceLock::new();
static LAST_WRITTEN_SNAPSHOT: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn configured_ui_state_file() -> Option<&'static PathBuf> {
    UI_STATE_FILE
        .get_or_init(|| std::env::var_os(UI_STATE_FILE_ENV).map(PathBuf::from))
        .as_ref()
}

fn configured_ui_state_socket() -> Option<&'static PathBuf> {
    UI_STATE_SOCKET
        .get_or_init(|| std::env::var_os(UI_STATE_SOCKET_ENV).map(PathBuf::from))
        .as_ref()
}

fn last_written_snapshot() -> &'static Mutex<Option<String>> {
    LAST_WRITTEN_SNAPSHOT.get_or_init(|| Mutex::new(None))
}

pub struct TuiSemanticInputs<'a> {
    pub app_snapshot: &'a StateSnapshot,
    pub contacts: &'a [TuiContact],
    pub settings_devices: &'a [TuiDevice],
    pub chat_channels: &'a [TuiChannel],
    pub chat_messages: &'a [TuiMessage],
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

pub fn authoritative_ui_snapshot(
    state: &TuiState,
    semantic_inputs: TuiSemanticInputs<'_>,
) -> UiSnapshot {
    let app_snapshot = semantic_inputs.app_snapshot;
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

    let focused_control = if onboarding_active {
        Some(ControlId::OnboardingRoot)
    } else if let Some(QueuedModal::ContactsCreate(modal_state)) = state.modal_queue.current() {
        let _ = modal_state;
        Some(ControlId::ModalInput)
    } else if let Some(modal_id) = open_modal {
        Some(ControlId::Modal(modal_id))
    } else {
        match state.screen() {
            Screen::Neighborhood => Some(ControlId::Screen(ScreenId::Neighborhood)),
            Screen::Chat => Some(ControlId::Screen(ScreenId::Chat)),
            Screen::Contacts => Some(ControlId::Screen(ScreenId::Contacts)),
            Screen::Notifications => Some(ControlId::Screen(ScreenId::Notifications)),
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
    let selected_navigation_id = (!onboarding_active).then(|| screen_item_id(screen));
    push_list(
        &mut lists,
        &mut selections,
        ListId::Navigation,
        navigation_ids,
        selected_navigation_id,
    );

    let selected_channel_id = semantic_inputs
        .chat_channels
        .get(state.chat.selected_channel)
        .map(|channel| channel.id.clone());
    let channel_items = semantic_inputs
        .chat_channels
        .iter()
        .map(|channel| ListItemSnapshot {
            id: channel.id.clone(),
            selected: selected_channel_id.as_ref() == Some(&channel.id),
            confirmation: ConfirmationState::Confirmed,
        })
        .collect::<Vec<_>>();
    push_list(
        &mut lists,
        &mut selections,
        ListId::Channels,
        channel_items,
        selected_channel_id.clone(),
    );

    let contact_items = semantic_inputs
        .contacts
        .iter()
        .enumerate()
        .map(|(idx, contact)| ListItemSnapshot {
            id: contact.id.clone(),
            selected: idx == state.contacts.selected_index,
            confirmation: ConfirmationState::Confirmed,
        })
        .collect::<Vec<_>>();
    let selected_contact_id = contact_items
        .iter()
        .find(|item| item.selected)
        .map(|item| item.id.clone());
    push_list(
        &mut lists,
        &mut selections,
        ListId::Contacts,
        contact_items,
        selected_contact_id,
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

    let current_home_id = app_snapshot
        .homes
        .current_home()
        .map(|home| home.id)
        .filter(|home_id| *home_id != aura_core::identifiers::ChannelId::default());
    let neighborhood_home_id = (app_snapshot.neighborhood.home_home_id
        != aura_core::identifiers::ChannelId::default())
    .then_some(app_snapshot.neighborhood.home_home_id);
    let mut home_ids = app_snapshot
        .homes
        .iter()
        .map(|(home_id, _)| *home_id)
        .filter(|home_id| *home_id != aura_core::identifiers::ChannelId::default())
        .map(|home_id| home_id.to_string())
        .collect::<Vec<_>>();
    home_ids.sort();
    home_ids.dedup();
    if let Some(current_home_id) = current_home_id {
        let current_home_id = current_home_id.to_string();
        home_ids.retain(|home_id| home_id != &current_home_id);
        home_ids.insert(0, current_home_id);
    } else if let Some(neighborhood_home_id) = neighborhood_home_id {
        let neighborhood_home_id = neighborhood_home_id.to_string();
        if !home_ids
            .iter()
            .any(|home_id| home_id == &neighborhood_home_id)
        {
            home_ids.insert(0, neighborhood_home_id);
        }
    }
    let selected_home_id = current_home_id.or(neighborhood_home_id);
    let neighbor_home_ids = app_snapshot
        .neighborhood
        .all_neighbors()
        .filter(|home| Some(home.id) != selected_home_id)
        .map(|home| home.id.to_string())
        .filter(|home_id| !home_ids.iter().any(|existing| existing == home_id))
        .collect::<Vec<_>>();
    home_ids.extend(neighbor_home_ids);
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
        .map(|section| aura_app::ui_contract::settings_section_item_id(section.surface_id()).to_string())
        .collect::<Vec<_>>();
    let settings_items = SettingsSection::all()
        .iter()
        .map(|section| ListItemSnapshot {
            id: aura_app::ui_contract::settings_section_item_id(section.surface_id()).to_string(),
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

    if state.settings.section == SettingsSection::Devices {
        let device_items = semantic_inputs
            .settings_devices
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

    let messages = if semantic_inputs.chat_messages.is_empty() {
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
    } else {
        semantic_inputs
            .chat_messages
            .iter()
            .map(|message| MessageSnapshot {
                id: message.id.clone(),
                content: message.content.clone(),
            })
            .collect::<Vec<_>>()
    };

    let operations = state.exported_operation_snapshots();
    let runtime_events = state.exported_runtime_events();
    let readiness = if onboarding_active {
        UiReadiness::Loading
    } else {
        UiReadiness::Ready
    };

    let snapshot = UiSnapshot {
        screen,
        focused_control,
        open_modal,
        readiness,
        revision: next_projection_revision(None),
        quiescence: QuiescenceSnapshot::derive(readiness, open_modal, &operations),
        selections,
        lists,
        messages,
        operations,
        toasts,
        runtime_events,
    };
    snapshot
        .validate_invariants()
        .unwrap_or_else(|error| panic!("invalid TUI semantic snapshot export: {error}"));
    snapshot
}

pub fn maybe_export_ui_snapshot(
    state: &TuiState,
    semantic_inputs: TuiSemanticInputs<'_>,
) {
    let socket_path = configured_ui_state_socket();
    let file_path = configured_ui_state_file();
    if socket_path.is_none() && file_path.is_none() {
        return;
    }

    let snapshot = authoritative_ui_snapshot(state, semantic_inputs);
    let Ok(snapshot_json) = serde_json::to_string_pretty(&snapshot) else {
        return;
    };

    let mut last_written = last_written_snapshot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if last_written.as_deref() == Some(snapshot_json.as_str()) {
        return;
    }

    let write_result = socket_path
        .map(|path| {
            UnixStream::connect(path).and_then(|mut stream| stream.write_all(snapshot_json.as_bytes()))
        })
        .or_else(|| file_path.map(|path| write_snapshot_file(path, &snapshot_json)));
    if matches!(write_result, Some(Ok(()))) {
        *last_written = Some(snapshot_json);
    }
}

#[cfg(test)]
mod tests {
    use super::{authoritative_ui_snapshot, TuiSemanticInputs};
    use crate::tui::screens::Screen;
    use crate::tui::state::modal_queue::QueuedModal;
    use crate::tui::state::views::{AccountSetupModalState, DeviceEnrollmentCeremonyModalState};
    use crate::tui::TuiState;
    use aura_app::ui::contract::{ControlId, ListId, OperationId, OperationState, UiReadiness};
    use aura_app::ui::types::StateSnapshot;
    use aura_app::ui_contract::RuntimeFact;
    use std::path::Path;

    #[test]
    fn account_setup_maps_to_onboarding_state() {
        let mut state = TuiState::new();
        state.show_modal(QueuedModal::AccountSetup(AccountSetupModalState::default()));

        let app_snapshot = StateSnapshot::default();
        let snapshot = authoritative_ui_snapshot(
            &state,
            TuiSemanticInputs {
                app_snapshot: &app_snapshot,
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        );
        assert_eq!(snapshot.readiness, UiReadiness::Loading);
        assert_eq!(snapshot.focused_control, Some(ControlId::OnboardingRoot));
        assert_eq!(snapshot.open_modal, None);
    }

    #[test]
    fn navigation_list_marks_current_screen() {
        let mut state = TuiState::new();
        state.router.go_to(Screen::Contacts);

        let app_snapshot = StateSnapshot::default();
        let snapshot = authoritative_ui_snapshot(
            &state,
            TuiSemanticInputs {
                app_snapshot: &app_snapshot,
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        );
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

        let app_snapshot = StateSnapshot::default();
        let snapshot = authoritative_ui_snapshot(
            &state,
            TuiSemanticInputs {
                app_snapshot: &app_snapshot,
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        );
        let operation_state = snapshot
            .operations
            .iter()
            .find(|operation| operation.id == OperationId::device_enrollment())
            .map(|operation| operation.state);

        assert_eq!(operation_state, Some(OperationState::Submitting));
    }

    #[test]
    fn semantic_snapshot_does_not_synthesize_placeholder_contact_ids() {
        let mut state = TuiState::new();
        state.contacts.contact_count = 3;

        let app_snapshot = StateSnapshot::default();
        let snapshot = authoritative_ui_snapshot(
            &state,
            TuiSemanticInputs {
                app_snapshot: &app_snapshot,
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        );

        let contacts = snapshot
            .lists
            .iter()
            .find(|list| list.id == ListId::Contacts)
            .map(|list| list.items.clone())
            .unwrap_or_default();

        assert!(contacts.is_empty());
        assert!(!snapshot
            .selections
            .iter()
            .any(|selection| selection.list == ListId::Contacts));
    }

    #[test]
    fn semantic_snapshot_exporter_does_not_depend_on_parity_override_caches() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let harness_state_path = repo_root.join("crates/aura-terminal/src/tui/harness_state.rs");
        let source = std::fs::read_to_string(&harness_state_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", harness_state_path.display())
        });
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(&source);

        assert!(
            !production_source.contains("static CONTACTS_OVERRIDE")
                && !production_source.contains("static DEVICES_OVERRIDE")
                && !production_source.contains("static MESSAGES_OVERRIDE"),
            "parity-critical TUI exports may not depend on override caches"
        );
        assert!(
            !production_source.contains("pub fn publish_contacts_list_export")
                && !production_source.contains("pub fn publish_devices_list_export")
                && !production_source.contains("pub fn publish_messages_export"),
            "parity-critical TUI exports may not declare parity override helpers"
        );
    }

    #[test]
    fn semantic_snapshot_ready_state_is_projection_only() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let harness_state_path = repo_root.join("crates/aura-terminal/src/tui/harness_state.rs");
        let source = std::fs::read_to_string(&harness_state_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", harness_state_path.display())
        });
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(&source);

        assert!(
            !production_source.contains("contacts_override_input")
                && !production_source.contains("contact_items.is_empty()")
                && !production_source.contains("if home_ids.is_empty()"),
            "ready-state TUI export must stay pure projection without reconstruction fallbacks"
        );
    }

    #[test]
    fn semantic_snapshot_exports_tui_owned_runtime_facts() {
        let mut state = TuiState::new();
        state.upsert_runtime_fact(RuntimeFact::InvitationCodeReady {
            receiver_authority_id: None,
            source_operation: OperationId::invitation_create(),
            code: Some("invite-code".to_string()),
        });

        let app_snapshot = StateSnapshot::default();
        let snapshot = authoritative_ui_snapshot(
            &state,
            TuiSemanticInputs {
                app_snapshot: &app_snapshot,
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        );

        assert!(snapshot.runtime_events.iter().any(|event| {
            matches!(
                &event.fact,
                RuntimeFact::InvitationCodeReady { source_operation, .. }
                    if *source_operation == OperationId::invitation_create()
            )
        }));
    }

    #[test]
    fn semantic_snapshot_exporter_does_not_infer_parity_runtime_events() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let harness_state_path = repo_root.join("crates/aura-terminal/src/tui/harness_state.rs");
        let source = std::fs::read_to_string(&harness_state_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", harness_state_path.display())
        });
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(&source);

        for forbidden in [
            "RuntimeFact::ContactLinkReady",
            "RuntimeFact::PendingHomeInvitationReady",
            "RuntimeFact::ChannelMembershipReady",
            "RuntimeFact::RecipientPeersResolved",
            "RuntimeFact::MessageDeliveryReady",
            "runtime_events.push(RuntimeEventSnapshot",
        ] {
            assert!(
                !production_source.contains(forbidden),
                "parity-critical runtime facts must not be synthesized during TUI snapshot export: {forbidden}"
            );
        }
    }
}
