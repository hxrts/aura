//! Harness command application logic.

use crate::tui::screens::Screen;
use crate::tui::state::commands::ThresholdK;
use crate::tui::state::modal_queue::QueuedModal;
use crate::tui::state::toast::ToastLevel;
use crate::tui::state::{DispatchCommand, ImportInvitationModalState, InvitationKind, TuiCommand};
use crate::tui::types::{
    Channel as TuiChannel, Contact as TuiContact, Device as TuiDevice, Message as TuiMessage,
    SettingsSection,
};
use crate::tui::TuiState;
use aura_app::ui::contract::{
    ControlId, HarnessUiCommand, ListId, ListItemSnapshot, ListSnapshot, ModalId, ScreenId,
    SelectionSnapshot, ToastKind,
};
use aura_app::ui::types::StateSnapshot;

#[derive(Clone, Copy)]
pub struct TuiSemanticInputs<'a> {
    pub app_snapshot: &'a StateSnapshot,
    pub contacts: &'a [TuiContact],
    pub settings_devices: &'a [TuiDevice],
    pub chat_channels: &'a [TuiChannel],
    pub chat_messages: &'a [TuiMessage],
}

pub(super) fn map_screen(screen: Screen) -> ScreenId {
    match screen {
        Screen::Neighborhood => ScreenId::Neighborhood,
        Screen::Chat => ScreenId::Chat,
        Screen::Contacts => ScreenId::Contacts,
        Screen::Notifications => ScreenId::Notifications,
        Screen::Settings => ScreenId::Settings,
    }
}

fn screen_from_id(screen: ScreenId) -> Option<Screen> {
    match screen {
        ScreenId::Onboarding => None,
        ScreenId::Neighborhood => Some(Screen::Neighborhood),
        ScreenId::Chat => Some(Screen::Chat),
        ScreenId::Contacts => Some(Screen::Contacts),
        ScreenId::Notifications => Some(Screen::Notifications),
        ScreenId::Settings => Some(Screen::Settings),
    }
}

fn settings_section_from_item_id(item_id: &str) -> Option<SettingsSection> {
    match item_id {
        "profile" => Some(SettingsSection::Profile),
        "guardian-threshold" => Some(SettingsSection::Threshold),
        "request-recovery" => Some(SettingsSection::Recovery),
        "devices" => Some(SettingsSection::Devices),
        "authority" => Some(SettingsSection::Authority),
        "observability" => Some(SettingsSection::Observability),
        _ => None,
    }
}

fn select_settings_section(state: &mut TuiState, section: SettingsSection) {
    state.router.go_to(Screen::Settings);
    state.settings.section = section;
    state.settings.selected_index = section.index();
    state.settings.focus = crate::tui::navigation::TwoPanelFocus::List;
}

pub(super) fn visible_home_ids(app_snapshot: &StateSnapshot) -> Vec<String> {
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
    home_ids
}

