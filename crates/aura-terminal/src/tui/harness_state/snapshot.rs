//! Snapshot export for harness observation.

use super::commands::{
    map_modal, map_screen, map_toast_kind, push_list, screen_item_id, selected_by_index,
    visible_home_ids, TuiSemanticInputs,
};
use crate::tui::screens::Screen;
use crate::tui::state::modal_queue::QueuedModal;
use crate::tui::TuiState;
use aura_app::ui::contract::{
    ConfirmationState, ControlId, ListId, ListItemSnapshot, MessageSnapshot, ModalId, ScreenId,
    ToastId, ToastSnapshot, UiReadiness, UiSnapshot,
};
use aura_app::ui_contract::{next_projection_revision, QuiescenceSnapshot};
use parking_lot::Mutex;
use std::fs;
use std::io;
use std::io::Write;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::tui::types::SettingsSection;

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

fn write_snapshot_file(path: &Path, snapshot_json: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, snapshot_json)?;
    fs::rename(temp_path, path)?;
    Ok(())
}

pub fn try_authoritative_ui_snapshot(
    state: &TuiState,
    semantic_inputs: TuiSemanticInputs<'_>,
) -> Result<UiSnapshot, String> {
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

    let mut home_ids = Vec::new();
    if app_snapshot.neighborhood.home_home_id != aura_core::types::identifiers::ChannelId::default()
    {
        home_ids.push(app_snapshot.neighborhood.home_home_id.to_string());
    }
    let neighbor_home_ids = app_snapshot
        .neighborhood
        .all_neighbors()
        .filter(|home| home.id != aura_core::types::identifiers::ChannelId::default())
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
        .map(|section| {
            aura_app::ui_contract::settings_section_item_id(section.surface_id()).to_string()
        })
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
    let readiness = if state.pending_runtime_bootstrap {
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
    snapshot.validate_invariants()?;
    Ok(snapshot)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn authoritative_ui_snapshot(
    state: &TuiState,
    semantic_inputs: TuiSemanticInputs<'_>,
) -> UiSnapshot {
    try_authoritative_ui_snapshot(state, semantic_inputs)
        .unwrap_or_else(|error| panic!("invalid TUI semantic snapshot export: {error}"))
}

pub fn maybe_export_ui_snapshot(
    state: &TuiState,
    semantic_inputs: TuiSemanticInputs<'_>,
) -> Result<(), String> {
    let socket_path = configured_ui_state_socket();
    let file_path = configured_ui_state_file();
    if socket_path.is_none() && file_path.is_none() {
        return Ok(());
    }

    let snapshot = try_authoritative_ui_snapshot(state, semantic_inputs)?;
    let snapshot_json = serde_json::to_string_pretty(&snapshot)
        .map_err(|error| format!("failed to encode TUI semantic snapshot: {error}"))?;

    let mut last_written = last_written_snapshot().lock();
    if last_written.as_deref() == Some(snapshot_json.as_str()) {
        return Ok(());
    }

    let write_result = socket_path
        .map(|path| {
            StdUnixStream::connect(path)
                .and_then(|mut stream| stream.write_all(snapshot_json.as_bytes()))
        })
        .or_else(|| file_path.map(|path| write_snapshot_file(path, &snapshot_json)));
    if matches!(write_result, Some(Ok(()))) {
        *last_written = Some(snapshot_json);
        return Ok(());
    }
    if let Some(Err(error)) = write_result {
        return Err(format!("failed to publish TUI semantic snapshot: {error}"));
    }
    Ok(())
}
