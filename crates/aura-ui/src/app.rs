//! Dioxus-based web UI application root and screen components.
//!
//! Provides the main application shell, screen routing, keyboard handling,
//! and toast notifications for the Aura web interface.

use crate::components::{
    AuthorityPickerItem, ButtonVariant, ModalView, PillTone, UiAuthorityPickerModal, UiButton,
    UiCard, UiDeviceEnrollmentModal, UiFooter, UiListButton, UiListItem, UiModal, UiPill,
};
use crate::model::{
    AccessDepth, AddDeviceWizardStep, CreateChannelDetailsField, CreateChannelWizardStep,
    ModalState, NeighborhoodMode, ThresholdWizardStep, UiController, UiModel, UiScreen,
};
use aura_app::signal_defs::SettingsState;
use aura_app::ui::signals::{
    NetworkStatus, CHAT_SIGNAL, CONTACTS_SIGNAL, ERROR_SIGNAL, HOMES_SIGNAL, INVITATIONS_SIGNAL,
    NEIGHBORHOOD_SIGNAL, NETWORK_STATUS_SIGNAL, RECOVERY_SIGNAL, SETTINGS_SIGNAL,
    TRANSPORT_PEERS_SIGNAL,
};
use aura_app::ui::types::{
    format_network_status_with_severity, AccessLevel, AppError, ChatState, ContactsState, HomeRole,
    HomesState, InvitationBridgeType, InvitationsState, NeighborhoodState, RecoveryState,
};
use aura_app::ui::workflows::ceremonies as ceremony_workflows;
use aura_app::ui::workflows::moderator as moderator_workflows;
use aura_app::ui::workflows::{
    access as access_workflows, contacts as contacts_workflows, context as context_workflows,
    invitation as invitation_workflows, messaging as messaging_workflows,
    recovery as recovery_workflows, settings as settings_workflows, time as time_workflows,
};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::identifiers::{AuthorityId, CeremonyId};
use dioxus::dioxus_core::schedule_update;
use dioxus::events::KeyboardData;
use dioxus::prelude::*;
use dioxus_shadcn::components::empty::{Empty, EmptyDescription, EmptyHeader, EmptyTitle};
use dioxus_shadcn::components::scroll_area::{ScrollArea, ScrollAreaViewport};
use dioxus_shadcn::components::toast::{use_toast, ToastOptions, ToastPosition, ToastProvider};
use dioxus_shadcn::theme::{themes, use_theme, ColorScheme, ThemeProvider};
use std::sync::Arc;
use std::time::Duration;