pub(super) fn screen_item_id(screen: ScreenId) -> String {
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

pub(super) fn map_modal(modal: &QueuedModal) -> Option<ModalId> {
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

pub(super) fn map_toast_kind(level: ToastLevel) -> ToastKind {
    match level {
        ToastLevel::Success => ToastKind::Success,
        ToastLevel::Info | ToastLevel::Warning => ToastKind::Info,
        ToastLevel::Error => ToastKind::Error,
    }
}

pub(super) fn push_list(
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

pub(super) fn selected_by_index(items: &[String], index: usize) -> Option<String> {
    items.get(index).cloned()
}

pub(crate) fn apply_harness_command(
    state: &mut TuiState,
    command: HarnessUiCommand,
    semantic_inputs: TuiSemanticInputs<'_>,
) -> Result<Vec<TuiCommand>, String> {
    match command {
        HarnessUiCommand::NavigateScreen { screen } => {
            if let Some(screen) = screen_from_id(screen) {
                state.router.go_to(screen);
            }
            Ok(Vec::new())
        }
        HarnessUiCommand::DismissTransient => {
            if state.modal_queue.current().is_some() {
                state.modal_queue.dismiss();
            } else if state.toast_queue.current().is_some() {
                state.toast_queue.dismiss();
            }
            Ok(Vec::new())
        }
        HarnessUiCommand::OpenSettingsSection { section } => {
            let section = match section {
                aura_app::scenario_contract::SettingsSection::Devices => SettingsSection::Devices,
            };
            select_settings_section(state, section);
            Ok(Vec::new())
        }
        HarnessUiCommand::ActivateControl { control_id } => match control_id {
            ControlId::NavNeighborhood => {
                state.router.go_to(Screen::Neighborhood);
                Ok(Vec::new())
            }
            ControlId::NavChat => {
                state.router.go_to(Screen::Chat);
                Ok(Vec::new())
            }
            ControlId::NavContacts => {
                state.router.go_to(Screen::Contacts);
                Ok(Vec::new())
            }
            ControlId::NavNotifications => {
                state.router.go_to(Screen::Notifications);
                Ok(Vec::new())
            }
            ControlId::NavSettings => {
                state.router.go_to(Screen::Settings);
                Ok(Vec::new())
            }
            ControlId::SettingsAddDeviceButton => {
                select_settings_section(state, SettingsSection::Devices);
                let mut modal_state = crate::tui::state::AddDeviceModalState::default();
                if !state.settings.demo_mobile_authority_id.is_empty() {
                    modal_state.invitee_authority_id =
                        state.settings.demo_mobile_authority_id.clone();
                }
                state
                    .modal_queue
                    .enqueue(QueuedModal::SettingsAddDevice(modal_state));
                Ok(Vec::new())
            }
            ControlId::SettingsImportDeviceCodeButton => {
                select_settings_section(state, SettingsSection::Devices);
                state.modal_queue.enqueue(QueuedModal::SettingsDeviceImport(
                    ImportInvitationModalState::default(),
                ));
                if !state.settings.demo_mobile_device_id.is_empty() {
                    state.next_toast_id += 1;
                    state
                        .toast_queue
                        .enqueue(crate::tui::state::QueuedToast::new(
                            state.next_toast_id,
                            "[DEMO] Press Ctrl+m to auto-fill the Mobile device code",
                            ToastLevel::Info,
                        ));
                }
                Ok(Vec::new())
            }
            ControlId::SettingsRemoveDeviceButton => {
                select_settings_section(state, SettingsSection::Devices);
                Ok(vec![TuiCommand::Dispatch(
                    DispatchCommand::OpenDeviceSelectModal,
                )])
            }
            ControlId::ContactsInviteToChannelButton => Ok(vec![TuiCommand::Dispatch(
                DispatchCommand::InviteSelectedContactToChannel,
            )]),
            _ => Ok(Vec::new()),
        },
        HarnessUiCommand::ActivateListItem { list_id, item_id } => match list_id {
            ListId::Navigation => {
                let screen = match item_id.as_str() {
                    "neighborhood" => Some(Screen::Neighborhood),
                    "chat" => Some(Screen::Chat),
                    "contacts" => Some(Screen::Contacts),
                    "notifications" => Some(Screen::Notifications),
                    "settings" => Some(Screen::Settings),
                    _ => None,
                };
                if let Some(screen) = screen {
                    state.router.go_to(screen);
                }
                Ok(Vec::new())
            }
            ListId::SettingsSections => {
                if let Some(section) = settings_section_from_item_id(&item_id) {
                    select_settings_section(state, section);
                }
                Ok(Vec::new())
            }
            ListId::Channels => {
                let selected_index = semantic_inputs
                    .chat_channels
                    .iter()
                    .position(|candidate| candidate.id == item_id)
                    .ok_or_else(|| format!("channel list item {item_id} is not visible"))?;
                state.router.go_to(Screen::Chat);
                state.chat.selected_channel = selected_index;
                Ok(Vec::new())
            }
            ListId::Contacts => {
                let selected_index = semantic_inputs
                    .contacts
                    .iter()
                    .position(|candidate| candidate.id == item_id)
                    .ok_or_else(|| format!("contact list item {item_id} is not visible"))?;
                state.router.go_to(Screen::Contacts);
                state.contacts.selected_index = selected_index;
                Ok(Vec::new())
            }
            _ => Ok(Vec::new()),
        },
        HarnessUiCommand::CreateAccount { account_name } => {
            Ok(vec![TuiCommand::Dispatch(DispatchCommand::CreateAccount {
                name: account_name,
            })])
        }
        HarnessUiCommand::CreateHome { home_name } => {
            Ok(vec![TuiCommand::Dispatch(DispatchCommand::CreateHome {
                name: home_name,
                description: None,
            })])
        }
        HarnessUiCommand::CreateChannel { channel_name } => {
            state.router.go_to(Screen::Chat);
            Ok(vec![TuiCommand::Dispatch(DispatchCommand::CreateChannel {
                name: channel_name,
                topic: None,
                members: Vec::new(),
                threshold_k: ThresholdK::new(1)
                    .map_err(|error| format!("invalid default channel threshold: {error}"))?,
            })])
        }
        HarnessUiCommand::SelectHome { home_id } => {
            let home_ids = visible_home_ids(semantic_inputs.app_snapshot);
            let selected_index = home_ids
                .iter()
                .position(|candidate| candidate == &home_id)
                .ok_or_else(|| format!("home list item {home_id} is not visible"))?;
            state.router.go_to(Screen::Neighborhood);
            state.neighborhood.grid.set_cols(1);
            state.neighborhood.grid.set_count(home_ids.len());
            state.neighborhood.grid.select(selected_index);
            state.neighborhood.selected_home = selected_index;
            Ok(Vec::new())
        }
        HarnessUiCommand::StartDeviceEnrollment { device_name } => {
            select_settings_section(state, SettingsSection::Devices);
            let invitee_authority_id = state
                .settings
                .demo_mobile_authority_id
                .parse::<aura_core::AuthorityId>()
                .map_err(|error| {
                    format!("invalid or missing demo mobile authority id in settings state: {error}")
                })?;
            Ok(vec![TuiCommand::Dispatch(DispatchCommand::AddDevice {
                name: device_name,
                invitee_authority_id,
            })])
        }
        HarnessUiCommand::ImportDeviceEnrollmentCode { code } => Ok(vec![TuiCommand::Dispatch(
            DispatchCommand::ImportDeviceEnrollmentDuringOnboarding { code },
        )]),
        HarnessUiCommand::RemoveSelectedDevice { device_id } => {
            select_settings_section(state, SettingsSection::Devices);
            let _ = semantic_inputs;
            Ok(vec![TuiCommand::HarnessRemoveVisibleDevice { device_id }])
        }
        HarnessUiCommand::SwitchAuthority { authority_id } => {
            state.router.go_to(Screen::Settings);
            state.settings.section = SettingsSection::Authority;
            let Some(selected_index) = state
                .authorities
                .iter()
                .position(|authority| authority.id == authority_id)
            else {
                return Err(format!("authority {authority_id} is not visible"));
            };
            state.current_authority_index = selected_index;
            if state
                .authorities
                .get(selected_index)
                .is_some_and(|authority| authority.is_current)
            {
                return Ok(Vec::new());
            }
            let authority_id = authority_id
                .parse::<aura_core::AuthorityId>()
                .map_err(|error| format!("invalid authority id {authority_id}: {error}"))?;
            Ok(vec![TuiCommand::Dispatch(
                DispatchCommand::SwitchAuthority { authority_id },
            )])
        }
        HarnessUiCommand::CreateContactInvitation {
            receiver_authority_id,
        } => {
            let receiver_id = receiver_authority_id
                .parse::<aura_core::AuthorityId>()
                .map_err(|error| {
                    format!("invalid authority id {receiver_authority_id}: {error}")
                })?;
            Ok(vec![TuiCommand::Dispatch(
                DispatchCommand::CreateInvitation {
                    receiver_id,
                    invitation_type: InvitationKind::Contact,
                    message: None,
                    ttl_secs: None,
                },
            )])
        }
        HarnessUiCommand::ImportInvitation { code } => Ok(vec![TuiCommand::Dispatch(
            DispatchCommand::ImportInvitation { code },
        )]),
        HarnessUiCommand::InviteActorToChannel {
            authority_id,
            channel_id,
        } => {
            let authority_id = authority_id
                .parse::<aura_core::AuthorityId>()
                .map_err(|error| format!("invalid authority id: {error}"))?;
            channel_id
                .parse::<aura_core::ChannelId>()
                .map_err(|error| format!("invalid channel id: {error}"))?;
            Ok(vec![TuiCommand::Dispatch(
                DispatchCommand::InviteActorToChannel {
                    authority_id,
                    channel_id,
                },
            )])
        }
        HarnessUiCommand::AcceptPendingChannelInvitation => {
            state.router.go_to(Screen::Chat);
            Ok(vec![TuiCommand::Dispatch(
                DispatchCommand::AcceptPendingHomeInvitation,
            )])
        }
        HarnessUiCommand::JoinChannel { channel_name } => {
            state.router.go_to(Screen::Chat);
            Ok(vec![TuiCommand::Dispatch(DispatchCommand::JoinChannel {
                channel_name,
            })])
        }
        HarnessUiCommand::SelectChannel { channel_id } => {
            let channel_visible = semantic_inputs
                .chat_channels
                .iter()
                .any(|candidate| candidate.id == channel_id);
            if !channel_visible {
                return Err(format!("channel list item {channel_id} is not visible"));
            }
            state.router.go_to(Screen::Chat);
            Ok(vec![TuiCommand::Dispatch(DispatchCommand::SelectChannel {
                channel_id: channel_id.into(),
            })])
        }
        HarnessUiCommand::SendChatMessage { content } => Ok(vec![TuiCommand::Dispatch(
            DispatchCommand::SendChatMessage { content },
        )]),
    }
}
