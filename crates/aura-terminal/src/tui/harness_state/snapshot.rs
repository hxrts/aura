//! Snapshot export for harness observation.

use super::commands::{
    map_modal, map_screen, map_toast_kind, push_list, selected_by_index, TuiSemanticInputs,
};
use super::socket::authoritative_harness_snapshot_readiness;
use crate::tui::screens::Screen;
use crate::tui::state::modal_queue::QueuedModal;
use crate::tui::TuiState;
use aura_app::ui::contract::{
    screen_item_id, ConfirmationState, ControlId, ListId, ListItemSnapshot, MessageSnapshot,
    ScreenId, ToastId, ToastSnapshot, UiReadiness, UiSnapshot,
};
use aura_app::ui_contract::{
    next_projection_revision, ProjectionRevision, QuiescenceSnapshot, QuiescenceState,
};
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
static LAST_WRITTEN_SNAPSHOT: OnceLock<Mutex<Option<Vec<u8>>>> = OnceLock::new();

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

fn last_written_snapshot() -> &'static Mutex<Option<Vec<u8>>> {
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

fn build_authoritative_ui_snapshot(
    state: &TuiState,
    semantic_inputs: TuiSemanticInputs<'_>,
    revision: ProjectionRevision,
) -> Result<UiSnapshot, String> {
    let app_snapshot = semantic_inputs.app_snapshot;
    let screen = authoritative_screen_id(state);
    let onboarding_active = screen == ScreenId::Onboarding;
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
                id: screen_item_id(id).to_string(),
                selected: !onboarding_active && *candidate == state.screen(),
                confirmation: ConfirmationState::Confirmed,
                is_current: false,
            }
        })
        .collect::<Vec<_>>();
    let selected_navigation_id = (!onboarding_active).then(|| screen_item_id(screen).to_string());
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
            is_current: false,
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
            is_current: false,
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
            is_current: false,
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
                is_current: false,
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
            is_current: false,
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
            is_current: false,
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
            is_current: false,
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
        .map(|section| section.parity_item_id().to_string())
        .collect::<Vec<_>>();
    let settings_items = SettingsSection::all()
        .iter()
        .map(|section| ListItemSnapshot {
            id: section.parity_item_id().to_string(),
            selected: *section == state.settings.section,
            confirmation: ConfirmationState::Confirmed,
            is_current: false,
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
        // The TUI does not own an inline device-list selection on the Settings screen.
        // Do not fabricate one from visible rows; remove-device flow uses the modal-owned selector.
        let device_items = semantic_inputs
            .settings_devices
            .iter()
            .map(|device| ListItemSnapshot {
                id: device.id.clone(),
                selected: false,
                confirmation: ConfirmationState::Confirmed,
                is_current: device.is_current,
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
    let readiness = authoritative_harness_snapshot_readiness(
        state.should_exit,
        state.pending_runtime_bootstrap,
    );

    let snapshot = UiSnapshot {
        screen,
        focused_control,
        open_modal,
        readiness,
        revision,
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

pub fn try_authoritative_ui_snapshot(
    state: &TuiState,
    semantic_inputs: TuiSemanticInputs<'_>,
) -> Result<UiSnapshot, String> {
    build_authoritative_ui_snapshot(state, semantic_inputs, next_projection_revision(None))
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn authoritative_ui_snapshot(
    state: &TuiState,
    semantic_inputs: TuiSemanticInputs<'_>,
) -> UiSnapshot {
    try_authoritative_ui_snapshot(state, semantic_inputs)
        .unwrap_or_else(|error| panic!("invalid TUI semantic snapshot export: {error}"))
}

fn authoritative_screen_id(state: &TuiState) -> ScreenId {
    if matches!(
        state.modal_queue.current(),
        Some(QueuedModal::AccountSetup(_))
    ) {
        ScreenId::Onboarding
    } else {
        map_screen(state.screen())
    }
}

fn publish_snapshot(snapshot: &UiSnapshot) -> Result<(), String> {
    let socket_path = configured_ui_state_socket();
    let file_path = configured_ui_state_file();
    if socket_path.is_none() && file_path.is_none() {
        return Ok(());
    }

    let canonical_snapshot = UiSnapshot {
        revision: ProjectionRevision {
            semantic_seq: 0,
            render_seq: None,
        },
        ..snapshot.clone()
    };
    let canonical_json = serde_json::to_vec(&canonical_snapshot)
        .map_err(|error| format!("failed to encode canonical TUI semantic snapshot: {error}"))?;
    let snapshot_json = serde_json::to_vec(snapshot)
        .map_err(|error| format!("failed to encode TUI semantic snapshot: {error}"))?;

    let mut last_written = last_written_snapshot().lock();
    if last_written.as_deref() == Some(canonical_json.as_slice()) {
        return Ok(());
    }

    let socket_write_result = socket_path.map(|path| {
        StdUnixStream::connect(path).and_then(|mut stream| stream.write_all(&snapshot_json))
    });
    let file_write_result = file_path.map(|path| {
        let text = std::str::from_utf8(&snapshot_json)
            .map_err(|error| io::Error::other(format!("invalid UTF-8 snapshot: {error}")))?;
        write_snapshot_file(path, text)
    });
    if matches!(socket_write_result, Some(Ok(()))) || matches!(file_write_result, Some(Ok(()))) {
        *last_written = Some(canonical_json);
        return Ok(());
    }
    if let Some(Err(error)) = socket_write_result {
        return Err(format!("failed to publish TUI semantic snapshot: {error}"));
    }
    if let Some(Err(error)) = file_write_result {
        return Err(format!(
            "failed to publish TUI semantic snapshot mirror file: {error}"
        ));
    }
    Ok(())
}

pub fn publish_loading_ui_snapshot(state: &TuiState) -> Result<(), String> {
    let screen = authoritative_screen_id(state);
    let mut snapshot = UiSnapshot::loading(screen);
    snapshot.operations = state.exported_operation_snapshots();
    snapshot.runtime_events = state.exported_runtime_events();
    snapshot.toasts = state
        .toast_queue
        .current()
        .map(|toast| {
            vec![ToastSnapshot {
                id: ToastId(toast.id.to_string()),
                message: toast.message.clone(),
                kind: map_toast_kind(toast.level),
            }]
        })
        .unwrap_or_default();
    snapshot.quiescence = QuiescenceSnapshot {
        state: QuiescenceState::Busy,
        reason_codes: vec!["owner_transition_reloading".to_string()],
    };
    snapshot.readiness = UiReadiness::Loading;
    publish_snapshot(&snapshot)
}

pub fn maybe_export_ui_snapshot(
    state: &TuiState,
    semantic_inputs: TuiSemanticInputs<'_>,
) -> Result<(), String> {
    // Build a canonical snapshot with a stable placeholder revision so identical
    // semantic state deduplicates cleanly instead of generating a fresh revision
    // and flooding the harness bridge on every render.
    let snapshot =
        build_authoritative_ui_snapshot(state, semantic_inputs, next_projection_revision(None))?;
    publish_snapshot(&snapshot)
}

#[cfg(test)]
mod tests {
    use crate::tui::harness_state::TuiSemanticInputs;

    #[test]
    fn tui_harness_snapshot_exports_canonical_selection_for_parity_lists() {
        let source = include_str!("snapshot.rs");

        assert!(source.contains("ListId::Channels"));
        assert!(source.contains("selected_channel_id.clone()"));
        assert!(source.contains("ListId::Contacts"));
        assert!(source.contains("selected_contact_id"));
        assert!(source.contains("ListId::Homes"));
        assert!(source.contains("selected_by_index(&home_ids, state.neighborhood.selected_home)"));
        assert!(source.contains("ListId::Authorities"));
        assert!(source.contains("selected_by_index(&authority_ids, state.current_authority_index)"));
        assert!(source.contains("ListId::SettingsSections"));
        assert!(source.contains("settings_section_ids"));
    }

    #[test]
    fn tui_harness_snapshot_does_not_fabricate_device_selection_without_owned_state() {
        let source = include_str!("snapshot.rs");
        let devices_start = source
            .find("if state.settings.section == SettingsSection::Devices {")
            .unwrap_or_else(|| panic!("missing device snapshot branch"));
        let devices_end = source[devices_start..]
            .find("let toasts = state")
            .map(|offset| devices_start + offset)
            .unwrap_or(source.len());
        let devices_block = &source[devices_start..devices_end];

        assert!(devices_block.contains("ListId::Devices"));
        assert!(devices_block.contains("selected: false"));
        assert!(devices_block.contains("None,"));
    }

    #[test]
    fn onboarding_snapshot_does_not_mark_navigation_row_selected_without_exported_selection() {
        use super::authoritative_ui_snapshot;
        use crate::tui::state::modal_queue::QueuedModal;
        use crate::tui::state::views::AccountSetupModalState;
        use crate::tui::TuiState;
        use aura_app::ui::contract::{ListId, ScreenId};
        use aura_app::ui::types::StateSnapshot;

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

        assert_eq!(snapshot.screen, ScreenId::Onboarding);
        assert_eq!(snapshot.selected_item_id(ListId::Navigation), None);
        let navigation = snapshot
            .lists
            .iter()
            .find(|list| list.id == ListId::Navigation)
            .unwrap_or_else(|| panic!("navigation list should exist"));
        assert!(navigation.items.iter().all(|item| !item.selected));
    }
}