const SETTINGS_ROWS: [&str; 6] = [
    "Profile",
    "Guardian Threshold",
    "Request Recovery",
    "Devices",
    "Authority",
    "Appearance",
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct NeighborhoodRuntimeHome {
    id: String,
    name: String,
    member_count: Option<u32>,
    can_enter: bool,
    is_local: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct NeighborhoodRuntimeMember {
    authority_id: String,
    name: String,
    role_label: String,
    is_self: bool,
    is_online: bool,
    is_moderator: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct NeighborhoodRuntimeChannel {
    name: String,
    topic: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct NeighborhoodRuntimeView {
    loaded: bool,
    neighborhood_name: String,
    active_home_name: String,
    active_home_id: String,
    homes: Vec<NeighborhoodRuntimeHome>,
    members: Vec<NeighborhoodRuntimeMember>,
    channels: Vec<NeighborhoodRuntimeChannel>,
    network_status: String,
    transport_peers: usize,
    online_contacts: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ChatRuntimeChannel {
    name: String,
    topic: String,
    unread_count: u32,
    last_message: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ChatRuntimeMessage {
    sender_name: String,
    content: String,
    is_own: bool,
    delivery_status: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ChatRuntimeView {
    loaded: bool,
    active_channel: String,
    channels: Vec<ChatRuntimeChannel>,
    messages: Vec<ChatRuntimeMessage>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ContactsRuntimeContact {
    authority_id: String,
    name: String,
    nickname_hint: Option<String>,
    is_guardian: bool,
    is_member: bool,
    is_online: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ContactsRuntimeView {
    loaded: bool,
    contacts: Vec<ContactsRuntimeContact>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SettingsRuntimeDevice {
    id: String,
    name: String,
    is_current: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SettingsRuntimeAuthority {
    id: String,
    label: String,
    is_current: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SettingsRuntimeView {
    loaded: bool,
    nickname: String,
    authority_id: String,
    threshold_k: u8,
    threshold_n: u8,
    guardian_count: usize,
    active_recovery_label: String,
    pending_recovery_requests: usize,
    guardian_binding_count: usize,
    mfa_policy: String,
    devices: Vec<SettingsRuntimeDevice>,
    authorities: Vec<SettingsRuntimeAuthority>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct NotificationRuntimeItem {
    id: String,
    kind_label: String,
    title: String,
    subtitle: String,
    detail: String,
    timestamp: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct NotificationsRuntimeView {
    loaded: bool,
    items: Vec<NotificationRuntimeItem>,
}

fn role_label(role: HomeRole) -> &'static str {
    match role {
        HomeRole::Participant => "Participant",
        HomeRole::Member => "Member",
        HomeRole::Moderator => "Moderator",
    }
}

fn active_home_scope_id(neighborhood: &NeighborhoodState) -> String {
    neighborhood
        .position
        .as_ref()
        .map(|position| position.current_home_id.to_string())
        .unwrap_or_else(|| neighborhood.home_home_id.to_string())
}

fn is_dm_like_channel(channel: &aura_app::ui::types::Channel) -> bool {
    channel.is_dm
        || channel.name.to_ascii_lowercase().starts_with("dm:")
        || channel
            .topic
            .as_deref()
            .map(|topic| topic.to_ascii_lowercase().starts_with("direct messages"))
            .unwrap_or(false)
}

fn scoped_channels(
    chat_state: &ChatState,
    active_home_scope: Option<&str>,
) -> Vec<NeighborhoodRuntimeChannel> {
    let active_home_scope = active_home_scope
        .map(str::trim)
        .filter(|scope| !scope.is_empty());
    let active_home_channel = active_home_scope.and_then(|scope| {
        chat_state
            .all_channels()
            .find(|channel| channel.id.to_string() == scope)
    });
    let has_active_home_channel = active_home_channel.is_some();
    let active_home_context = active_home_channel.and_then(|channel| channel.context_id);

    let mut channels: Vec<_> = chat_state
        .all_channels()
        .filter(|channel| {
            if is_dm_like_channel(channel) {
                return true;
            }

            match active_home_scope {
                None => true,
                Some(scope) => {
                    let id_match = channel.id.to_string() == scope;
                    let context_match = active_home_context
                        .map(|ctx| channel.context_id == Some(ctx))
                        .unwrap_or(false);
                    id_match || context_match || !has_active_home_channel
                }
            }
        })
        .map(|channel| NeighborhoodRuntimeChannel {
            name: channel.name.clone(),
            topic: channel.topic.clone().unwrap_or_default(),
        })
        .collect();

    channels.sort_by(|left, right| left.name.cmp(&right.name));
    channels
}

fn build_neighborhood_runtime_view(
    authority_id: &str,
    neighborhood: NeighborhoodState,
    homes: HomesState,
    contacts: ContactsState,
    chat: ChatState,
    network_status: NetworkStatus,
    transport_peers: usize,
) -> NeighborhoodRuntimeView {
    let neighborhood_name = neighborhood
        .neighborhood_name
        .clone()
        .unwrap_or_else(|| neighborhood.home_name.clone());
    let active_home_name = neighborhood
        .position
        .as_ref()
        .map(|position| position.current_home_name.clone())
        .unwrap_or_else(|| neighborhood.home_name.clone());
    let active_home_id = neighborhood
        .position
        .as_ref()
        .map(|position| position.current_home_id.to_string())
        .unwrap_or_else(|| neighborhood.home_home_id.to_string());

    let current_home = homes.current_home().cloned();

    let mut runtime_homes = Vec::new();
    if neighborhood.home_home_id != Default::default() || !neighborhood.home_name.is_empty() {
        runtime_homes.push(NeighborhoodRuntimeHome {
            id: neighborhood.home_home_id.to_string(),
            name: neighborhood.home_name.clone(),
            member_count: current_home.as_ref().map(|home| home.member_count),
            can_enter: true,
            is_local: true,
        });
    }

    for neighbor in neighborhood.all_neighbors() {
        runtime_homes.push(NeighborhoodRuntimeHome {
            id: neighbor.id.to_string(),
            name: neighbor.name.clone(),
            member_count: neighbor.member_count,
            can_enter: neighbor.can_traverse,
            is_local: false,
        });
    }

    runtime_homes.sort_by(|left, right| left.name.cmp(&right.name));
    runtime_homes.dedup_by(|left, right| left.id == right.id);

    let members = current_home
        .as_ref()
        .map(|home| {
            let mut rows: Vec<_> = home
                .members
                .iter()
                .map(|member| NeighborhoodRuntimeMember {
                    authority_id: member.id.to_string(),
                    name: member.name.clone(),
                    role_label: role_label(member.role).to_string(),
                    is_self: member.id.to_string() == authority_id,
                    is_online: member.is_online,
                    is_moderator: member.is_moderator(),
                })
                .collect();
            rows.sort_by(|left, right| left.name.cmp(&right.name));
            rows
        })
        .unwrap_or_default();

    let active_scope = active_home_scope_id(&neighborhood);
    let channels = scoped_channels(&chat, Some(active_scope.as_str()));
    let online_contacts = contacts
        .all_contacts()
        .filter(|contact| contact.is_online)
        .count();
    let network_status = format_network_status_with_severity(&network_status, None).0;

    NeighborhoodRuntimeView {
        loaded: true,
        neighborhood_name,
        active_home_name,
        active_home_id,
        homes: runtime_homes,
        members,
        channels,
        network_status,
        transport_peers,
        online_contacts,
    }
}

async fn load_neighborhood_runtime_view(controller: Arc<UiController>) -> NeighborhoodRuntimeView {
    let authority_id = controller.authority_id();

    let neighborhood = {
        let core = controller.app_core().read().await;
        core.read(&*NEIGHBORHOOD_SIGNAL).await.unwrap_or_default()
    };
    let homes = {
        let core = controller.app_core().read().await;
        core.read(&*HOMES_SIGNAL).await.unwrap_or_default()
    };
    let contacts = {
        let core = controller.app_core().read().await;
        core.read(&*CONTACTS_SIGNAL).await.unwrap_or_default()
    };
    let chat = {
        let core = controller.app_core().read().await;
        core.read(&*CHAT_SIGNAL).await.unwrap_or_default()
    };
    let network_status = {
        let core = controller.app_core().read().await;
        core.read(&*NETWORK_STATUS_SIGNAL).await.unwrap_or_default()
    };
    let transport_peers = {
        let core = controller.app_core().read().await;
        core.read(&*TRANSPORT_PEERS_SIGNAL)
            .await
            .unwrap_or_default()
    };

    build_neighborhood_runtime_view(
        &authority_id,
        neighborhood,
        homes,
        contacts,
        chat,
        network_status,
        transport_peers,
    )
}

fn display_contact_name(contact: &aura_app::ui::types::Contact) -> String {
    if !contact.nickname.trim().is_empty() {
        return contact.nickname.clone();
    }
    if let Some(suggestion) = contact
        .nickname_suggestion
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        return suggestion.clone();
    }
    contact.id.to_string().chars().take(8).collect()
}

fn build_chat_runtime_view(
    chat: ChatState,
    selected_channel_name: Option<&str>,
) -> ChatRuntimeView {
    let mut channels: Vec<_> = chat
        .all_channels()
        .map(|channel| ChatRuntimeChannel {
            name: channel.name.clone(),
            topic: channel.topic.clone().unwrap_or_default(),
            unread_count: channel.unread_count,
            last_message: channel.last_message.clone(),
        })
        .collect();
    channels.sort_by(|left, right| left.name.cmp(&right.name));

    let active_channel = selected_channel_name
        .and_then(|name| {
            channels
                .iter()
                .find(|channel| channel.name.eq_ignore_ascii_case(name))
                .map(|channel| channel.name.clone())
        })
        .or_else(|| channels.first().map(|channel| channel.name.clone()))
        .unwrap_or_default();

    let messages = chat
        .all_channels()
        .find(|channel| channel.name.eq_ignore_ascii_case(&active_channel))
        .map(|channel| {
            chat.messages_for_channel(&channel.id)
                .iter()
                .map(|message| ChatRuntimeMessage {
                    sender_name: message.sender_name.clone(),
                    content: message.content.clone(),
                    is_own: message.is_own,
                    delivery_status: message.delivery_status.description().to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    ChatRuntimeView {
        loaded: true,
        active_channel,
        channels,
        messages,
    }
}

async fn load_chat_runtime_view(controller: Arc<UiController>) -> ChatRuntimeView {
    let chat = {
        let core = controller.app_core().read().await;
        core.read(&*CHAT_SIGNAL).await.unwrap_or_default()
    };
    let selected_name = controller
        .ui_model()
        .and_then(|model| model.selected_channel_name().map(str::to_string));
    let runtime = build_chat_runtime_view(chat, selected_name.as_deref());
    controller.sync_runtime_channels(
        runtime
            .channels
            .iter()
            .map(|channel| (channel.name.clone(), channel.topic.clone()))
            .collect(),
    );
    runtime
}

fn build_contacts_runtime_view(contacts: ContactsState) -> ContactsRuntimeView {
    let mut rows: Vec<_> = contacts
        .all_contacts()
        .map(|contact| ContactsRuntimeContact {
            authority_id: contact.id.to_string(),
            name: display_contact_name(contact),
            nickname_hint: contact
                .nickname_suggestion
                .clone()
                .filter(|value| !value.trim().is_empty()),
            is_guardian: contact.is_guardian,
            is_member: contact.is_member,
            is_online: contact.is_online,
        })
        .collect();
    rows.sort_by(|left, right| left.name.cmp(&right.name));
    ContactsRuntimeView {
        loaded: true,
        contacts: rows,
    }
}

async fn load_contacts_runtime_view(controller: Arc<UiController>) -> ContactsRuntimeView {
    let contacts = {
        let core = controller.app_core().read().await;
        core.read(&*CONTACTS_SIGNAL).await.unwrap_or_default()
    };
    let runtime = build_contacts_runtime_view(contacts);
    controller.sync_runtime_contacts(
        runtime
            .contacts
            .iter()
            .map(|contact| (contact.name.clone(), contact.is_guardian))
            .collect(),
    );
    runtime
}

fn build_settings_runtime_view(
    settings: SettingsState,
    recovery: RecoveryState,
) -> SettingsRuntimeView {
    let devices = settings
        .devices
        .iter()
        .map(|device| SettingsRuntimeDevice {
            id: device.id.to_string(),
            name: if device.name.trim().is_empty() {
                let short = device.id.to_string().chars().take(8).collect::<String>();
                format!("Device {short}")
            } else {
                device.name.clone()
            },
            is_current: device.is_current,
        })
        .collect();
    let authorities = settings
        .authorities
        .iter()
        .map(|authority| SettingsRuntimeAuthority {
            id: authority.id.clone(),
            label: if authority.nickname_suggestion.trim().is_empty() {
                authority.id.clone()
            } else {
                authority.nickname_suggestion.clone()
            },
            is_current: authority.is_current,
        })
        .collect();

    let active_recovery_label = recovery
        .active_recovery()
        .map(|process| format!("{:?}", process.status))
        .unwrap_or_else(|| "Idle".to_string());

    SettingsRuntimeView {
        loaded: true,
        nickname: settings.nickname_suggestion,
        authority_id: settings.authority_id,
        threshold_k: settings.threshold_k,
        threshold_n: settings.threshold_n,
        guardian_count: recovery.guardian_count(),
        active_recovery_label,
        pending_recovery_requests: recovery.pending_requests().len(),
        guardian_binding_count: recovery.guardian_binding_count(),
        mfa_policy: settings.mfa_policy,
        devices,
        authorities,
    }
}

async fn load_settings_runtime_view(controller: Arc<UiController>) -> SettingsRuntimeView {
    let settings = {
        let core = controller.app_core().read().await;
        core.read(&*SETTINGS_SIGNAL).await.unwrap_or_default()
    };
    let recovery = {
        let core = controller.app_core().read().await;
        core.read(&*RECOVERY_SIGNAL).await.unwrap_or_default()
    };
    let runtime = build_settings_runtime_view(settings, recovery);
    controller.sync_runtime_profile(runtime.authority_id.clone(), runtime.nickname.clone());
    controller.sync_runtime_devices(
        runtime
            .devices
            .iter()
            .map(|device| (device.name.clone(), device.is_current))
            .collect(),
    );
    controller.sync_runtime_authorities(
        runtime
            .authorities
            .iter()
            .map(|authority| {
                (
                    authority.id.clone(),
                    authority.label.clone(),
                    authority.is_current,
                )
            })
            .collect(),
    );
    runtime
}

fn build_notifications_runtime_view(
    invitations: InvitationsState,
    recovery: RecoveryState,
    error: Option<AppError>,
) -> NotificationsRuntimeView {
    let mut items = Vec::new();

    for invitation in invitations.all_pending() {
        if !matches!(
            invitation.direction,
            aura_app::ui::types::InvitationDirection::Received
        ) || !matches!(
            invitation.status,
            aura_app::ui::types::InvitationStatus::Pending
        ) {
            continue;
        }

        let kind_label = match invitation.invitation_type {
            aura_app::ui::types::InvitationType::Guardian => "Guardian Request",
            aura_app::ui::types::InvitationType::Chat => "Contact Request",
            aura_app::ui::types::InvitationType::Home => "Home Invite",
        };
        items.push(NotificationRuntimeItem {
            id: invitation.id.clone(),
            kind_label: kind_label.to_string(),
            title: format!("{kind_label} from {}", invitation.from_name),
            subtitle: invitation
                .message
                .clone()
                .unwrap_or_else(|| "Pending response".to_string()),
            detail: invitation
                .home_name
                .clone()
                .unwrap_or_else(|| invitation.from_id.to_string()),
            timestamp: invitation.created_at,
        });
    }

    for request in recovery.pending_requests() {
        items.push(NotificationRuntimeItem {
            id: request.id.to_string(),
            kind_label: "Recovery Approval".to_string(),
            title: "Recovery approval requested".to_string(),
            subtitle: format!(
                "{}/{} approvals",
                request.approvals_received, request.approvals_required
            ),
            detail: request.account_id.to_string(),
            timestamp: request.initiated_at,
        });
    }

    if let Some(error) = error {
        items.push(NotificationRuntimeItem {
            id: "runtime-error".to_string(),
            kind_label: "Runtime Error".to_string(),
            title: "Latest runtime error".to_string(),
            subtitle: error.to_string(),
            detail: "Check browser console and runtime logs for context.".to_string(),
            timestamp: 0,
        });
    }

    items.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
    NotificationsRuntimeView {
        loaded: true,
        items,
    }
}

async fn load_notifications_runtime_view(
    controller: Arc<UiController>,
) -> NotificationsRuntimeView {
    let invitations = {
        let core = controller.app_core().read().await;
        core.read(&*INVITATIONS_SIGNAL).await.unwrap_or_default()
    };
    let recovery = {
        let core = controller.app_core().read().await;
        core.read(&*RECOVERY_SIGNAL).await.unwrap_or_default()
    };
    let error = {
        let core = controller.app_core().read().await;
        core.read(&*ERROR_SIGNAL).await.unwrap_or_default()
    };
    build_notifications_runtime_view(invitations, recovery, error)
}

fn selected_home_id_for_modal(
    runtime: &NeighborhoodRuntimeView,
    model: &UiModel,
) -> Option<String> {
    let selected_home = model.selected_home.as_deref();
    runtime
        .homes
        .iter()
        .find(|home| Some(home.name.as_str()) == selected_home)
        .map(|home| home.id.clone())
        .filter(|id| !id.is_empty())
        .or_else(|| {
            if !runtime.active_home_id.is_empty() {
                Some(runtime.active_home_id.clone())
            } else {
                None
            }
        })
}

fn selected_contact_for_modal(
    runtime: &ContactsRuntimeView,
    model: &UiModel,
) -> Option<ContactsRuntimeContact> {
    runtime.contacts.get(model.selected_contact_index).cloned()
}

fn removable_device_for_modal(
    runtime: &SettingsRuntimeView,
    model: &UiModel,
) -> Option<SettingsRuntimeDevice> {
    runtime
        .devices
        .iter()
        .find(|device| {
            !device.is_current
                && device.name
                    == model
                        .secondary_device_name()
                        .unwrap_or(model.remove_device_candidate_name.as_str())
        })
        .cloned()
        .or_else(|| {
            runtime
                .devices
                .iter()
                .find(|device| !device.is_current)
                .cloned()
        })
}

fn submit_runtime_modal_action(
    controller: Arc<UiController>,
    modal_state: Option<ModalState>,
    add_device_step: AddDeviceWizardStep,
    add_device_ceremony_id: Option<String>,
    add_device_is_complete: bool,
    add_device_has_failed: bool,
    modal_buffer: String,
    neighborhood_runtime: NeighborhoodRuntimeView,
    chat_runtime: ChatRuntimeView,
    contacts_runtime: ContactsRuntimeView,
    settings_runtime: SettingsRuntimeView,
    selected_home_id: Option<String>,
    selected_member_index: usize,
    rerender: Arc<dyn Fn() + Send + Sync>,
) -> bool {
    let current_model = controller.ui_model();
    match modal_state {
        Some(ModalState::AddDeviceStep1) => match add_device_step {
            AddDeviceWizardStep::Name => {
                let name = modal_buffer.trim().to_string();
                if name.is_empty() {
                    controller.runtime_error_toast("Device name is required");
                    rerender();
                    return true;
                }

                let app_core = controller.app_core().clone();
                let rerender_for_start = rerender.clone();
                spawn(async move {
                    match ceremony_workflows::start_device_enrollment_ceremony(
                        &app_core,
                        name.clone(),
                        None,
                    )
                    .await
                    {
                        Ok(start) => {
                            controller.set_runtime_device_enrollment_ceremony_id(
                                &start.ceremony_id.to_string(),
                            );
                            controller.complete_runtime_device_enrollment_started(
                                &name,
                                &start.enrollment_code,
                            );

                            let controller_for_status = controller.clone();
                            let app_core_for_status = app_core.clone();
                            let rerender_for_status = rerender_for_start.clone();
                            let ceremony_id = CeremonyId::new(start.ceremony_id.to_string());
                            spawn(async move {
                                loop {
                                    let _ =
                                        time_workflows::sleep_ms(&app_core_for_status, 1_000).await;
                                    match ceremony_workflows::get_key_rotation_ceremony_status(
                                        &app_core_for_status,
                                        &ceremony_id,
                                    )
                                    .await
                                    {
                                        Ok(status) => {
                                            controller_for_status
                                                .update_runtime_device_enrollment_status(
                                                    status.accepted_count,
                                                    status.total_count,
                                                    status.threshold,
                                                    status.is_complete,
                                                    status.has_failed,
                                                    status.error_message.clone(),
                                                );
                                            rerender_for_status();
                                            if status.is_complete || status.has_failed {
                                                break;
                                            }
                                        }
                                        Err(_) => break,
                                    }
                                }
                            });
                        }
                        Err(error) => controller.runtime_error_toast(error.to_string()),
                    }
                    rerender_for_start();
                });
                true
            }
            AddDeviceWizardStep::ShareCode => {
                controller.advance_runtime_device_enrollment_share();
                rerender();
                true
            }
            AddDeviceWizardStep::Confirm => {
                if add_device_is_complete || add_device_has_failed {
                    controller.complete_runtime_device_enrollment_ready();
                    rerender();
                    return true;
                }

                let Some(ceremony_id) = add_device_ceremony_id else {
                    controller.runtime_error_toast("No active enrollment ceremony");
                    rerender();
                    return true;
                };

                let app_core = controller.app_core().clone();
                let rerender_for_status = rerender.clone();
                spawn(async move {
                    match ceremony_workflows::get_key_rotation_ceremony_status(
                        &app_core,
                        &CeremonyId::new(ceremony_id),
                    )
                    .await
                    {
                        Ok(status) => controller.update_runtime_device_enrollment_status(
                            status.accepted_count,
                            status.total_count,
                            status.threshold,
                            status.is_complete,
                            status.has_failed,
                            status.error_message.clone(),
                        ),
                        Err(error) => controller.runtime_error_toast(error.to_string()),
                    }
                    rerender_for_status();
                });
                true
            }
        },
        Some(ModalState::CreateHome) => {
            let name = modal_buffer.trim().to_string();
            if name.is_empty() {
                controller.runtime_error_toast("Home name is required");
                rerender();
                return true;
            }

            let app_core = controller.app_core().clone();
            let rerender_for_create = rerender.clone();
            spawn(async move {
                match context_workflows::create_home(&app_core, Some(name.clone()), None).await {
                    Ok(_) => controller.complete_runtime_home_created(&name),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_create();
            });
            true
        }
        Some(ModalState::AcceptInvitation) => {
            let code = modal_buffer.trim().to_string();
            if code.is_empty() {
                controller.runtime_error_toast("Invitation code is required");
                rerender();
                return true;
            }

            let app_core = controller.app_core().clone();
            let rerender_for_import = rerender.clone();
            spawn(async move {
                match invitation_workflows::import_invitation_details(&app_core, &code).await {
                    Ok(invitation) => {
                        let accepted = match invitation.invitation_type {
                            InvitationBridgeType::DeviceEnrollment { .. } => Ok(()),
                            _ => {
                                invitation_workflows::accept_invitation(
                                    &app_core,
                                    &invitation.invitation_id,
                                )
                                .await
                            }
                        };

                        match accepted {
                            Ok(_) => controller.complete_runtime_invitation_import(),
                            Err(error) => controller.runtime_error_toast(error.to_string()),
                        }
                    }
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_import();
            });
            true
        }
        Some(ModalState::ImportDeviceEnrollmentCode) => {
            let code = modal_buffer.trim().to_string();
            if code.is_empty() {
                controller.runtime_error_toast("Enrollment code is required");
                rerender();
                return true;
            }

            let app_core = controller.app_core().clone();
            let rerender_for_import = rerender.clone();
            spawn(async move {
                match invitation_workflows::import_invitation_details(&app_core, &code).await {
                    Ok(invitation) => {
                        if !matches!(
                            invitation.invitation_type,
                            InvitationBridgeType::DeviceEnrollment { .. }
                        ) {
                            controller
                                .runtime_error_toast("Code is not a device enrollment invitation");
                            rerender_for_import();
                            return;
                        }

                        match invitation_workflows::accept_invitation(
                            &app_core,
                            &invitation.invitation_id,
                        )
                        .await
                        {
                            Ok(()) => {
                                let _ =
                                    settings_workflows::refresh_settings_from_runtime(&app_core)
                                        .await;
                                controller
                                    .complete_runtime_modal_success("Device enrollment complete");
                            }
                            Err(error) => controller.runtime_error_toast(error.to_string()),
                        }
                    }
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_import();
            });
            true
        }
        Some(ModalState::CreateInvitation) => {
            let receiver = modal_buffer.trim().to_string();
            if receiver.is_empty() {
                controller.runtime_error_toast("Receiver authority id is required");
                rerender();
                return true;
            }

            let app_core = controller.app_core().clone();
            let rerender_for_create = rerender.clone();
            spawn(async move {
                let receiver_id = match receiver.parse::<AuthorityId>() {
                    Ok(value) => value,
                    Err(error) => {
                        controller.runtime_error_toast(format!("Invalid authority id: {error}"));
                        rerender_for_create();
                        return;
                    }
                };

                match invitation_workflows::create_contact_invitation(
                    &app_core,
                    receiver_id,
                    None,
                    None,
                    None,
                )
                .await
                {
                    Ok(invitation) => match invitation_workflows::export_invitation(
                        &app_core,
                        &invitation.invitation_id,
                    )
                    .await
                    {
                        Ok(code) => {
                            controller.write_clipboard(&code);
                            controller.complete_runtime_modal_success(
                                "Invitation code copied to clipboard",
                            );
                        }
                        Err(error) => controller.runtime_error_toast(error.to_string()),
                    },
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_create();
            });
            true
        }
        Some(ModalState::CreateChannel)
            if matches!(
                current_model.as_ref().map(|m| m.create_channel_step),
                Some(CreateChannelWizardStep::Threshold)
            ) =>
        {
            let Some(model) = current_model.clone() else {
                return false;
            };
            let app_core = controller.app_core().clone();
            let rerender_for_create = rerender.clone();
            spawn(async move {
                let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
                    Ok(value) => value,
                    Err(error) => {
                        controller.runtime_error_toast(error.to_string());
                        rerender_for_create();
                        return;
                    }
                };
                let members: Vec<String> = model
                    .create_channel_selected_members
                    .iter()
                    .filter_map(|idx| contacts_runtime.contacts.get(*idx))
                    .map(|contact| contact.authority_id.clone())
                    .collect();

                match messaging_workflows::create_channel(
                    &app_core,
                    model.create_channel_name.trim(),
                    (!model.create_channel_topic.trim().is_empty())
                        .then(|| model.create_channel_topic.trim().to_string()),
                    &members,
                    model.create_channel_threshold,
                    timestamp_ms,
                )
                .await
                {
                    Ok(_) => controller.complete_runtime_modal_success(format!(
                        "Created '{}'",
                        model.create_channel_name.trim()
                    )),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_create();
            });
            true
        }
        Some(ModalState::SetChannelTopic) => {
            let channel_name = chat_runtime.active_channel.trim().to_string();
            let topic = modal_buffer.trim().to_string();
            if channel_name.is_empty() {
                controller.runtime_error_toast("Select a channel first");
                rerender();
                return true;
            }

            let app_core = controller.app_core().clone();
            let rerender_for_topic = rerender.clone();
            spawn(async move {
                let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
                    Ok(value) => value,
                    Err(error) => {
                        controller.runtime_error_toast(error.to_string());
                        rerender_for_topic();
                        return;
                    }
                };
                match messaging_workflows::set_topic_by_name(
                    &app_core,
                    &channel_name,
                    &topic,
                    timestamp_ms,
                )
                .await
                {
                    Ok(()) => controller.complete_runtime_modal_success("Topic updated"),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_topic();
            });
            true
        }
        Some(ModalState::EditNickname) => {
            let value = modal_buffer.trim().to_string();
            if value.is_empty() {
                controller.runtime_error_toast("Nickname is required");
                rerender();
                return true;
            }

            let app_core = controller.app_core().clone();
            let rerender_for_nickname = rerender.clone();
            let selected_contact = current_model
                .as_ref()
                .and_then(|model| selected_contact_for_modal(&contacts_runtime, model));
            let is_settings_screen = current_model
                .as_ref()
                .map(|model| matches!(model.screen, UiScreen::Settings))
                .unwrap_or(false);
            spawn(async move {
                let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
                    Ok(value) => value,
                    Err(error) => {
                        controller.runtime_error_toast(error.to_string());
                        rerender_for_nickname();
                        return;
                    }
                };
                let result = if is_settings_screen {
                    settings_workflows::update_nickname(&app_core, value.clone()).await
                } else if let Some(contact) = selected_contact {
                    contacts_workflows::update_contact_nickname(
                        &app_core,
                        &contact.authority_id,
                        &value,
                        timestamp_ms,
                    )
                    .await
                } else {
                    Err(aura_core::AuraError::not_found("No contact selected"))
                };

                match result {
                    Ok(()) => controller.complete_runtime_modal_success("Nickname updated"),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_nickname();
            });
            true
        }
        Some(ModalState::RemoveContact) => {
            let Some(contact) = current_model
                .as_ref()
                .and_then(|model| selected_contact_for_modal(&contacts_runtime, model))
            else {
                controller.runtime_error_toast("Select a contact first");
                rerender();
                return true;
            };
            let app_core = controller.app_core().clone();
            let rerender_for_remove = rerender.clone();
            spawn(async move {
                let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
                    Ok(value) => value,
                    Err(error) => {
                        controller.runtime_error_toast(error.to_string());
                        rerender_for_remove();
                        return;
                    }
                };
                match contacts_workflows::remove_contact(
                    &app_core,
                    &contact.authority_id,
                    timestamp_ms,
                )
                .await
                {
                    Ok(()) => controller.complete_runtime_modal_success("Contact removed"),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_remove();
            });
            true
        }
        Some(ModalState::RequestRecovery) => {
            let app_core = controller.app_core().clone();
            let rerender_for_recovery = rerender.clone();
            spawn(async move {
                match recovery_workflows::start_recovery_from_state(&app_core).await {
                    Ok(_) => controller.complete_runtime_modal_success("Recovery process started"),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_recovery();
            });
            true
        }
        Some(ModalState::GuardianSetup)
            if matches!(
                current_model.as_ref().map(|m| m.guardian_wizard_step),
                Some(ThresholdWizardStep::Ceremony)
            ) =>
        {
            let Some(model) = current_model.clone() else {
                return false;
            };
            let app_core = controller.app_core().clone();
            let rerender_for_guardians = rerender.clone();
            spawn(async move {
                let ids = model
                    .guardian_selected_indices
                    .iter()
                    .filter_map(|idx| contacts_runtime.contacts.get(*idx))
                    .map(|contact| contact.authority_id.parse::<AuthorityId>())
                    .collect::<Result<Vec<_>, _>>();
                let guardian_ids = match ids {
                    Ok(value) => value,
                    Err(error) => {
                        controller.runtime_error_toast(format!("Invalid guardian id: {error}"));
                        rerender_for_guardians();
                        return;
                    }
                };
                let threshold = match aura_core::types::FrostThreshold::new(u16::from(
                    model.guardian_threshold_k,
                )) {
                    Ok(value) => value,
                    Err(error) => {
                        controller.runtime_error_toast(format!("Invalid threshold: {error}"));
                        rerender_for_guardians();
                        return;
                    }
                };

                match ceremony_workflows::start_guardian_ceremony(
                    &app_core,
                    threshold,
                    guardian_ids.len() as u16,
                    guardian_ids,
                )
                .await
                {
                    Ok(_) => controller.complete_runtime_modal_success("Guardian ceremony started"),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_guardians();
            });
            true
        }
        Some(ModalState::ConfirmRemoveDevice) => {
            let Some(device) = current_model
                .as_ref()
                .and_then(|model| removable_device_for_modal(&settings_runtime, model))
            else {
                controller.runtime_error_toast("No removable device selected");
                rerender();
                return true;
            };
            let app_core = controller.app_core().clone();
            let rerender_for_remove = rerender.clone();
            spawn(async move {
                match ceremony_workflows::start_device_removal_ceremony(
                    &app_core,
                    device.id.clone(),
                )
                .await
                {
                    Ok(ceremony_id) => {
                        match ceremony_workflows::get_key_rotation_ceremony_status(
                            &app_core,
                            &ceremony_id,
                        )
                        .await
                        {
                            Ok(status) if status.is_complete => {
                                let _ =
                                    settings_workflows::refresh_settings_from_runtime(&app_core)
                                        .await;
                                controller
                                    .complete_runtime_modal_success("Device removal complete");
                            }
                            Ok(_) => controller
                                .complete_runtime_modal_success("Device removal ceremony started"),
                            Err(error) => controller.runtime_error_toast(error.to_string()),
                        }
                    }
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_remove();
            });
            true
        }
        Some(ModalState::MfaSetup)
            if matches!(
                current_model.as_ref().map(|m| m.mfa_wizard_step),
                Some(ThresholdWizardStep::Ceremony)
            ) =>
        {
            let Some(model) = current_model else {
                return false;
            };
            let app_core = controller.app_core().clone();
            let rerender_for_mfa = rerender.clone();
            spawn(async move {
                let device_ids: Vec<String> = model
                    .mfa_selected_indices
                    .iter()
                    .filter_map(|idx| settings_runtime.devices.get(*idx))
                    .map(|device| device.id.clone())
                    .collect();
                let threshold =
                    match aura_core::types::FrostThreshold::new(u16::from(model.mfa_threshold_k)) {
                        Ok(value) => value,
                        Err(error) => {
                            controller.runtime_error_toast(format!("Invalid threshold: {error}"));
                            rerender_for_mfa();
                            return;
                        }
                    };

                match ceremony_workflows::start_device_threshold_ceremony(
                    &app_core,
                    threshold,
                    device_ids.len() as u16,
                    device_ids,
                )
                .await
                {
                    Ok(_) => {
                        controller.complete_runtime_modal_success("Multifactor ceremony started")
                    }
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_mfa();
            });
            true
        }
        Some(ModalState::AssignModerator) => {
            let Some(selected_home_id) = selected_home_id else {
                controller.runtime_error_toast("Select an entered home first");
                rerender();
                return true;
            };
            let Some(member) = neighborhood_runtime
                .members
                .get(selected_member_index)
                .cloned()
            else {
                controller.runtime_error_toast("Select a member first");
                rerender();
                return true;
            };
            if member.authority_id.is_empty() {
                controller.runtime_error_toast("Selected member cannot be resolved");
                rerender();
                return true;
            }

            let app_core = controller.app_core().clone();
            let rerender_for_moderator = rerender.clone();
            spawn(async move {
                let result = if member.is_moderator {
                    moderator_workflows::revoke_moderator(
                        &app_core,
                        Some(selected_home_id.as_str()),
                        &member.authority_id,
                    )
                    .await
                } else {
                    moderator_workflows::grant_moderator(
                        &app_core,
                        Some(selected_home_id.as_str()),
                        &member.authority_id,
                    )
                    .await
                };

                match result {
                    Ok(_) => controller.complete_runtime_modal_success(if member.is_moderator {
                        format!("Moderator revoked for {}", member.name)
                    } else {
                        format!("Moderator granted for {}", member.name)
                    }),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_moderator();
            });
            true
        }
        Some(ModalState::SwitchAuthority) => {
            let Some(model) = current_model.clone() else {
                return false;
            };
            let Some(authority) = settings_runtime
                .authorities
                .get(model.selected_authority_index)
                .cloned()
            else {
                controller.runtime_error_toast("Select an authority first");
                rerender();
                return true;
            };

            if authority.is_current {
                controller.complete_runtime_modal_success("Already using that authority");
                rerender();
                return true;
            }

            if !controller.request_authority_switch(&authority.id) {
                controller.runtime_error_toast("Authority switching is not available");
                rerender();
                return true;
            }

            true
        }
        Some(ModalState::AccessOverride) => {
            let Some(model) = current_model.clone() else {
                return false;
            };
            let Some(selected_home_id) = selected_home_id else {
                controller.runtime_error_toast("Select an entered home first");
                rerender();
                return true;
            };
            let Some(contact) = contacts_runtime
                .contacts
                .get(model.selected_contact_index)
                .cloned()
            else {
                controller.runtime_error_toast("Select a contact first");
                rerender();
                return true;
            };
            let authority_id = match contact.authority_id.parse::<AuthorityId>() {
                Ok(authority_id) => authority_id,
                Err(error) => {
                    controller.runtime_error_toast(format!(
                        "Invalid authority id for {}: {error}",
                        contact.name
                    ));
                    rerender();
                    return true;
                }
            };
            let access_level = if model.access_override_partial {
                AccessLevel::Partial
            } else {
                AccessLevel::Limited
            };

            let app_core = controller.app_core().clone();
            let rerender_for_override = rerender.clone();
            spawn(async move {
                match access_workflows::set_access_override(
                    &app_core,
                    Some(selected_home_id.as_str()),
                    authority_id,
                    access_level,
                )
                .await
                {
                    Ok(()) => controller.complete_runtime_modal_success(format!(
                        "Access override set for {} ({})",
                        contact.name,
                        match access_level {
                            AccessLevel::Partial => "Partial",
                            AccessLevel::Limited => "Limited",
                            AccessLevel::Full => "Full",
                        }
                    )),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_override();
            });
            true
        }
        Some(ModalState::CapabilityConfig) => {
            let Some(model) = current_model.clone() else {
                return false;
            };
            let Some(selected_home_id) = selected_home_id else {
                controller.runtime_error_toast("Select an entered home first");
                rerender();
                return true;
            };

            let (full_caps, partial_caps, limited_caps) = match model.capability_active_field {
                0 => (
                    modal_buffer.clone(),
                    model.capability_partial_caps.clone(),
                    model.capability_limited_caps.clone(),
                ),
                1 => (
                    model.capability_full_caps.clone(),
                    modal_buffer.clone(),
                    model.capability_limited_caps.clone(),
                ),
                _ => (
                    model.capability_full_caps.clone(),
                    model.capability_partial_caps.clone(),
                    modal_buffer.clone(),
                ),
            };

            let app_core = controller.app_core().clone();
            let rerender_for_caps = rerender.clone();
            spawn(async move {
                match access_workflows::configure_home_capabilities(
                    &app_core,
                    Some(selected_home_id.as_str()),
                    &full_caps,
                    &partial_caps,
                    &limited_caps,
                )
                .await
                {
                    Ok(()) => controller.complete_runtime_modal_success("Capability config saved"),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_caps();
            });
            true
        }
        _ => false,
    }
}

fn cancel_runtime_modal_action(
    controller: Arc<UiController>,
    modal_state: Option<ModalState>,
    add_device_step: AddDeviceWizardStep,
    add_device_ceremony_id: Option<String>,
    add_device_is_complete: bool,
    add_device_has_failed: bool,
    rerender: Arc<dyn Fn() + Send + Sync>,
) -> bool {
    if !matches!(modal_state, Some(ModalState::AddDeviceStep1)) {
        return false;
    }
    if matches!(add_device_step, AddDeviceWizardStep::Name)
        || add_device_is_complete
        || add_device_has_failed
    {
        return false;
    }

    let Some(ceremony_id) = add_device_ceremony_id else {
        return false;
    };

    let app_core = controller.app_core().clone();
    let rerender_for_cancel = rerender.clone();
    spawn(async move {
        match ceremony_workflows::cancel_key_rotation_ceremony(
            &app_core,
            &CeremonyId::new(ceremony_id),
        )
        .await
        {
            Ok(()) => controller.complete_runtime_modal_success("Device enrollment canceled"),
            Err(error) => controller.runtime_error_toast(error.to_string()),
        }
        rerender_for_cancel();
    });
    true
}

fn submit_runtime_chat_input(
    controller: Arc<UiController>,
    channel_name: String,
    input_text: String,
    rerender: Arc<dyn Fn() + Send + Sync>,
) -> bool {
    let trimmed = input_text.trim().to_string();
    if trimmed.is_empty() {
        return false;
    }

    let app_core = controller.app_core().clone();
    let controller_for_task = controller.clone();
    spawn(async move {
        let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
            Ok(value) => value,
            Err(error) => {
                controller_for_task.runtime_error_toast(error.to_string());
                rerender();
                return;
            }
        };

        let result = if let Some(command_input) = trimmed.strip_prefix('/') {
            let raw = format!("/{command_input}");
            match aura_app::ui::types::parse_chat_command(&raw) {
                Ok(aura_app::ui::types::ChatCommand::Join { channel }) => {
                    messaging_workflows::join_channel_by_name(&app_core, &channel).await
                }
                Ok(aura_app::ui::types::ChatCommand::Leave) => {
                    messaging_workflows::leave_channel_by_name(&app_core, &channel_name).await
                }
                Ok(aura_app::ui::types::ChatCommand::Topic { text }) => {
                    messaging_workflows::set_topic_by_name(
                        &app_core,
                        &channel_name,
                        &text,
                        timestamp_ms,
                    )
                    .await
                }
                Ok(aura_app::ui::types::ChatCommand::Me { action }) => {
                    messaging_workflows::send_action_by_name(
                        &app_core,
                        &channel_name,
                        &action,
                        timestamp_ms,
                    )
                    .await
                    .map(|_| ())
                }
                Ok(aura_app::ui::types::ChatCommand::Msg { target, text }) => {
                    messaging_workflows::send_direct_message(
                        &app_core,
                        &target,
                        &text,
                        timestamp_ms,
                    )
                    .await
                    .map(|_| ())
                }
                Ok(aura_app::ui::types::ChatCommand::Invite { target }) => {
                    messaging_workflows::invite_user_to_channel(
                        &app_core,
                        &target,
                        &channel_name,
                        None,
                        None,
                    )
                    .await
                    .map(|_| ())
                }
                Ok(command) => Err(aura_core::AuraError::agent(format!(
                    "Command not wired in web chat yet: {}",
                    command.name()
                ))),
                Err(error) => Err(aura_core::AuraError::invalid(error.to_string())),
            }
        } else {
            messaging_workflows::send_message_by_name(
                &app_core,
                &channel_name,
                &trimmed,
                timestamp_ms,
            )
            .await
            .map(|_| ())
        };

        match result {
            Ok(()) => controller_for_task.clear_input_buffer(),
            Err(error) => controller_for_task.runtime_error_toast(error.to_string()),
        }
        rerender();
    });

    controller.clear_input_buffer();
    true
}

#[component]
pub fn AuraUiRoot(controller: Arc<UiController>) -> Element {
    rsx! {
        ThemeProvider {
            theme: themes::neutral(),
            color_scheme: ColorScheme::Dark,
            div {
                style: "--normal-bg: var(--popover); --normal-text: var(--popover-foreground); --normal-border: var(--border);",
                ToastProvider {
                    default_duration: Duration::from_secs(5),
                    max_toasts: 8,
                    position: ToastPosition::BottomLeft,
                    AuraUiShell { controller }
                }
            }
        }
    }
}

#[component]
fn AuraUiShell(controller: Arc<UiController>) -> Element {
    let mut render_tick = use_signal(|| 0_u64);
    let _render_tick_value = render_tick();
    let mut last_toast_key = use_signal(|| None::<String>);
    let mut runtime_bridge_started = use_signal(|| false);
    let neighborhood_runtime = use_signal(NeighborhoodRuntimeView::default);
    let chat_runtime = use_signal(ChatRuntimeView::default);
    let contacts_runtime = use_signal(ContactsRuntimeView::default);
    let settings_runtime = use_signal(SettingsRuntimeView::default);
    let notifications_runtime = use_signal(NotificationsRuntimeView::default);
    let toasts = use_toast();
    let theme = use_theme();

    let Some(model) = controller.ui_model() else {
        return rsx! {
            main {
                class: "min-h-screen bg-background text-foreground grid place-items-center",
                p { "UI state unavailable" }
            }
        };
    };

    let modal = modal_view(&model);
    let controller_for_toast = controller.clone();
    let controller_for_runtime = controller.clone();

    use_effect(move || {
        let _ = render_tick();
        let Some(current_model) = controller_for_toast.ui_model() else {
            return;
        };
        let next_key = current_model.toast.as_ref().map(|toast| {
            format!(
                "{}::{}::{}",
                current_model.toast_key, toast.icon, toast.message
            )
        });

        if last_toast_key() == next_key {
            return;
        }

        if let Some(toast) = &current_model.toast {
            let opts = Some(ToastOptions {
                description: None,
                duration: Some(Duration::from_secs(5)),
                permanent: false,
                action: None,
                on_dismiss: None,
            });

            match toast.icon {
                'Y' | 'y' | '+' | '✓' => toasts.success(toast.message.clone(), opts),
                'X' | 'x' | '-' | '!' | '✗' => toasts.error(toast.message.clone(), opts),
                _ => toasts.info(toast.message.clone(), opts),
            };
        }

        last_toast_key.set(next_key);
    });

    use_effect(move || {
        if runtime_bridge_started() {
            return;
        }

        runtime_bridge_started.set(true);

        let mut runtime_for_initial = neighborhood_runtime;
        let controller_for_initial = controller_for_runtime.clone();
        spawn(async move {
            runtime_for_initial.set(load_neighborhood_runtime_view(controller_for_initial).await);
        });

        let mut chat_for_initial = chat_runtime;
        let controller_for_chat_initial = controller_for_runtime.clone();
        spawn(async move {
            chat_for_initial.set(load_chat_runtime_view(controller_for_chat_initial).await);
        });

        let mut contacts_for_initial = contacts_runtime;
        let controller_for_contacts_initial = controller_for_runtime.clone();
        spawn(async move {
            contacts_for_initial
                .set(load_contacts_runtime_view(controller_for_contacts_initial).await);
        });

        let mut settings_for_initial = settings_runtime;
        let controller_for_settings_initial = controller_for_runtime.clone();
        spawn(async move {
            settings_for_initial
                .set(load_settings_runtime_view(controller_for_settings_initial).await);
        });

        let mut notifications_for_initial = notifications_runtime;
        let controller_for_notifications_initial = controller_for_runtime.clone();
        spawn(async move {
            notifications_for_initial
                .set(load_notifications_runtime_view(controller_for_notifications_initial).await);
        });

        let mut runtime_for_neighborhood = neighborhood_runtime;
        let controller_for_neighborhood = controller_for_runtime.clone();
        spawn(async move {
            let mut stream = {
                let core = controller_for_neighborhood.app_core().read().await;
                core.subscribe(&*NEIGHBORHOOD_SIGNAL)
            };

            while stream.recv().await.is_ok() {
                runtime_for_neighborhood
                    .set(load_neighborhood_runtime_view(controller_for_neighborhood.clone()).await);
            }
        });

        let mut runtime_for_homes = neighborhood_runtime;
        let controller_for_homes = controller_for_runtime.clone();
        spawn(async move {
            let mut stream = {
                let core = controller_for_homes.app_core().read().await;
                core.subscribe(&*HOMES_SIGNAL)
            };

            while stream.recv().await.is_ok() {
                runtime_for_homes
                    .set(load_neighborhood_runtime_view(controller_for_homes.clone()).await);
            }
        });

        let mut runtime_for_contacts = neighborhood_runtime;
        let controller_for_contacts = controller_for_runtime.clone();
        spawn(async move {
            let mut stream = {
                let core = controller_for_contacts.app_core().read().await;
                core.subscribe(&*CONTACTS_SIGNAL)
            };

            while stream.recv().await.is_ok() {
                runtime_for_contacts
                    .set(load_neighborhood_runtime_view(controller_for_contacts.clone()).await);
            }
        });

        let mut contacts_for_contacts_signal = contacts_runtime;
        let controller_for_contacts_signal = controller_for_runtime.clone();
        spawn(async move {
            let mut stream = {
                let core = controller_for_contacts_signal.app_core().read().await;
                core.subscribe(&*CONTACTS_SIGNAL)
            };

            while stream.recv().await.is_ok() {
                contacts_for_contacts_signal
                    .set(load_contacts_runtime_view(controller_for_contacts_signal.clone()).await);
            }
        });

        let mut runtime_for_chat = neighborhood_runtime;
        let controller_for_chat = controller_for_runtime.clone();
        spawn(async move {
            let mut stream = {
                let core = controller_for_chat.app_core().read().await;
                core.subscribe(&*CHAT_SIGNAL)
            };

            while stream.recv().await.is_ok() {
                runtime_for_chat
                    .set(load_neighborhood_runtime_view(controller_for_chat.clone()).await);
            }
        });

        let mut chat_for_chat_signal = chat_runtime;
        let controller_for_chat_signal = controller_for_runtime.clone();
        spawn(async move {
            let mut stream = {
                let core = controller_for_chat_signal.app_core().read().await;
                core.subscribe(&*CHAT_SIGNAL)
            };

            while stream.recv().await.is_ok() {
                chat_for_chat_signal
                    .set(load_chat_runtime_view(controller_for_chat_signal.clone()).await);
            }
        });

        let mut runtime_for_network = neighborhood_runtime;
        let controller_for_network = controller_for_runtime.clone();
        spawn(async move {
            let mut stream = {
                let core = controller_for_network.app_core().read().await;
                core.subscribe(&*NETWORK_STATUS_SIGNAL)
            };

            while stream.recv().await.is_ok() {
                runtime_for_network
                    .set(load_neighborhood_runtime_view(controller_for_network.clone()).await);
            }
        });

        let mut runtime_for_transport = neighborhood_runtime;
        let controller_for_transport = controller_for_runtime.clone();
        spawn(async move {
            let mut stream = {
                let core = controller_for_transport.app_core().read().await;
                core.subscribe(&*TRANSPORT_PEERS_SIGNAL)
            };

            while stream.recv().await.is_ok() {
                runtime_for_transport
                    .set(load_neighborhood_runtime_view(controller_for_transport.clone()).await);
            }
        });

        let mut settings_for_settings_signal = settings_runtime;
        let controller_for_settings_signal = controller_for_runtime.clone();
        spawn(async move {
            let mut stream = {
                let core = controller_for_settings_signal.app_core().read().await;
                core.subscribe(&*SETTINGS_SIGNAL)
            };

            while stream.recv().await.is_ok() {
                settings_for_settings_signal
                    .set(load_settings_runtime_view(controller_for_settings_signal.clone()).await);
            }
        });

        let mut settings_for_recovery_signal = settings_runtime;
        let controller_for_recovery_signal = controller_for_runtime.clone();
        spawn(async move {
            let mut stream = {
                let core = controller_for_recovery_signal.app_core().read().await;
                core.subscribe(&*RECOVERY_SIGNAL)
            };

            while stream.recv().await.is_ok() {
                settings_for_recovery_signal
                    .set(load_settings_runtime_view(controller_for_recovery_signal.clone()).await);
            }
        });

        let mut notifications_for_invites = notifications_runtime;
        let controller_for_invites = controller_for_runtime.clone();
        spawn(async move {
            let mut stream = {
                let core = controller_for_invites.app_core().read().await;
                core.subscribe(&*INVITATIONS_SIGNAL)
            };

            while stream.recv().await.is_ok() {
                notifications_for_invites
                    .set(load_notifications_runtime_view(controller_for_invites.clone()).await);
            }
        });

        let mut notifications_for_recovery = notifications_runtime;
        let controller_for_notifications_recovery = controller_for_runtime.clone();
        spawn(async move {
            let mut stream = {
                let core = controller_for_notifications_recovery
                    .app_core()
                    .read()
                    .await;
                core.subscribe(&*RECOVERY_SIGNAL)
            };

            while stream.recv().await.is_ok() {
                notifications_for_recovery.set(
                    load_notifications_runtime_view(controller_for_notifications_recovery.clone())
                        .await,
                );
            }
        });

        let mut notifications_for_errors = notifications_runtime;
        let controller_for_errors = controller_for_runtime.clone();
        spawn(async move {
            let mut stream = {
                let core = controller_for_errors.app_core().read().await;
                core.subscribe(&*ERROR_SIGNAL)
            };

            while stream.recv().await.is_ok() {
                notifications_for_errors
                    .set(load_notifications_runtime_view(controller_for_errors.clone()).await);
            }
        });
    });

    let resolved_scheme = theme.resolved_scheme();
    let runtime_snapshot = neighborhood_runtime();
    let chat_runtime_snapshot = chat_runtime();
    let contacts_runtime_snapshot = contacts_runtime();
    let settings_runtime_snapshot = settings_runtime();
    let notifications_runtime_snapshot = notifications_runtime();
    let footer_network_status = if runtime_snapshot.loaded {
        runtime_snapshot.network_status.clone()
    } else {
        format_network_status_with_severity(&NetworkStatus::Disconnected, None).0
    };
    let footer_peer_count = if runtime_snapshot.loaded {
        runtime_snapshot.transport_peers.to_string()
    } else {
        "0".to_string()
    };
    let footer_online_count = if runtime_snapshot.loaded {
        runtime_snapshot.online_contacts.to_string()
    } else {
        "0".to_string()
    };
    let keydown_runtime_snapshot = runtime_snapshot.clone();
    let keydown_chat_runtime = chat_runtime_snapshot.clone();
    let keydown_contacts_runtime = contacts_runtime_snapshot.clone();
    let keydown_settings_runtime = settings_runtime_snapshot.clone();
    let keydown_model = model.clone();
    let modal_runtime_snapshot = runtime_snapshot.clone();
    let modal_chat_runtime = chat_runtime_snapshot.clone();
    let modal_contacts_runtime = contacts_runtime_snapshot.clone();
    let modal_settings_runtime = settings_runtime_snapshot.clone();
    let modal_model = model.clone();
    let cancel_add_device_ceremony_id = model.add_device_ceremony_id.clone();
    let rerender = schedule_update();
    let keydown_rerender = rerender.clone();
    let cancel_rerender = rerender.clone();
    let dedicated_primary_rerender = rerender.clone();
    let generic_confirm_rerender = rerender.clone();

    rsx! {
        main {
            class: "relative flex h-[100dvh] min-h-[100dvh] flex-col overflow-hidden bg-background text-foreground font-mono outline-none",
            tabindex: 0,
            autofocus: true,
            onmounted: move |mounted| {
                spawn(async move {
                    let _ = mounted.data().set_focus(true).await;
                });
            },
            onkeydown: move |event| {
                if should_skip_global_key(controller.as_ref(), event.data().as_ref()) {
                    return;
                }
                if matches!(event.data().key(), Key::Enter)
                    && matches!(model.screen, UiScreen::Chat)
                    && model.input_mode
                    && submit_runtime_chat_input(
                        controller.clone(),
                        chat_runtime_snapshot.active_channel.clone(),
                        model.input_buffer.clone(),
                        keydown_rerender.clone(),
                    )
                {
                    event.prevent_default();
                    return;
                }
                if matches!(event.data().key(), Key::Enter)
                    && submit_runtime_modal_action(
                                controller.clone(),
                                model.modal,
                                model.add_device_step,
                                keydown_model.add_device_ceremony_id.clone(),
                                model.add_device_is_complete,
                                model.add_device_has_failed,
                                model.modal_buffer.clone(),
                        keydown_runtime_snapshot.clone(),
                        keydown_chat_runtime.clone(),
                        keydown_contacts_runtime.clone(),
                        keydown_settings_runtime.clone(),
                        selected_home_id_for_modal(&keydown_runtime_snapshot, &keydown_model),
                        model.selected_neighborhood_member_index,
                        keydown_rerender.clone(),
                    )
                {
                    event.prevent_default();
                    return;
                }
                if handle_keydown(controller.as_ref(), event.data().as_ref()) {
                    event.prevent_default();
                    render_tick.set(render_tick() + 1);
                }
            },
            nav {
                class: "border-b border-border bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/80",
                div {
                    class: "relative flex items-center px-4 py-3 sm:px-6",
                    div {
                        class: "absolute left-4 top-1/2 z-10 flex -translate-y-1/2 items-center justify-start gap-3 sm:left-6",
                        span { class: "inline-flex h-9 items-center text-xs font-bold uppercase tracking-[0.12em] text-foreground", "AURA" }
                    }
                    div {
                        class: "w-full min-w-0 overflow-x-auto px-16 [::-webkit-scrollbar]:hidden sm:px-24",
                        div {
                            class: "flex min-w-max h-9 items-center justify-center gap-2 mx-auto",
                            for (screen, label, is_active) in screen_tabs(model.screen) {
                                button {
                                    r#type: "button",
                                    class: nav_tab_class(is_active),
                                    onclick: {
                                        let controller = controller.clone();
                                        move |_| {
                                            controller.set_screen(screen);
                                            render_tick.set(render_tick() + 1);
                                        }
                                    },
                                    "{label}"
                                }
                            }
                        }
                    }
                }
            }
            div {
                class: "flex-1 min-h-0 overflow-hidden px-4 py-4 sm:px-6 sm:py-5",
                {render_screen_content(
                    &model,
                    &runtime_snapshot,
                    &chat_runtime_snapshot,
                    &contacts_runtime_snapshot,
                    &settings_runtime_snapshot,
                    &notifications_runtime_snapshot,
                    controller.clone(),
                    render_tick,
                    theme.clone(),
                    resolved_scheme,
                )}
            }

            if let Some(modal) = modal {
                if matches!(model.modal, Some(ModalState::AddDeviceStep1))
                    && !matches!(model.add_device_step, AddDeviceWizardStep::Name)
                {
                    UiDeviceEnrollmentModal {
                        title: if matches!(model.add_device_step, AddDeviceWizardStep::ShareCode) {
                            "Add Device — Step 2 of 3".to_string()
                        } else {
                            "Add Device — Step 3 of 3".to_string()
                        },
                        enrollment_code: model.add_device_enrollment_code.clone(),
                        ceremony_id: model.add_device_ceremony_id.clone(),
                        device_name: model.add_device_name.clone(),
                        accepted_count: model.add_device_accepted_count,
                        total_count: model.add_device_total_count,
                        threshold: model.add_device_threshold,
                        is_complete: model.add_device_is_complete,
                        has_failed: model.add_device_has_failed,
                        error_message: model.add_device_error_message.clone(),
                        copied: model.add_device_code_copied,
                        primary_label: if matches!(model.add_device_step, AddDeviceWizardStep::ShareCode) {
                            "Next".to_string()
                        } else if model.add_device_is_complete || model.add_device_has_failed {
                            "Close".to_string()
                        } else {
                            "Refresh".to_string()
                        },
                        on_cancel: {
                            let controller = controller.clone();
                            move |_| {
                                if !cancel_runtime_modal_action(
                                    controller.clone(),
                                    model.modal,
                                    model.add_device_step,
                                    cancel_add_device_ceremony_id.clone(),
                                    model.add_device_is_complete,
                                    model.add_device_has_failed,
                                    cancel_rerender.clone(),
                                ) {
                                    controller.send_key_named("esc", 1);
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        },
                        on_copy: {
                            let controller = controller.clone();
                            let enrollment_code = model.add_device_enrollment_code.clone();
                            move |_| {
                                controller.write_clipboard(&enrollment_code);
                                controller.mark_add_device_code_copied();
                                controller.info_toast("Copied to clipboard");
                                render_tick.set(render_tick() + 1);
                            }
                        },
                        on_primary: {
                            let controller = controller.clone();
                            let modal_state = model.modal;
                            let modal_buffer = model.modal_buffer.clone();
                            move |_| {
                                if !submit_runtime_modal_action(
                                    controller.clone(),
                                    modal_state,
                                    model.add_device_step,
                                    modal_model.add_device_ceremony_id.clone(),
                                    model.add_device_is_complete,
                                    model.add_device_has_failed,
                                    modal_buffer.clone(),
                                    modal_runtime_snapshot.clone(),
                                    modal_chat_runtime.clone(),
                                    modal_contacts_runtime.clone(),
                                    modal_settings_runtime.clone(),
                                    selected_home_id_for_modal(&modal_runtime_snapshot, &modal_model),
                                    model.selected_neighborhood_member_index,
                                    dedicated_primary_rerender.clone(),
                                ) {
                                    controller.send_key_named("enter", 1);
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                    }
                } else if matches!(model.modal, Some(ModalState::SwitchAuthority)) {
                    UiAuthorityPickerModal {
                        title: active_modal_title(&model)
                            .unwrap_or_else(|| "Switch Authority".to_string()),
                        current_label: settings_runtime_snapshot
                            .authorities
                            .iter()
                            .find(|authority| authority.is_current)
                            .map(|authority| authority.label.clone())
                            .unwrap_or_else(|| "Current Authority".to_string()),
                        current_id: settings_runtime_snapshot.authority_id.clone(),
                        mfa_policy: settings_runtime_snapshot.mfa_policy.clone(),
                        authorities: model
                            .authorities
                            .iter()
                            .map(|authority| AuthorityPickerItem {
                                id: authority.id.clone(),
                                label: authority.label.clone(),
                                is_current: authority.is_current,
                                is_selected: authority.selected,
                            })
                            .collect(),
                        on_cancel: {
                            let controller = controller.clone();
                            move |_| {
                                controller.send_key_named("esc", 1);
                                render_tick.set(render_tick() + 1);
                            }
                        },
                        on_select: {
                            let controller = controller.clone();
                            move |index| {
                                controller.set_selected_authority_index(index);
                                render_tick.set(render_tick() + 1);
                            }
                        },
                        on_confirm: {
                            let controller = controller.clone();
                            let modal_state = model.modal;
                            let modal_buffer = model.modal_buffer.clone();
                            move |_| {
                                if !submit_runtime_modal_action(
                                    controller.clone(),
                                    modal_state,
                                    model.add_device_step,
                                    modal_model.add_device_ceremony_id.clone(),
                                    model.add_device_is_complete,
                                    model.add_device_has_failed,
                                    modal_buffer.clone(),
                                    modal_runtime_snapshot.clone(),
                                    modal_chat_runtime.clone(),
                                    modal_contacts_runtime.clone(),
                                    modal_settings_runtime.clone(),
                                    selected_home_id_for_modal(&modal_runtime_snapshot, &modal_model),
                                    model.selected_neighborhood_member_index,
                                    dedicated_primary_rerender.clone(),
                                ) {
                                    controller.send_key_named("enter", 1);
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                    }
                } else {
                    UiModal {
                        modal,
                        on_cancel: {
                            let controller = controller.clone();
                            move |_| {
                                if !cancel_runtime_modal_action(
                                    controller.clone(),
                                    model.modal,
                                    model.add_device_step,
                                    cancel_add_device_ceremony_id.clone(),
                                    model.add_device_is_complete,
                                    model.add_device_has_failed,
                                    cancel_rerender.clone(),
                                ) {
                                    controller.send_key_named("esc", 1);
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        },
                        on_confirm: {
                            let controller = controller.clone();
                            let modal_state = model.modal;
                            let modal_buffer = model.modal_buffer.clone();
                            move |_| {
                                if !submit_runtime_modal_action(
                                    controller.clone(),
                                    modal_state,
                                    model.add_device_step,
                                    modal_model.add_device_ceremony_id.clone(),
                                    model.add_device_is_complete,
                                    model.add_device_has_failed,
                                    modal_buffer.clone(),
                                    modal_runtime_snapshot.clone(),
                                    modal_chat_runtime.clone(),
                                    modal_contacts_runtime.clone(),
                                    modal_settings_runtime.clone(),
                                    selected_home_id_for_modal(&modal_runtime_snapshot, &modal_model),
                                    model.selected_neighborhood_member_index,
                                    generic_confirm_rerender.clone(),
                                ) {
                                    controller.send_key_named("enter", 1);
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        },
                        on_input_change: {
                            let controller = controller.clone();
                            move |value: String| {
                                controller.set_modal_buffer(&value);
                                render_tick.set(render_tick() + 1);
                            }
                        }
                    }
                }
            }

            UiFooter {
                left: String::new(),
                network_status: footer_network_status,
                peer_count: footer_peer_count,
                online_count: footer_online_count,
            }
        }
    }
}

fn neighborhood_screen(
    model: &UiModel,
    runtime: &NeighborhoodRuntimeView,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
) -> Element {
    let is_detail = matches!(model.neighborhood_mode, NeighborhoodMode::Detail);
    let selected_home = model
        .selected_home
        .clone()
        .or_else(|| {
            if !runtime.active_home_name.is_empty() {
                Some(runtime.active_home_name.clone())
            } else if !runtime.neighborhood_name.is_empty() {
                Some(runtime.neighborhood_name.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "Neighborhood".to_string());
    let access_label = model.access_depth.label().to_string();
    let access_tone = match model.access_depth.label() {
        "Full" => PillTone::Success,
        "Partial" => PillTone::Info,
        _ => PillTone::Neutral,
    };
    let hop_hint = neighborhood_hop_hint(model.access_depth.label());
    let show_detail_lists = is_detail && matches!(model.access_depth, AccessDepth::Full);
    let mut home_rows = runtime.homes.clone();
    if home_rows.is_empty() {
        if let Some(home) = model.selected_home.clone() {
            home_rows.push(NeighborhoodRuntimeHome {
                id: format!("home-{}", home.to_lowercase().replace(' ', "-")),
                name: home,
                member_count: None,
                can_enter: true,
                is_local: false,
            });
        }
    }
    let should_materialize_selected_home =
        model.selected_home.is_some() || !runtime.homes.is_empty();
    if should_materialize_selected_home && !home_rows.iter().any(|home| home.name == selected_home)
    {
        home_rows.push(NeighborhoodRuntimeHome {
            id: if selected_home == "Neighborhood" {
                model.authority_id.clone()
            } else {
                format!("home-{}", selected_home.to_lowercase().replace(' ', "-"))
            },
            name: selected_home.clone(),
            member_count: None,
            can_enter: true,
            is_local: selected_home == "Neighborhood",
        });
    }
    home_rows.sort_by(|left, right| {
        right
            .is_local
            .cmp(&left.is_local)
            .then_with(|| left.name.cmp(&right.name))
    });
    home_rows.dedup_by(|left, right| left.name == right.name);

    let display_members = if !runtime.members.is_empty() {
        runtime.members.clone()
    } else {
        let mut members = vec![NeighborhoodRuntimeMember {
            authority_id: model.authority_id.clone(),
            name: model.profile_nickname.clone(),
            role_label: "Member".to_string(),
            is_self: true,
            is_online: true,
            is_moderator: false,
        }];
        members.extend(
            model
                .contacts
                .iter()
                .map(|contact| NeighborhoodRuntimeMember {
                    authority_id: String::new(),
                    name: contact.name.clone(),
                    role_label: "Participant".to_string(),
                    is_self: false,
                    is_online: false,
                    is_moderator: false,
                }),
        );
        members
    };
    let member_count = display_members.len();
    let selected_member_index = model
        .selected_neighborhood_member_index
        .min(display_members.len().saturating_sub(1));
    let selected_runtime_member = display_members.get(selected_member_index).cloned();

    let selected_channel_name = model.selected_channel_name().unwrap_or("none").to_string();
    let display_channels = if !runtime.channels.is_empty() {
        let has_selected = runtime
            .channels
            .iter()
            .any(|channel| channel.name == selected_channel_name);
        runtime
            .channels
            .iter()
            .enumerate()
            .map(|(idx, channel)| {
                let is_selected = if has_selected {
                    channel.name == selected_channel_name
                } else {
                    idx == 0
                };
                (channel.name.clone(), channel.topic.clone(), is_selected)
            })
            .collect::<Vec<_>>()
    } else {
        model
            .channels
            .iter()
            .map(|channel| {
                (
                    channel.name.clone(),
                    channel.topic.clone(),
                    channel.selected,
                )
            })
            .collect::<Vec<_>>()
    };
    let selected_channel_name = display_channels
        .iter()
        .find(|(_, _, selected)| *selected)
        .map(|(name, _, _)| name.clone())
        .unwrap_or_else(|| "none".to_string());
    let social_mode_label = if is_detail { "Entered" } else { "Browsing" };
    let selected_home_id = home_rows
        .iter()
        .find(|home| home.name == selected_home)
        .map(|home| home.id.clone())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| {
            if selected_home == "Neighborhood" {
                model.authority_id.clone()
            } else if !runtime.active_home_id.is_empty()
                && selected_home == runtime.active_home_name
            {
                runtime.active_home_id.clone()
            } else {
                format!("home-{}", selected_home.to_lowercase().replace(' ', "-"))
            }
        });
    let display_neighborhood_name = if !runtime.neighborhood_name.is_empty() {
        runtime.neighborhood_name.clone()
    } else {
        "Neighborhood".to_string()
    };
    let selected_runtime_home = runtime.homes.iter().find(|home| home.name == selected_home);
    let enter_target_home_id = selected_runtime_home
        .map(|home| home.id.clone())
        .or_else(|| {
            if !runtime.active_home_id.is_empty() && runtime.active_home_name == selected_home {
                Some(runtime.active_home_id.clone())
            } else {
                None
            }
        });
    let can_enter_selected_home = selected_runtime_home
        .map(|home| home.can_enter)
        .unwrap_or_else(|| enter_target_home_id.is_some());
    let detail_back_controller = controller.clone();
    let detail_depth_controller = controller.clone();
    let detail_moderator_controller = controller.clone();
    let detail_access_override_controller = controller.clone();
    let detail_capability_controller = controller.clone();
    let map_enter_controller = controller.clone();
    let map_new_home_controller = controller.clone();
    let map_accept_invitation_controller = controller.clone();
    let map_depth_controller = controller.clone();

    rsx! {
        div {
            class: "grid h-full min-h-0 w-full gap-3 lg:grid-cols-12 lg:[grid-template-rows:minmax(0,1fr)]",
            UiCard {
                title: if is_detail { "Home".to_string() } else { "Map".to_string() },
                subtitle: Some(if is_detail {
                    format!("Access: {} ({hop_hint})", access_label)
                } else {
                    format!("Enter as: {} ({hop_hint})", access_label)
                }),
                extra_class: Some("lg:col-span-4".to_string()),
                if is_detail {
                    div {
                        class: "flex flex-1 min-h-0 flex-col gap-3",
                        div {
                            class: "rounded-lg border border-border bg-background/60 px-3 py-3",
                            p { class: "m-0 text-xs font-semibold uppercase tracking-[0.08em] text-muted-foreground", "Home" }
                            p { class: "m-0 mt-1 text-sm text-foreground", "Name: {selected_home}" }
                            p {
                                class: "m-0 mt-1 text-xs text-muted-foreground",
                                "Members/Participants: {member_count} • Access: {access_label} • Mode: {social_mode_label}"
                            }
                        }
                        if show_detail_lists {
                            div {
                                class: "grid flex-1 min-h-0 gap-3 md:grid-cols-2",
                                div {
                                    class: "flex min-h-0 flex-col rounded-lg border border-border bg-background/60 px-3 py-3",
                                    p { class: "m-0 text-xs font-semibold uppercase tracking-[0.08em] text-muted-foreground", "Channels" }
                                    div {
                                        class: "mt-3 flex-1 min-h-0 overflow-y-auto pr-1",
                                        if display_channels.is_empty() {
                                            p { class: "m-0 text-sm text-muted-foreground", "No channels" }
                                        } else {
                                            div { class: "space-y-2",
                                                for (channel_name, channel_topic, is_selected) in &display_channels {
                                                    button {
                                                        r#type: "button",
                                                        class: "block w-full text-left",
                                                        onclick: {
                                                            let controller = controller.clone();
                                                            let channel_name = channel_name.clone();
                                                            move |_| {
                                                                controller.select_channel_by_name(&channel_name);
                                                                render_tick.set(render_tick() + 1);
                                                            }
                                                        },
                                                        UiListItem {
                                                            label: format!("# {}", channel_name),
                                                            secondary: Some(if channel_topic.is_empty() {
                                                                "\u{00A0}".to_string()
                                                            } else {
                                                                channel_topic.clone()
                                                            }),
                                                            active: *is_selected,
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                div {
                                    class: "flex min-h-0 flex-col rounded-lg border border-border bg-background/60 px-3 py-3",
                                    p { class: "m-0 text-xs font-semibold uppercase tracking-[0.08em] text-muted-foreground", "Members & Participants" }
                                    div {
                                        class: "mt-3 flex-1 min-h-0 overflow-y-auto pr-1",
                                        div { class: "space-y-2",
                                            for (idx, member) in display_members.iter().enumerate() {
                                                button {
                                                    r#type: "button",
                                                    class: "block w-full text-left",
                                                    onclick: {
                                                        let controller = controller.clone();
                                                        move |_| {
                                                            controller.set_selected_neighborhood_member_index(idx);
                                                            render_tick.set(render_tick() + 1);
                                                        }
                                                    },
                                                    UiListItem {
                                                        label: if member.is_self {
                                                            format!("{} (you)", member.name)
                                                        } else {
                                                            member.name.clone()
                                                        },
                                                        secondary: Some(if member.is_online {
                                                            format!("{} • online", member.role_label)
                                                        } else {
                                                            member.role_label.clone()
                                                        }),
                                                        active: idx == selected_member_index,
                                                    }
                                                }
                                            }
                                            if display_members.is_empty() {
                                                UiListItem {
                                                    label: "No other members or participants".to_string(),
                                                    secondary: Some("Invite or join another home to populate this view.".to_string()),
                                                    active: false,
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            div {
                                class: "flex flex-1 items-center justify-center rounded-lg border border-border bg-background/40 px-4 py-6 text-center",
                                p {
                                    class: "m-0 text-sm text-muted-foreground",
                                    "Partial/Limited view: full channel and membership details are hidden until Full access is active."
                                }
                            }
                        }
                        div { class: "mt-auto flex flex-wrap gap-2 border-t border-border pt-4",
                            UiButton {
                                label: "Back To Map".to_string(),
                                variant: ButtonVariant::Secondary,
                                on_click: move |_| {
                                    detail_back_controller.send_key_named("esc", 1);
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                            UiButton {
                                label: format!("Enter As: {access_label}"),
                                variant: ButtonVariant::Secondary,
                                on_click: move |_| {
                                    detail_depth_controller.send_action_keys("d");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                            if show_detail_lists {
                                UiButton {
                                    label: if selected_runtime_member.as_ref().map(|member| member.is_moderator).unwrap_or(false) {
                                        "Revoke Moderator".to_string()
                                    } else {
                                        "Assign Moderator".to_string()
                                    },
                                    variant: ButtonVariant::Secondary,
                                    on_click: move |_| {
                                        detail_moderator_controller.send_action_keys("o");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                                UiButton {
                                    label: "Access Override Preview".to_string(),
                                    variant: ButtonVariant::Secondary,
                                    on_click: move |_| {
                                        detail_access_override_controller.send_action_keys("x");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                                UiButton {
                                    label: "Capability Preview".to_string(),
                                    variant: ButtonVariant::Secondary,
                                    on_click: move |_| {
                                        detail_capability_controller.send_action_keys("p");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                        }
                    }
                } else {
                    div {
                        class: "flex flex-1 min-h-0 flex-col gap-3",
                        div { class: "flex flex-wrap gap-2",
                            UiPill { label: format!("Access: {access_label}"), tone: access_tone }
                            UiPill {
                                label: if home_rows.is_empty() {
                                    "Known Homes: 0".to_string()
                                } else {
                                    format!("Known Homes: {}", home_rows.len())
                                },
                                tone: PillTone::Neutral
                            }
                        }
                        if home_rows.is_empty() {
                            Empty {
                                class: Some("flex-1 min-h-[16rem] border-border bg-background/40".to_string()),
                                EmptyHeader {
                                    EmptyTitle { "No home yet" }
                                    EmptyDescription { "Create a new home or accept an invitation to join an existing one." }
                                }
                            }
                        } else {
                            div {
                                class: "flex-1 min-h-0 overflow-y-auto pr-1",
                                div { class: "space-y-2",
                                    for home in &home_rows {
                                        button {
                                            r#type: "button",
                                            class: "block w-full text-left",
                                            onclick: {
                                                let controller = controller.clone();
                                                let home_name = home.name.clone();
                                                move |_| {
                                                    controller.select_home_by_name(&home_name);
                                                    render_tick.set(render_tick() + 1);
                                                }
                                            },
                                            UiListItem {
                                                label: home.name.clone(),
                                                secondary: Some(if home.is_local {
                                                    "Local home".to_string()
                                                } else if let Some(member_count) = home.member_count {
                                                    format!(
                                                        "Members/Participants: {}{}",
                                                        member_count,
                                                        if home.can_enter { "" } else { " • traversal unavailable" }
                                                    )
                                                } else if home.can_enter {
                                                    "Neighbor home".to_string()
                                                } else {
                                                    "Neighbor home • traversal unavailable".to_string()
                                                }),
                                                active: selected_home == home.name,
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        div {
                            class: "rounded-lg border border-border bg-background/60 px-3 py-3",
                            p { class: "m-0 text-xs font-semibold uppercase tracking-[0.08em] text-muted-foreground", "Traversal" }
                            p {
                                class: "m-0 mt-1 text-sm text-foreground",
                                "Can enter: Limited, Partial, Full"
                            }
                            p {
                                class: "m-0 mt-1 text-xs text-muted-foreground",
                                "Current depth is {access_label} ({hop_hint}). Select a home, then enter it."
                            }
                        }
                        div { class: "mt-auto flex flex-wrap gap-2 border-t border-border pt-4",
                            if can_enter_selected_home {
                                UiButton {
                                    label: "Enter Home".to_string(),
                                    variant: ButtonVariant::Primary,
                                    on_click: {
                                        let controller = map_enter_controller.clone();
                                        let home_name = selected_home.clone();
                                        let depth = model.access_depth;
                                        let target_home_id = enter_target_home_id.clone();
                                        move |_| {
                                            let Some(target_home_id) = target_home_id.clone() else {
                                                controller.runtime_error_toast("No runtime home selected");
                                                render_tick.set(render_tick() + 1);
                                                return;
                                            };
                                            let controller = controller.clone();
                                            let app_core = controller.app_core().clone();
                                            let mut tick = render_tick;
                                            let home_name = home_name.clone();
                                            spawn(async move {
                                                match context_workflows::move_position(
                                                    &app_core,
                                                    &target_home_id,
                                                    depth.label(),
                                                )
                                                .await
                                                {
                                                    Ok(_) => {
                                                        controller.complete_runtime_enter_home(
                                                            &home_name,
                                                            depth,
                                                        );
                                                    }
                                                    Err(error) => {
                                                        controller.runtime_error_toast(
                                                            error.to_string(),
                                                        );
                                                    }
                                                }
                                                tick.set(tick() + 1);
                                            });
                                        }
                                    }
                                }
                            }
                            UiButton {
                                label: "New Home".to_string(),
                                variant: ButtonVariant::Primary,
                                on_click: move |_| {
                                    map_new_home_controller.send_action_keys("n");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                            UiButton {
                                label: "Accept Invitation".to_string(),
                                variant: ButtonVariant::Secondary,
                                on_click: move |_| {
                                    map_accept_invitation_controller.send_action_keys("a");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                            UiButton {
                                label: format!("Enter As: {access_label}"),
                                variant: ButtonVariant::Secondary,
                                on_click: move |_| {
                                    map_depth_controller.send_action_keys("d");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                    }
                }
            }

            UiCard {
                title: "Social View".to_string(),
                subtitle: Some("Neighborhood status and scope".to_string()),
                extra_class: Some("lg:col-span-8".to_string()),
                div {
                    class: "grid gap-2 md:grid-cols-2",
                    UiListItem {
                        label: format!("Neighborhood: {display_neighborhood_name}"),
                        secondary: Some(format!("Selected home: {selected_home}")),
                        active: true,
                    }
                    UiListItem {
                        label: format!("Home ID: {selected_home_id}"),
                        secondary: Some("Authority-scoped identifier".to_string()),
                        active: false,
                    }
                    UiListItem {
                        label: format!("Access: {access_label} ({hop_hint})"),
                        secondary: Some(format!("{social_mode_label} • {}", model.access_depth.compact())),
                        active: false,
                    }
                    UiListItem {
                        label: format!("Known Homes: {}", home_rows.len()),
                        secondary: Some("Neighborhood graph currently in view".to_string()),
                        active: false,
                    }
                    UiListItem {
                        label: format!("Channels: {}", display_channels.len()),
                        secondary: Some(format!("Focus: #{selected_channel_name}")),
                        active: false,
                    }
                    UiListItem {
                        label: format!("Members/Participants: {member_count}"),
                        secondary: Some(if show_detail_lists {
                            "Full detail available".to_string()
                        } else {
                            "Detail lists hidden outside Full access".to_string()
                        }),
                        active: false,
                    }
                    UiListItem {
                        label: format!("Authority: {}", model.authority_id),
                        secondary: Some("Local identity".to_string()),
                        active: false,
                    }
                    UiListItem {
                        label: "Moderator Actions".to_string(),
                        secondary: Some(if show_detail_lists {
                            "Available in detail view".to_string()
                        } else {
                            "Unavailable".to_string()
                        }),
                        active: false,
                    }
                }
            }
        }
    }
}

fn neighborhood_hop_hint(access_label: &str) -> &'static str {
    match access_label {
        "Full" => "0-hop",
        "Partial" => "1-hop",
        _ => "2+ hops/disconnected",
    }
}

fn chat_screen(
    model: &UiModel,
    runtime: &ChatRuntimeView,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
) -> Element {
    let active_channel = if runtime.active_channel.is_empty() {
        model
            .selected_channel_name()
            .unwrap_or("general")
            .to_string()
    } else {
        runtime.active_channel.clone()
    };
    let topic = runtime
        .channels
        .iter()
        .find(|channel| channel.name.eq_ignore_ascii_case(&active_channel))
        .map(|channel| channel.topic.clone())
        .unwrap_or_else(|| model.selected_channel_topic().to_string());
    let is_input_mode = model.input_mode;
    let mode = if is_input_mode { "insert" } else { "normal" };
    let composer_text = model.input_buffer.clone();
    let new_group_controller = controller.clone();
    let composer_focus_controller = controller.clone();
    let send_message_controller = controller.clone();
    let runtime_channels = if runtime.loaded {
        runtime.channels.clone()
    } else {
        model
            .channels
            .iter()
            .map(|channel| ChatRuntimeChannel {
                name: channel.name.clone(),
                topic: channel.topic.clone(),
                unread_count: 0,
                last_message: None,
            })
            .collect()
    };

    rsx! {
        div {
            class: "grid h-full min-h-0 w-full gap-3 lg:grid-cols-12 lg:[grid-template-rows:minmax(0,1fr)]",
            UiCard {
                title: "Channels".to_string(),
                subtitle: Some(format!("Current: #{active_channel}")),
                extra_class: Some("lg:col-span-4".to_string()),
                ScrollArea {
                    class: Some("flex-1 min-h-0 pr-1".to_string()),
                    ScrollAreaViewport {
                        class: Some("space-y-2".to_string()),
                        for channel in &runtime_channels {
                            button {
                                r#type: "button",
                                class: "block w-full text-left",
                                onclick: {
                                    let controller = controller.clone();
                                    let channel_name = channel.name.clone();
                                    move |_| {
                                        controller.select_channel_by_name(&channel_name);
                                        render_tick.set(render_tick() + 1);
                                    }
                                },
                                UiListItem {
                                    label: format!("# {}", channel.name),
                                    secondary: Some(
                                        channel
                                            .last_message
                                            .clone()
                                            .or_else(|| {
                                                (!channel.topic.is_empty()).then(|| channel.topic.clone())
                                            })
                                            .unwrap_or_else(|| "\u{00A0}".to_string())
                                    ),
                                    active: channel.name.eq_ignore_ascii_case(&active_channel),
                                }
                            }
                        }
                    }
                }
                div { class: "flex gap-2 pt-1",
                    UiButton {
                        label: "New Group".to_string(),
                        variant: ButtonVariant::Primary,
                        on_click: move |_| {
                            new_group_controller.send_action_keys("n");
                            render_tick.set(render_tick() + 1);
                        }
                    }
                }
            }

            UiCard {
                title: "Conversation".to_string(),
                subtitle: Some(format!("Topic: {topic}")),
                extra_class: Some("lg:col-span-8".to_string()),
                div {
                    class: "flex-1 min-h-0 overflow-y-auto pr-1",
                    div {
                        class: "flex min-h-full flex-col justify-end gap-3",
                        if runtime.messages.is_empty() {
                            Empty {
                                class: Some("min-h-full border-border bg-background/40".to_string()),
                                EmptyHeader {
                                    EmptyTitle { "No messages yet" }
                                    EmptyDescription { "Send one from input mode." }
                                }
                            }
                        } else {
                            for message in &runtime.messages {
                                {render_chat_message_bubble(message.clone())}
                            }
                        }
                    }
                }
                div {
                    class: "mt-3 flex items-end gap-3 rounded-xl border border-border bg-background/80 px-3 py-3",
                    div {
                        class: "flex min-w-0 flex-1 flex-col gap-1",
                        div {
                            class: "flex items-center justify-between gap-2",
                            p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Message" }
                            p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Mode: {mode}" }
                        }
                        div {
                            class: "min-h-[4.5rem] rounded-lg border border-border bg-muted/30 px-3 py-2",
                            onclick: move |_| {
                                if !is_input_mode {
                                    composer_focus_controller.send_action_keys("i");
                                    render_tick.set(render_tick() + 1);
                                }
                            },
                            if composer_text.is_empty() {
                                p {
                                    class: "m-0 text-sm text-muted-foreground",
                                    if is_input_mode {
                                        "Type a message and press Enter to send"
                                    } else {
                                        "Press i to start typing"
                                    }
                                }
                            } else {
                                p {
                                    class: "m-0 whitespace-pre-wrap break-words text-sm text-foreground",
                                    "{composer_text}"
                                }
                            }
                        }
                    }
                    UiButton {
                        label: if is_input_mode { "Send".to_string() } else { "Reply".to_string() },
                        variant: ButtonVariant::Primary,
                        on_click: move |_| {
                            if is_input_mode {
                                let _ = submit_runtime_chat_input(
                                    send_message_controller.clone(),
                                    active_channel.clone(),
                                    composer_text.clone(),
                                    schedule_update(),
                                );
                            } else {
                                send_message_controller.send_action_keys("i");
                            }
                            render_tick.set(render_tick() + 1);
                        }
                    }
                }
            }
        }
    }
}

fn render_chat_message_bubble(message: ChatRuntimeMessage) -> Element {
    rsx! {
        div {
            class: if message.is_own {
                "ml-auto flex w-full justify-end"
            } else {
                "mr-auto flex w-full justify-start"
            },
            div {
                class: if message.is_own {
                    "flex max-w-[78%] flex-col items-end gap-1"
                } else {
                    "flex max-w-[78%] flex-col items-start gap-1"
                },
                span {
                    class: "text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground",
                    if message.is_own { "You" } else { {message.sender_name.clone()} }
                }
                div {
                    class: if message.is_own {
                        "rounded-[1.75rem] bg-primary px-5 py-3 text-sm text-primary-foreground shadow-sm"
                    } else {
                        "rounded-[1.75rem] border border-border bg-muted px-5 py-3 text-sm text-foreground shadow-sm"
                    },
                    p {
                        class: "m-0 whitespace-pre-wrap break-words leading-relaxed",
                        {message.content.clone()}
                    }
                }
                if message.is_own {
                    span {
                        class: "text-[0.68rem] text-muted-foreground",
                        {message.delivery_status.clone()}
                    }
                }
            }
        }
    }
}

fn contacts_screen(
    model: &UiModel,
    runtime: &ContactsRuntimeView,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
) -> Element {
    let selected_contact = runtime.contacts.get(model.selected_contact_index).cloned();
    let selected_name = selected_contact
        .as_ref()
        .map(|contact| contact.name.clone())
        .unwrap_or_else(|| "none".to_string());
    let invite_controller = controller.clone();
    let start_chat_controller = controller.clone();
    let edit_controller = controller.clone();
    let remove_controller = controller.clone();

    rsx! {
        div {
            class: "grid h-full min-h-0 w-full gap-3 lg:grid-cols-12 lg:[grid-template-rows:minmax(0,1fr)]",
            UiCard {
                title: format!("Contacts ({})", runtime.contacts.len()),
                subtitle: Some("Contacts share relational context".to_string()),
                extra_class: Some("lg:col-span-4".to_string()),
                div {
                    class: "flex-1 min-h-0",
                    if runtime.contacts.is_empty() {
                        Empty {
                            class: Some("h-full border-border bg-background".to_string()),
                            EmptyHeader {
                                EmptyTitle { "No contacts yet" }
                                EmptyDescription { "Use the invitation flow to add contacts." }
                            }
                        }
                    } else {
                        ScrollArea {
                            class: Some("h-full pr-1".to_string()),
                            ScrollAreaViewport {
                                class: Some("space-y-2".to_string()),
                                for (idx, contact) in runtime.contacts.iter().enumerate() {
                                    button {
                                        r#type: "button",
                                        class: "block w-full text-left",
                                        onclick: {
                                            let controller = controller.clone();
                                            move |_| {
                                                controller.set_selected_contact_index(idx);
                                                render_tick.set(render_tick() + 1);
                                            }
                                        },
                                        UiListItem {
                                            label: contact.name.clone(),
                                            secondary: Some(
                                                if contact.is_guardian {
                                                    "Guardian".to_string()
                                                } else if contact.is_member {
                                                    "Member".to_string()
                                                } else if contact.is_online {
                                                    "Online".to_string()
                                                } else {
                                                    "\u{00A0}".to_string()
                                                }
                                            ),
                                            active: model.selected_contact_index == idx,
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                div { class: "flex gap-2 pt-1",
                    UiButton {
                        label: "Invite".to_string(),
                        variant: ButtonVariant::Primary,
                        on_click: move |_| {
                            invite_controller.send_action_keys("n");
                            render_tick.set(render_tick() + 1);
                        }
                    }
                }
            }

            UiCard {
                title: "Details".to_string(),
                subtitle: Some(format!("Selected: {selected_name}")),
                extra_class: Some("lg:col-span-8".to_string()),
                if let Some(contact) = selected_contact {
                    div {
                        class: "flex flex-1 min-h-0 flex-col gap-2",
                        UiListItem {
                            label: format!("Authority: {}", contact.authority_id),
                            secondary: Some("Relational identity".to_string()),
                            active: false,
                        }
                        UiListItem {
                            label: format!("Name: {}", contact.name),
                            secondary: contact.nickname_hint.clone().or_else(|| Some("No shared nickname suggestion".to_string())),
                            active: false,
                        }
                        UiListItem {
                            label: if contact.is_online { "Status: Online".to_string() } else { "Status: Offline".to_string() },
                            secondary: Some(if contact.is_guardian {
                                "Guardian contact".to_string()
                            } else if contact.is_member {
                                "Home member".to_string()
                            } else {
                                "Direct contact".to_string()
                            }),
                            active: false,
                        }
                        div {
                            class: "mt-auto flex flex-wrap gap-2 border-t border-border pt-4",
                            UiButton {
                                label: "Start Chat".to_string(),
                                variant: ButtonVariant::Primary,
                                on_click: {
                                    let authority_id = contact.authority_id.clone();
                                    let name = contact.name.clone();
                                    move |_| {
                                        let controller = start_chat_controller.clone();
                                        let app_core = controller.app_core().clone();
                                        let authority_id = authority_id.clone();
                                        let name = name.clone();
                                        spawn(async move {
                                            let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
                                                Ok(value) => value,
                                                Err(error) => {
                                                    controller.runtime_error_toast(error.to_string());
                                                    return;
                                                }
                                            };
                                            match messaging_workflows::start_direct_chat(
                                                &app_core,
                                                &authority_id,
                                                timestamp_ms,
                                            ).await {
                                                Ok(_) => {
                                                    controller.set_screen(UiScreen::Chat);
                                                    controller.select_channel_by_name(&name);
                                                }
                                                Err(error) => controller.runtime_error_toast(error.to_string()),
                                            }
                                        });
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                            UiButton {
                                label: "Edit Nickname".to_string(),
                                variant: ButtonVariant::Secondary,
                                on_click: move |_| {
                                    edit_controller.send_action_keys("e");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                            UiButton {
                                label: "Remove Contact".to_string(),
                                variant: ButtonVariant::Secondary,
                                on_click: move |_| {
                                    remove_controller.send_action_keys("r");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                    }
                } else {
                    Empty {
                        class: Some("h-full border-border bg-background".to_string()),
                        EmptyHeader {
                            EmptyTitle { "No contact selected" }
                            EmptyDescription { "Select a contact to inspect identity and relationship details." }
                        }
                    }
                }
            }
        }
    }
}

fn notifications_screen(
    model: &UiModel,
    runtime: &NotificationsRuntimeView,
    _controller: Arc<UiController>,
    _render_tick: Signal<u64>,
) -> Element {
    let selected = runtime
        .items
        .get(
            model
                .selected_notification_index
                .min(runtime.items.len().saturating_sub(1)),
        )
        .cloned();
    rsx! {
        div {
            class: "grid h-full min-h-0 w-full gap-3 lg:grid-cols-12 lg:[grid-template-rows:minmax(0,1fr)]",
            UiCard {
                title: "Notifications".to_string(),
                subtitle: Some("Runtime events".to_string()),
                extra_class: Some("lg:col-span-4".to_string()),
                if runtime.items.is_empty() {
                    Empty {
                        class: Some("h-full border-border bg-background".to_string()),
                        EmptyHeader {
                            EmptyTitle { "No notifications" }
                            EmptyDescription { "Runtime events will appear here." }
                        }
                    }
                } else {
                    ScrollArea {
                        class: Some("flex-1 min-h-0 pr-1".to_string()),
                        ScrollAreaViewport {
                            class: Some("space-y-2".to_string()),
                            for (idx, entry) in runtime.items.iter().enumerate().take(24) {
                                UiListItem {
                                    label: entry.title.clone(),
                                    secondary: Some(entry.kind_label.clone()),
                                    active: idx == model.selected_notification_index,
                                }
                            }
                        }
                    }
                }
            }
            UiCard {
                title: "Details".to_string(),
                subtitle: Some("Selected notification".to_string()),
                extra_class: Some("lg:col-span-8".to_string()),
                if let Some(item) = selected {
                    div {
                        class: "flex flex-1 min-h-0 flex-col gap-2",
                        UiListItem {
                            label: item.kind_label,
                            secondary: Some(item.title),
                            active: false,
                        }
                        UiListItem {
                            label: item.subtitle,
                            secondary: Some(item.detail),
                            active: false,
                        }
                    }
                } else {
                    Empty {
                        class: Some("h-full border-border bg-background".to_string()),
                        EmptyHeader {
                            EmptyTitle { "No notification selected" }
                            EmptyDescription { "Select an item from the list to inspect the latest invitation or recovery activity." }
                        }
                    }
                }
            }
        }
    }
}

fn settings_screen(
    model: &UiModel,
    runtime: &SettingsRuntimeView,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
    mut theme: dioxus_shadcn::theme::ThemeContext,
    resolved_scheme: ColorScheme,
) -> Element {
    rsx! {
        div {
            class: "grid h-full min-h-0 w-full gap-3 lg:grid-cols-12 lg:[grid-template-rows:minmax(0,1fr)]",
            UiCard {
                title: "Settings".to_string(),
                subtitle: Some("Storage: IndexedDB".to_string()),
                extra_class: Some("lg:col-span-4".to_string()),
                for (idx, section) in SETTINGS_ROWS.iter().enumerate() {
                    UiListButton {
                        label: section.to_string(),
                        active: idx == model.settings_index,
                        on_click: {
                            let controller = controller.clone();
                            move |_| {
                                controller.set_settings_index(idx);
                                render_tick.set(render_tick() + 1);
                            }
                        }
                    }
                }
            }

            UiCard {
                title: settings_panel_title(model.settings_index),
                subtitle: Some(settings_panel_subtitle(model.settings_index)),
                extra_class: Some("lg:col-span-8".to_string()),
                if model.settings_index == 0 {
                    div {
                        class: "flex flex-1 min-h-0 flex-col gap-2",
                        UiListItem {
                            label: format!("Nickname: {}", runtime.nickname),
                            secondary: Some("Update display name for this authority".to_string()),
                            active: false,
                        }
                        UiListItem {
                            label: format!("Authority: {}", runtime.authority_id),
                            secondary: Some("local".to_string()),
                            active: false,
                        }
                        div {
                            class: "mt-auto flex flex-wrap gap-2 border-t border-border pt-4",
                            UiButton {
                                label: "Edit Nickname".to_string(),
                                variant: ButtonVariant::Primary,
                                on_click: {
                                    let controller = controller.clone();
                                    move |_| {
                                        controller.set_settings_index(0);
                                        controller.send_action_keys("e");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                        }
                    }
                }
                if model.settings_index == 1 {
                    div {
                        class: "flex flex-1 min-h-0 flex-col gap-2",
                        UiListItem {
                            label: format!("Target threshold: {} of {}", runtime.threshold_k, runtime.threshold_n.max(runtime.guardian_count as u8)),
                            secondary: Some(format!("Configured guardians: {}", runtime.guardian_count)),
                            active: false,
                        }
                        UiListItem {
                            label: format!("Recovery bindings: {}", runtime.guardian_binding_count),
                            secondary: Some("Authorities for which this device can approve recovery".to_string()),
                            active: false,
                        }
                        div {
                            class: "mt-auto flex flex-wrap gap-2 border-t border-border pt-4",
                            UiButton {
                                label: "Configure Threshold".to_string(),
                                variant: ButtonVariant::Primary,
                                on_click: {
                                    let controller = controller.clone();
                                    move |_| {
                                        controller.set_settings_index(1);
                                        controller.send_action_keys("t");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                        }
                    }
                }
                if model.settings_index == 2 {
                    div {
                        class: "flex flex-1 min-h-0 flex-col gap-2",
                        UiListItem {
                            label: "Recovery request".to_string(),
                            secondary: Some("Start guardian-assisted recovery flow".to_string()),
                            active: false,
                        }
                        UiListItem {
                            label: format!("Last status: {}", runtime.active_recovery_label),
                            secondary: Some(format!("Pending approvals to review: {}", runtime.pending_recovery_requests)),
                            active: false,
                        }
                        div {
                            class: "mt-auto flex flex-wrap gap-2 border-t border-border pt-4",
                            UiButton {
                                label: "Request Recovery".to_string(),
                                variant: ButtonVariant::Primary,
                                on_click: {
                                    let controller = controller.clone();
                                    move |_| {
                                        controller.set_settings_index(2);
                                        controller.send_action_keys("s");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                        }
                    }
                }
                if model.settings_index == 3 {
                    div {
                        class: "flex flex-1 min-h-0 flex-col gap-2",
                        UiListItem {
                            label: "Add device".to_string(),
                            secondary: Some("Start device enrollment flow".to_string()),
                            active: false,
                        }
                        for device in &runtime.devices {
                            UiListItem {
                                label: if device.is_current {
                                    format!("{} (current)", device.name)
                                } else {
                                    device.name.clone()
                                },
                                secondary: Some(if device.is_current {
                                    "Local device".to_string()
                                } else {
                                    "Removable secondary device".to_string()
                                }),
                                active: false,
                            }
                        }
                        div {
                            class: "mt-auto flex flex-wrap gap-2 border-t border-border pt-4",
                            UiButton {
                                label: "Add Device".to_string(),
                                variant: ButtonVariant::Primary,
                                on_click: {
                                    let controller = controller.clone();
                                    move |_| {
                                        controller.set_settings_index(3);
                                        controller.send_action_keys("a");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                            UiButton {
                                label: "Import Code".to_string(),
                                variant: ButtonVariant::Secondary,
                                on_click: {
                                    let controller = controller.clone();
                                    move |_| {
                                        controller.set_settings_index(3);
                                        controller.send_action_keys("i");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                            UiButton {
                                label: "Remove Device".to_string(),
                                variant: ButtonVariant::Secondary,
                                on_click: {
                                    let controller = controller.clone();
                                    move |_| {
                                        controller.set_settings_index(3);
                                        controller.send_action_keys("r");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                        }
                    }
                }
                if model.settings_index == 4 {
                    div {
                        class: "flex flex-1 min-h-0 flex-col gap-2",
                        UiListItem {
                            label: format!("Authority ID: {}", runtime.authority_id),
                            secondary: Some("Scope: local authority".to_string()),
                            active: false,
                        }
                        for authority in runtime.authorities.clone() {
                            UiListButton {
                                label: if authority.is_current {
                                    format!("{} (current)", authority.label)
                                } else {
                                    authority.label.clone()
                                },
                                active: authority.is_current,
                                on_click: {
                                    let controller = controller.clone();
                                    let authority_id = authority.id.clone();
                                    move |_| {
                                        if authority.is_current {
                                            return;
                                        }
                                        let _ = controller.request_authority_switch(&authority_id);
                                    }
                                }
                            }
                        }
                        UiListItem {
                            label: "Multifactor".to_string(),
                            secondary: Some(format!("Policy: {}", runtime.mfa_policy)),
                            active: false,
                        }
                        div {
                            class: "mt-auto flex flex-wrap gap-2 border-t border-border pt-4",
                            UiButton {
                                label: "Switch Authority".to_string(),
                                variant: ButtonVariant::Primary,
                                on_click: {
                                    let controller = controller.clone();
                                    move |_| {
                                        controller.set_settings_index(4);
                                        controller.send_action_keys("s");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                            UiButton {
                                label: "Configure MFA".to_string(),
                                variant: ButtonVariant::Secondary,
                                on_click: {
                                    let controller = controller.clone();
                                    move |_| {
                                        controller.set_settings_index(4);
                                        controller.send_action_keys("m");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                        }
                    }
                }
                if model.settings_index == 5 {
                    div {
                        class: "flex flex-1 min-h-0 flex-col gap-2",
                        UiListItem {
                            label: format!(
                                "Color mode: {}",
                                match resolved_scheme {
                                    ColorScheme::Light => "Light",
                                    _ => "Dark",
                                }
                            ),
                            secondary: Some("Switch the current web theme".to_string()),
                            active: false,
                        }
                        UiListItem {
                            label: "Palette".to_string(),
                            secondary: Some("Aura uses the same neutral palette in both modes".to_string()),
                            active: false,
                        }
                        div {
                            class: "mt-auto flex flex-wrap gap-2 border-t border-border pt-4",
                            UiButton {
                                label: match resolved_scheme {
                                    ColorScheme::Light => "Switch to Dark".to_string(),
                                    _ => "Switch to Light".to_string(),
                                },
                                variant: ButtonVariant::Primary,
                                on_click: move |_| {
                                    theme.toggle_color_scheme();
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn settings_panel_title(index: usize) -> String {
    SETTINGS_ROWS
        .get(index)
        .copied()
        .unwrap_or("Settings")
        .to_string()
}

fn settings_panel_subtitle(index: usize) -> String {
    match index {
        0 => "Identity configuration".to_string(),
        1 => "Configure guardian policy".to_string(),
        2 => "Configure recovery operations".to_string(),
        3 => "Device management".to_string(),
        4 => "Authority scope".to_string(),
        5 => "Theme and display".to_string(),
        _ => "Settings details".to_string(),
    }
}

fn screen_tabs(active: UiScreen) -> Vec<(UiScreen, &'static str, bool)> {
    [
        (
            UiScreen::Neighborhood,
            "Neighborhood",
            active == UiScreen::Neighborhood,
        ),
        (UiScreen::Chat, "Chat", active == UiScreen::Chat),
        (UiScreen::Contacts, "Contacts", active == UiScreen::Contacts),
        (
            UiScreen::Notifications,
            "Notifications",
            active == UiScreen::Notifications,
        ),
        (UiScreen::Settings, "Settings", active == UiScreen::Settings),
    ]
    .to_vec()
}

fn nav_tab_class(is_active: bool) -> &'static str {
    if is_active {
        "inline-flex h-9 items-center rounded-md bg-accent px-3 text-xs uppercase tracking-[0.08em] text-foreground"
    } else {
        "inline-flex h-9 items-center rounded-md px-3 text-xs uppercase tracking-[0.08em] text-muted-foreground hover:bg-accent hover:text-foreground"
    }
}

fn render_screen_content(
    model: &UiModel,
    neighborhood_runtime: &NeighborhoodRuntimeView,
    chat_runtime: &ChatRuntimeView,
    contacts_runtime: &ContactsRuntimeView,
    settings_runtime: &SettingsRuntimeView,
    notifications_runtime: &NotificationsRuntimeView,
    controller: Arc<UiController>,
    render_tick: Signal<u64>,
    theme: dioxus_shadcn::theme::ThemeContext,
    resolved_scheme: ColorScheme,
) -> Element {
    match model.screen {
        UiScreen::Neighborhood => {
            neighborhood_screen(model, neighborhood_runtime, controller, render_tick)
        }
        UiScreen::Chat => chat_screen(model, chat_runtime, controller, render_tick),
        UiScreen::Contacts => contacts_screen(model, contacts_runtime, controller, render_tick),
        UiScreen::Notifications => {
            notifications_screen(model, notifications_runtime, controller, render_tick)
        }
        UiScreen::Settings => settings_screen(
            model,
            settings_runtime,
            controller,
            render_tick,
            theme,
            resolved_scheme,
        ),
    }
}

fn active_modal_title(model: &UiModel) -> Option<String> {
    let modal = model.modal?;
    if !model.modal_hint.trim().is_empty() {
        return Some(model.modal_hint.trim().to_string());
    }
    Some(
        match modal {
            ModalState::Help => "Help",
            ModalState::CreateInvitation => "Invite Contacts",
            ModalState::AcceptInvitation => "Accept Invitation",
            ModalState::CreateHome => "Create New Home",
            ModalState::CreateChannel => "New Chat Group",
            ModalState::SetChannelTopic => "Set Channel Topic",
            ModalState::ChannelInfo => "Channel Info",
            ModalState::EditNickname => "Edit Nickname",
            ModalState::RemoveContact => "Remove Contact",
            ModalState::GuardianSetup => "Guardian Setup",
            ModalState::RequestRecovery => "Request Recovery",
            ModalState::AddDeviceStep1 => "Add Device",
            ModalState::ImportDeviceEnrollmentCode => "Import Device Enrollment Code",
            ModalState::SelectDeviceToRemove => "Select Device to Remove",
            ModalState::ConfirmRemoveDevice => "Confirm Device Removal",
            ModalState::MfaSetup => "Multifactor Setup",
            ModalState::AssignModerator => "Assign Moderator",
            ModalState::SwitchAuthority => "Switch Authority",
            ModalState::AccessOverride => "Access Override",
            ModalState::CapabilityConfig => "Home Capability Configuration",
        }
        .to_string(),
    )
}

fn modal_view(model: &UiModel) -> Option<ModalView> {
    let modal = model.modal?;
    let title = active_modal_title(model).unwrap_or_else(|| "Modal".to_string());
    let mut details = Vec::new();
    let mut keybind_rows = Vec::new();
    let mut input_label = None;

    match modal {
        ModalState::Help => {
            let (help_details, help_keybind_rows) = help_modal_content(model.screen);
            details = help_details;
            keybind_rows = help_keybind_rows;
        }
        ModalState::CreateInvitation => {
            details.push("Create an invitation code for a contact.".to_string());
            details.push("Press Enter to generate and copy the code.".to_string());
        }
        ModalState::AcceptInvitation => {
            details.push("Paste an invitation code, then press Enter.".to_string());
            input_label = Some("Invitation Code".to_string());
        }
        ModalState::CreateHome => {
            details.push("Enter a new home name and press Enter.".to_string());
            input_label = Some("Home Name".to_string());
        }
        ModalState::CreateChannel => match model.create_channel_step {
            CreateChannelWizardStep::Details => {
                let active = match model.create_channel_active_field {
                    CreateChannelDetailsField::Name => "Group Name",
                    CreateChannelDetailsField::Topic => "Topic",
                };
                details.push("Step 1 of 3: Configure group details.".to_string());
                details.push(format!("Group name: {}", model.create_channel_name));
                details.push(format!("Topic: {}", model.create_channel_topic));
                details.push(format!("Active field: {active} (Tab to switch)"));
                input_label = Some(active.to_string());
            }
            CreateChannelWizardStep::Members => {
                details.push("Step 2 of 3: Select members to invite.".to_string());
                if model.contacts.is_empty() {
                    details.push("No contacts available.".to_string());
                } else {
                    for (idx, contact) in model.contacts.iter().enumerate() {
                        let focused = if idx == model.create_channel_member_focus {
                            ">"
                        } else {
                            " "
                        };
                        let selected = if model.create_channel_selected_members.contains(&idx) {
                            "[x]"
                        } else {
                            "[ ]"
                        };
                        details.push(format!("{focused} {selected} {}", contact.name));
                    }
                }
                details.push("Use ↑/↓ to move, Space to toggle, Enter to continue.".to_string());
            }
            CreateChannelWizardStep::Threshold => {
                let participant_total = model
                    .create_channel_selected_members
                    .len()
                    .saturating_add(1);
                details.push("Step 3 of 3: Set threshold.".to_string());
                details.push(format!("Participants (including you): {participant_total}"));
                details.push("Use ↑/↓ to adjust, Enter to create.".to_string());
                input_label = Some("Threshold".to_string());
            }
        },
        ModalState::SetChannelTopic => {
            details.push("Set a topic for the selected channel.".to_string());
            input_label = Some("Channel Topic".to_string());
        }
        ModalState::ChannelInfo => {
            details.push("Channel details view.".to_string());
        }
        ModalState::EditNickname => {
            details.push("Update the selected nickname and press Enter.".to_string());
            input_label = Some("Nickname".to_string());
        }
        ModalState::RemoveContact => {
            details.push("Remove the selected contact from this authority.".to_string());
            details.push("Press Enter to confirm.".to_string());
        }
        ModalState::GuardianSetup => match model.guardian_wizard_step {
            ThresholdWizardStep::Selection => {
                details.push("Step 1 of 3: Select guardians.".to_string());
                if model.contacts.is_empty() {
                    details.push("No contacts available.".to_string());
                } else {
                    for (idx, contact) in model.contacts.iter().enumerate() {
                        let focused = if idx == model.guardian_focus_index {
                            ">"
                        } else {
                            " "
                        };
                        let selected = if model.guardian_selected_indices.contains(&idx) {
                            "[x]"
                        } else {
                            "[ ]"
                        };
                        details.push(format!("{focused} {selected} {}", contact.name));
                    }
                }
                details.push("Use ↑/↓ to move, Space to toggle, Enter to continue.".to_string());
            }
            ThresholdWizardStep::Threshold => {
                details.push("Step 2 of 3: Choose threshold.".to_string());
                details.push(format!(
                    "Selected guardians: {}",
                    model.guardian_selected_count
                ));
                details.push("Enter k (approvals required).".to_string());
                input_label = Some("Threshold (k)".to_string());
            }
            ThresholdWizardStep::Ceremony => {
                details.push("Step 3 of 3: Ready to start ceremony.".to_string());
                details.push(format!(
                    "Will start guardian setup with {} of {} approvals.",
                    model.guardian_threshold_k, model.guardian_selected_count
                ));
                details.push("Press Enter to start.".to_string());
            }
        },
        ModalState::RequestRecovery => {
            details.push("Request guardian-assisted recovery for this authority.".to_string());
            details.push("Press Enter to notify your configured guardians.".to_string());
        }
        ModalState::AddDeviceStep1 => match model.add_device_step {
            AddDeviceWizardStep::Name => {
                details.push("Step 1 of 3: Name the device you want to invite.".to_string());
                details.push("This is the new device, not the current one.".to_string());
                details.push("Press Enter to generate an out-of-band enrollment code.".to_string());
                input_label = Some("Device Name".to_string());
            }
            AddDeviceWizardStep::ShareCode => {
                details
                    .push("Step 2 of 3: Share this code out-of-band with that device.".to_string());
                details.push(format!(
                    "Enrollment Code: {}",
                    model.add_device_enrollment_code
                ));
                details.push("Press c to copy, then press Enter when shared.".to_string());
                if let Some(ceremony_id) = &model.add_device_ceremony_id {
                    details.push(format!("Ceremony: {ceremony_id}"));
                }
            }
            AddDeviceWizardStep::Confirm => {
                details.push(
                    "Step 3 of 3: Waiting for the new device to import the code.".to_string(),
                );
                details.push(format!(
                    "Device '{}': {} of {} confirmations ({})",
                    model.add_device_name,
                    model.add_device_accepted_count,
                    model.add_device_total_count.max(1),
                    model.add_device_threshold.max(1)
                ));
                if let Some(error) = &model.add_device_error_message {
                    details.push(format!("Error: {error}"));
                } else if model.add_device_has_failed {
                    details.push("The enrollment ceremony failed.".to_string());
                } else if model.add_device_is_complete {
                    details.push("Enrollment ceremony complete. The new device is now part of this authority.".to_string());
                } else {
                    details.push("Leave this dialog open to monitor progress, or press Esc to cancel the ceremony.".to_string());
                }
            }
        },
        ModalState::ImportDeviceEnrollmentCode => {
            details.push("Import a device enrollment code and press Enter.".to_string());
            input_label = Some("Enrollment Code".to_string());
        }
        ModalState::SelectDeviceToRemove => {
            details.push("Select the device to remove.".to_string());
            details.push(format!(
                "Selected: {}",
                model
                    .secondary_device_name()
                    .unwrap_or(model.remove_device_candidate_name.as_str())
            ));
            details.push("Press Enter to continue.".to_string());
        }
        ModalState::ConfirmRemoveDevice => {
            details.push(format!(
                "Remove \"{}\" from this authority?",
                model
                    .secondary_device_name()
                    .unwrap_or(model.remove_device_candidate_name.as_str())
            ));
            details.push("Press Enter to confirm removal.".to_string());
        }
        ModalState::MfaSetup => match model.mfa_wizard_step {
            ThresholdWizardStep::Selection => {
                details.push("Step 1 of 3: Select devices for MFA signing.".to_string());
                let devices = if model.has_secondary_device {
                    vec![
                        "This Device".to_string(),
                        model
                            .secondary_device_name()
                            .unwrap_or("Secondary Device")
                            .to_string(),
                    ]
                } else {
                    vec!["This Device".to_string()]
                };
                for (idx, device) in devices.iter().enumerate() {
                    let focused = if idx == model.mfa_focus_index {
                        ">"
                    } else {
                        " "
                    };
                    let selected = if model.mfa_selected_indices.contains(&idx) {
                        "[x]"
                    } else {
                        "[ ]"
                    };
                    details.push(format!("{focused} {selected} {device}"));
                }
                details.push("Use ↑/↓ to move, Space to toggle, Enter to continue.".to_string());
            }
            ThresholdWizardStep::Threshold => {
                details.push("Step 2 of 3: Configure signing threshold.".to_string());
                details.push(format!("Selected devices: {}", model.mfa_selected_count));
                details.push("Enter required signatures (k).".to_string());
                input_label = Some("Threshold (k)".to_string());
            }
            ThresholdWizardStep::Ceremony => {
                details.push("Step 3 of 3: Ready to start MFA ceremony.".to_string());
                details.push(format!(
                    "Will start MFA with {} of {} signatures.",
                    model.mfa_threshold_k, model.mfa_selected_count
                ));
                details.push("Press Enter to start.".to_string());
            }
        },
        ModalState::AssignModerator => {
            details.push("Apply moderator role changes in the currently entered home.".to_string());
            details.push("Select a member in the Home panel first. Only members can be designated as moderators.".to_string());
        }
        ModalState::SwitchAuthority => {
            details.push("Switch to another authority stored on this device.".to_string());
            details.push("Use ↑/↓ to choose, then press Enter to reload into it.".to_string());
            if model.authorities.is_empty() {
                details.push("No authorities available.".to_string());
            } else {
                for (idx, authority) in model.authorities.iter().enumerate() {
                    let focused = if idx == model.selected_authority_index {
                        ">"
                    } else {
                        " "
                    };
                    let current = if authority.is_current {
                        " (current)"
                    } else {
                        ""
                    };
                    details.push(format!("{focused} {}{current}", authority.label));
                }
            }
        }
        ModalState::AccessOverride => {
            let selected_contact = model
                .selected_contact_name()
                .unwrap_or("No contact selected");
            let level = if model.access_override_partial {
                "Partial"
            } else {
                "Limited"
            };
            details.push("Apply a per-home access override for the selected contact.".to_string());
            details.push(format!("Selected contact: {selected_contact}"));
            details.push(format!("Access level: {level}"));
            details.push("Use ↑/↓ to select a contact. Tab toggles Limited/Partial.".to_string());
            details.push("Press Enter to apply the override to the current home.".to_string());
        }
        ModalState::CapabilityConfig => {
            let active = match model.capability_active_field {
                0 => "Full",
                1 => "Partial",
                _ => "Limited",
            };
            details.push("Configure per-home capabilities for each access level.".to_string());
            details.push("Tab switches fields. Enter saves to the current home.".to_string());
            details.push(format!("Editing: {active}"));
            details.push(format!("Full: {}", model.capability_full_caps));
            details.push(format!("Partial: {}", model.capability_partial_caps));
            details.push(format!("Limited: {}", model.capability_limited_caps));
            input_label = Some(format!("{active} Capabilities"));
        }
    }

    let input_value = if modal_accepts_text(model, modal) {
        Some(model.modal_buffer.clone())
    } else {
        None
    };

    let enter_label = match modal {
        ModalState::Help | ModalState::ChannelInfo => "Close".to_string(),
        ModalState::CreateChannel => match model.create_channel_step {
            CreateChannelWizardStep::Threshold => "Create".to_string(),
            _ => "Next".to_string(),
        },
        ModalState::AddDeviceStep1 => match model.add_device_step {
            AddDeviceWizardStep::Name => "Generate Code".to_string(),
            AddDeviceWizardStep::ShareCode => "Next".to_string(),
            AddDeviceWizardStep::Confirm => {
                if model.add_device_is_complete || model.add_device_has_failed {
                    "Close".to_string()
                } else {
                    "Refresh".to_string()
                }
            }
        },
        ModalState::GuardianSetup => match model.guardian_wizard_step {
            ThresholdWizardStep::Selection => "Next".to_string(),
            ThresholdWizardStep::Threshold => "Next".to_string(),
            ThresholdWizardStep::Ceremony => "Start".to_string(),
        },
        ModalState::MfaSetup => match model.mfa_wizard_step {
            ThresholdWizardStep::Selection => "Next".to_string(),
            ThresholdWizardStep::Threshold => "Next".to_string(),
            ThresholdWizardStep::Ceremony => "Start".to_string(),
        },
        ModalState::SwitchAuthority => "Switch".to_string(),
        ModalState::AccessOverride => "Apply".to_string(),
        ModalState::CapabilityConfig => "Save".to_string(),
        _ => "Confirm".to_string(),
    };

    Some(ModalView {
        title,
        details,
        keybind_rows,
        input_label,
        input_value,
        enter_label,
    })
}

fn help_modal_content(screen: UiScreen) -> (Vec<String>, Vec<(String, String)>) {
    let details = match screen {
        UiScreen::Neighborhood => vec![
            "Neighborhood reference".to_string(),
            "Browse homes, access depth, and neighborhood detail views.".to_string(),
        ],
        UiScreen::Chat => vec![
            "Chat reference".to_string(),
            "Navigate channels, compose messages, and manage channel metadata.".to_string(),
        ],
        UiScreen::Contacts => vec![
            "Contacts reference".to_string(),
            "Manage invitations, nicknames, guardians, and direct-message handoff.".to_string(),
        ],
        UiScreen::Notifications => vec![
            "Notifications reference".to_string(),
            "Review pending notices and move through the notification feed.".to_string(),
        ],
        UiScreen::Settings => vec![
            "Settings reference".to_string(),
            "Adjust profile, recovery, devices, authority, and appearance.".to_string(),
        ],
    };

    let keybind_rows = match screen {
        UiScreen::Neighborhood => vec![
            ("1-5".to_string(), "Switch screens".to_string()),
            ("tab / shift+tab".to_string(), "Cycle screens".to_string()),
            ("enter".to_string(), "Toggle map/detail view".to_string()),
            ("a".to_string(), "Accept home invitation".to_string()),
            ("n".to_string(), "Create home".to_string()),
            ("d".to_string(), "Cycle access depth".to_string()),
            ("esc".to_string(), "Close modal / back out".to_string()),
        ],
        UiScreen::Chat => vec![
            ("1-5".to_string(), "Switch screens".to_string()),
            ("tab / shift+tab".to_string(), "Cycle screens".to_string()),
            (
                "up / down".to_string(),
                "Move channel selection".to_string(),
            ),
            ("i".to_string(), "Enter message input".to_string()),
            ("n".to_string(), "Create channel".to_string()),
            ("t".to_string(), "Set channel topic".to_string()),
            ("o".to_string(), "Open channel info".to_string()),
            ("esc".to_string(), "Close modal / exit input".to_string()),
        ],
        UiScreen::Contacts => vec![
            ("1-5".to_string(), "Switch screens".to_string()),
            ("tab / shift+tab".to_string(), "Cycle screens".to_string()),
            (
                "up / down".to_string(),
                "Move contact selection".to_string(),
            ),
            (
                "left / right".to_string(),
                "Toggle contact detail pane".to_string(),
            ),
            ("n".to_string(), "Create invitation".to_string()),
            ("a".to_string(), "Accept invitation".to_string()),
            ("e".to_string(), "Edit nickname".to_string()),
            ("g".to_string(), "Configure guardians".to_string()),
            ("c".to_string(), "Open DM for selected contact".to_string()),
            ("r".to_string(), "Remove contact".to_string()),
        ],
        UiScreen::Notifications => vec![
            ("1-5".to_string(), "Switch screens".to_string()),
            ("tab / shift+tab".to_string(), "Cycle screens".to_string()),
            (
                "up / down".to_string(),
                "Move notification selection".to_string(),
            ),
            ("enter".to_string(), "No-op placeholder".to_string()),
            ("esc".to_string(), "Close modal".to_string()),
        ],
        UiScreen::Settings => vec![
            ("1-5".to_string(), "Switch screens".to_string()),
            ("tab / shift+tab".to_string(), "Cycle screens".to_string()),
            (
                "up / down".to_string(),
                "Move settings selection".to_string(),
            ),
            (
                "enter".to_string(),
                "Open selected settings action".to_string(),
            ),
            ("e".to_string(), "Edit profile nickname".to_string()),
            ("t".to_string(), "Guardian threshold setup".to_string()),
            ("s".to_string(), "Request recovery".to_string()),
            ("a".to_string(), "Add device".to_string()),
            ("i".to_string(), "Import enrollment code".to_string()),
        ],
    };

    (details, keybind_rows)
}

fn modal_accepts_text(model: &UiModel, modal: ModalState) -> bool {
    if matches!(modal, ModalState::CreateChannel) {
        return matches!(
            model.create_channel_step,
            CreateChannelWizardStep::Details | CreateChannelWizardStep::Threshold
        );
    }
    if matches!(modal, ModalState::AddDeviceStep1) {
        return matches!(model.add_device_step, AddDeviceWizardStep::Name);
    }
    if matches!(modal, ModalState::GuardianSetup) {
        return matches!(model.guardian_wizard_step, ThresholdWizardStep::Threshold);
    }
    if matches!(modal, ModalState::MfaSetup) {
        return matches!(model.mfa_wizard_step, ThresholdWizardStep::Threshold);
    }
    matches!(
        modal,
        ModalState::CreateInvitation
            | ModalState::AcceptInvitation
            | ModalState::CreateHome
            | ModalState::SetChannelTopic
            | ModalState::EditNickname
            | ModalState::ImportDeviceEnrollmentCode
    )
}

fn handle_keydown(controller: &UiController, event: &KeyboardData) -> bool {
    match event.key() {
        Key::Enter => {
            controller.send_key_named("enter", 1);
            true
        }
        Key::Escape => {
            controller.send_key_named("esc", 1);
            true
        }
        Key::Tab => {
            if event.modifiers().contains(Modifiers::SHIFT) {
                controller.send_key_named("backtab", 1);
            } else {
                controller.send_key_named("tab", 1);
            }
            true
        }
        Key::ArrowUp => {
            controller.send_key_named("up", 1);
            true
        }
        Key::ArrowDown => {
            controller.send_key_named("down", 1);
            true
        }
        Key::ArrowLeft => {
            controller.send_key_named("left", 1);
            true
        }
        Key::ArrowRight => {
            controller.send_key_named("right", 1);
            true
        }
        Key::Backspace => {
            controller.send_key_named("backspace", 1);
            true
        }
        Key::Character(text) => {
            if text.is_empty() {
                return false;
            }
            controller.send_keys(&text);
            true
        }
        _ => false,
    }
}

fn should_skip_global_key(controller: &UiController, event: &KeyboardData) -> bool {
    let Some(model) = controller.ui_model() else {
        return false;
    };
    let Some(modal) = model.modal else {
        return false;
    };
    if !modal_accepts_text(&model, modal) {
        return false;
    }
    !matches!(event.key(), Key::Enter | Key::Escape)
}
