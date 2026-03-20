//! Dioxus-based web UI application root and screen components.
//!
//! Provides the main application shell, screen routing, keyboard handling,
//! and toast notifications for the Aura web interface.

use crate::components::{
    AuthorityPickerItem, ButtonVariant, ModalInputView, ModalView, PillTone,
    UiAuthorityPickerModal, UiButton, UiCard, UiCardBody, UiCardFooter, UiDeviceEnrollmentModal,
    UiFooter, UiListButton, UiListItem, UiModal, UiPill,
};
use crate::model::{
    AccessDepth, AccessOverrideLevel, ActiveModal, AddDeviceWizardStep, CapabilityTier,
    CreateChannelDetailsField, CreateChannelWizardStep, ModalState, NeighborhoodMemberSelectionKey,
    NeighborhoodMode, NotificationSelectionId, ScreenId, SettingsSection, ThresholdWizardStep,
    UiController, UiModel, DEFAULT_CAPABILITY_FULL, DEFAULT_CAPABILITY_LIMITED,
    DEFAULT_CAPABILITY_PARTIAL,
};
use aura_app::signal_defs::{DiscoveredPeersState, SettingsState};
use aura_app::ui::contract::{
    list_item_dom_id, ConfirmationState, ControlId, FieldId, ListId, ListItemSnapshot,
    ListSnapshot, MessageSnapshot, ModalId, OperationId, OperationInstanceId, OperationSnapshot,
    OperationState, SelectionSnapshot, UiReadiness, UiSnapshot,
};
use aura_app::ui::signals::{
    DiscoveredPeerMethod, NetworkStatus, AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL, CHAT_SIGNAL,
    CONTACTS_SIGNAL, DISCOVERED_PEERS_SIGNAL, ERROR_SIGNAL, HOMES_SIGNAL, INVITATIONS_SIGNAL,
    NEIGHBORHOOD_SIGNAL, NETWORK_STATUS_SIGNAL, RECOVERY_SIGNAL, SETTINGS_SIGNAL,
    TRANSPORT_PEERS_SIGNAL,
};
use aura_app::ui::types::{
    all_command_help, command_help, format_network_status_with_severity, parse_chat_command,
    AccessLevel, AppError, ChatCommand, ChatState, ContactsState, HomeRole, HomesState,
    InvitationBridgeType, InvitationsState, NeighborhoodState, RecoveryState,
};
use aura_app::ui::workflows::ceremonies as ceremony_workflows;
use aura_app::ui::workflows::moderation as moderation_workflows;
use aura_app::ui::workflows::moderator as moderator_workflows;
use aura_app::ui::workflows::{
    access as access_workflows, contacts as contacts_workflows, context as context_workflows,
    invitation as invitation_workflows, messaging as messaging_workflows, query as query_workflows,
    recovery as recovery_workflows, runtime as runtime_workflows, settings as settings_workflows,
    time as time_workflows,
};
use aura_app::ui_contract::{bridged_operation_statuses, ChannelFactKey, RuntimeFact};
use aura_app::views::chat::{is_note_to_self_channel_name, NOTE_TO_SELF_CHANNEL_NAME};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::types::identifiers::{AuthorityId, CeremonyId};
use aura_core::ChannelId;
use dioxus::dioxus_core::schedule_update;
use dioxus::events::KeyboardData;
use dioxus::prelude::*;
use dioxus_shadcn::components::empty::{Empty, EmptyDescription, EmptyHeader, EmptyTitle};
use dioxus_shadcn::components::scroll_area::{ScrollArea, ScrollAreaViewport};
use dioxus_shadcn::components::toast::{use_toast, ToastOptions, ToastPosition, ToastProvider};
use dioxus_shadcn::theme::{themes, use_theme, ColorScheme, ThemeProvider};
use std::sync::Arc;
use std::time::Duration;

trait RequiredDomId {
    fn required_dom_id(self, context: &'static str) -> &'static str;
}

impl RequiredDomId for Option<&'static str> {
    fn required_dom_id(self, context: &'static str) -> &'static str {
        let Some(id) = self else {
            panic!("{context} must define a web DOM id");
        };
        id
    }
}

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
    id: String,
    name: String,
    topic: String,
    unread_count: u32,
    last_message: Option<String>,
    member_count: u32,
    is_dm: bool,
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct ContactsRuntimeContact {
    authority_id: AuthorityId,
    name: String,
    nickname_hint: Option<String>,
    is_guardian: bool,
    is_member: bool,
    is_online: bool,
    confirmation: ConfirmationState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ContactsRuntimePeer {
    authority_id: AuthorityId,
    address: String,
    invited: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ContactsRuntimeView {
    loaded: bool,
    contacts: Vec<ContactsRuntimeContact>,
    lan_peers: Vec<ContactsRuntimePeer>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SettingsRuntimeDevice {
    id: String,
    name: String,
    is_current: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SettingsRuntimeAuthority {
    id: AuthorityId,
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
    action: NotificationRuntimeAction,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum NotificationRuntimeAction {
    #[default]
    None,
    ReceivedInvitation,
    SentInvitation,
    RecoveryApproval,
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

fn is_pinned_chat_channel(channel: &aura_app::ui::types::Channel) -> bool {
    is_dm_like_channel(channel) || is_note_to_self_channel_name(&channel.name)
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
            if is_pinned_chat_channel(channel) {
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

    channels.sort_by(|left, right| {
        match (
            is_note_to_self_channel_name(&left.name),
            is_note_to_self_channel_name(&right.name),
        ) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => left.name.cmp(&right.name),
        }
    });
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

    let current_home = active_home_id
        .parse::<ChannelId>()
        .ok()
        .and_then(|home_id| homes.home_state(&home_id).cloned())
        .or_else(|| homes.current_home().cloned());

    let mut runtime_homes = Vec::new();
    if neighborhood.home_home_id != ChannelId::default() || !neighborhood.home_name.is_empty() {
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
            id: channel.id.to_string(),
            name: channel.name.clone(),
            topic: channel.topic.clone().unwrap_or_default(),
            unread_count: channel.unread_count,
            last_message: channel.last_message.clone(),
            member_count: channel.member_count,
            is_dm: channel.is_dm,
        })
        .collect();
    channels.sort_by(|left, right| {
        match (
            is_note_to_self_channel_name(&left.name),
            is_note_to_self_channel_name(&right.name),
        ) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => left.name.cmp(&right.name),
        }
    });

    let active_channel = selected_channel_name
        .and_then(|name| {
            channels
                .iter()
                .find(|channel| {
                    channel.name.eq_ignore_ascii_case(name) || channel.id.eq_ignore_ascii_case(name)
                })
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
    fn saturating_u32(value: usize) -> u32 {
        u32::try_from(value).unwrap_or(u32::MAX)
    }

    let (chat, authority_id) = {
        let core = controller.app_core().read().await;
        let signal_chat = core.read(&*CHAT_SIGNAL).await.unwrap_or_default();
        let snapshot = core.snapshot();
        let local_chat = snapshot.chat;
        let mut merged = merge_chat_state(signal_chat, local_chat);
        let authority_id = core.authority().cloned();
        if let Some(authority_id) = authority_id {
            merged.ensure_note_to_self_channel(authority_id);
        }
        (merged, authority_id)
    };
    let selected_name = controller
        .ui_model()
        .and_then(|model| model.selected_channel_name().map(str::to_string));
    let runtime = build_chat_runtime_view(chat.clone(), selected_name.as_deref());
    controller.push_log(&format!(
        "load_chat_runtime_view: selected={:?} active={} channels={}",
        selected_name,
        runtime.active_channel,
        runtime.channels.len()
    ));
    let mut runtime_facts = vec![RuntimeFact::ChatSignalUpdated {
        active_channel: runtime.active_channel.clone(),
        channel_count: saturating_u32(runtime.channels.len()),
        message_count: saturating_u32(runtime.messages.len()),
    }];
    if let (Some(channel), Some(authority_id)) = (
        chat.all_channels()
            .find(|channel| channel.name.eq_ignore_ascii_case(&runtime.active_channel)),
        authority_id,
    ) {
        let resolved_recipient_count = channel
            .member_ids
            .iter()
            .filter(|member_id| **member_id != authority_id)
            .count();
        let resolved_member_count = channel
            .member_count
            .max((resolved_recipient_count.saturating_add(1)) as u32);
        runtime_facts.push(RuntimeFact::ChannelMembershipReady {
            channel: ChannelFactKey::named(channel.name.clone()),
            member_count: Some(resolved_member_count),
        });
        if resolved_recipient_count > 0 {
            let channel_key = ChannelFactKey::named(channel.name.clone());
            runtime_facts.push(RuntimeFact::RecipientPeersResolved {
                channel: channel_key.clone(),
                member_count: resolved_member_count,
            });
            runtime_facts.push(RuntimeFact::MessageDeliveryReady {
                channel: channel_key,
                member_count: resolved_member_count,
            });
        }
    }
    controller.publish_runtime_channels_projection(
        runtime
            .channels
            .iter()
            .map(|channel| (channel.name.clone(), channel.topic.clone()))
            .collect(),
        runtime_facts,
    );
    runtime
}

fn merge_chat_state(
    mut signal_chat: aura_app::views::chat::ChatState,
    local_chat: aura_app::views::chat::ChatState,
) -> aura_app::views::chat::ChatState {
    for local_channel in local_chat.all_channels() {
        match signal_chat.channel_mut(&local_channel.id) {
            Some(signal_channel) => {
                if signal_channel.context_id.is_none() {
                    signal_channel.context_id = local_channel.context_id;
                }
                if signal_channel.topic.is_none() && local_channel.topic.is_some() {
                    signal_channel.topic = local_channel.topic.clone();
                }
                if signal_channel.name == signal_channel.id.to_string()
                    && local_channel.name != local_channel.id.to_string()
                    && !local_channel.name.trim().is_empty()
                {
                    signal_channel.name = local_channel.name.clone();
                }
                if local_channel.member_count > signal_channel.member_count {
                    signal_channel.member_count = local_channel.member_count;
                }
                for member in &local_channel.member_ids {
                    if !signal_channel.member_ids.contains(member) {
                        signal_channel.member_ids.push(*member);
                    }
                }
            }
            None => signal_chat.upsert_channel(local_channel.clone()),
        }

        for message in local_chat.messages_for_channel(&local_channel.id) {
            signal_chat.apply_message(local_channel.id, message.clone());
        }
    }

    signal_chat
}

fn build_contacts_runtime_view(
    contacts: ContactsState,
    discovered_peers: DiscoveredPeersState,
) -> ContactsRuntimeView {
    let mut rows: Vec<_> = contacts
        .all_contacts()
        .map(|contact| ContactsRuntimeContact {
            authority_id: contact.id,
            name: display_contact_name(contact),
            nickname_hint: contact
                .nickname_suggestion
                .clone()
                .filter(|value| !value.trim().is_empty()),
            is_guardian: contact.is_guardian,
            is_member: contact.is_member,
            is_online: contact.is_online,
            confirmation: ConfirmationState::Confirmed,
        })
        .collect();
    rows.sort_by(|left, right| left.name.cmp(&right.name));

    let mut lan_peers: Vec<_> = discovered_peers
        .peers
        .into_iter()
        .filter(|peer| peer.method == DiscoveredPeerMethod::Lan)
        .map(|peer| ContactsRuntimePeer {
            authority_id: peer.authority_id,
            address: peer.address,
            invited: peer.invited,
        })
        .collect();
    lan_peers.sort_by(|left, right| {
        left.authority_id
            .to_string()
            .cmp(&right.authority_id.to_string())
    });

    ContactsRuntimeView {
        loaded: true,
        contacts: rows,
        lan_peers,
    }
}

async fn load_contacts_runtime_view(controller: Arc<UiController>) -> ContactsRuntimeView {
    let contacts = {
        let core = controller.app_core().read().await;
        core.read(&*CONTACTS_SIGNAL).await.unwrap_or_default()
    };
    let discovered_peers = {
        let core = controller.app_core().read().await;
        core.read(&*DISCOVERED_PEERS_SIGNAL)
            .await
            .unwrap_or_default()
    };
    let runtime = build_contacts_runtime_view(contacts, discovered_peers);
    let runtime_facts = vec![RuntimeFact::RemoteFactsPulled {
        contact_count: u32::try_from(runtime.contacts.len()).unwrap_or(u32::MAX),
        lan_peer_count: u32::try_from(runtime.lan_peers.len()).unwrap_or(u32::MAX),
    }];
    controller.publish_runtime_contacts_projection(
        runtime
            .contacts
            .iter()
            .map(|contact| {
                (
                    contact.authority_id,
                    contact.name.clone(),
                    contact.is_guardian,
                )
            })
            .collect(),
        runtime_facts,
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
            label: if authority.nickname_suggestion.trim().is_empty() {
                authority.id.to_string()
            } else {
                authority.nickname_suggestion.clone()
            },
            id: authority.id,
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
            .map(|authority| (authority.id, authority.label.clone(), authority.is_current))
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
            invitation.status,
            aura_app::ui::types::InvitationStatus::Pending
        ) {
            continue;
        }

        let (kind_label, title, subtitle, detail, action) =
            match (invitation.direction, invitation.invitation_type) {
                (
                    aura_app::ui::types::InvitationDirection::Received,
                    aura_app::ui::types::InvitationType::Guardian,
                ) => (
                    "Guardian Request",
                    format!("Guardian Request from {}", invitation.from_name),
                    invitation
                        .message
                        .clone()
                        .unwrap_or_else(|| "Pending response".to_string()),
                    invitation
                        .home_name
                        .clone()
                        .unwrap_or_else(|| invitation.from_id.to_string()),
                    NotificationRuntimeAction::ReceivedInvitation,
                ),
                (
                    aura_app::ui::types::InvitationDirection::Received,
                    aura_app::ui::types::InvitationType::Chat,
                ) => (
                    "Contact Request",
                    format!("Contact Request from {}", invitation.from_name),
                    invitation
                        .message
                        .clone()
                        .unwrap_or_else(|| "Pending response".to_string()),
                    invitation
                        .home_name
                        .clone()
                        .unwrap_or_else(|| invitation.from_id.to_string()),
                    NotificationRuntimeAction::ReceivedInvitation,
                ),
                (
                    aura_app::ui::types::InvitationDirection::Received,
                    aura_app::ui::types::InvitationType::Home,
                ) => (
                    "Home Invite",
                    format!("Home Invite from {}", invitation.from_name),
                    invitation
                        .message
                        .clone()
                        .unwrap_or_else(|| "Pending response".to_string()),
                    invitation
                        .home_name
                        .clone()
                        .unwrap_or_else(|| invitation.from_id.to_string()),
                    NotificationRuntimeAction::ReceivedInvitation,
                ),
                (
                    aura_app::ui::types::InvitationDirection::Sent,
                    aura_app::ui::types::InvitationType::Guardian,
                ) => (
                    "Sent Guardian Invite",
                    format!(
                        "Guardian invite to {}",
                        invitation
                            .to_name
                            .clone()
                            .unwrap_or_else(|| "unknown recipient".to_string())
                    ),
                    invitation
                        .message
                        .clone()
                        .unwrap_or_else(|| "Waiting for recipient".to_string()),
                    invitation.home_name.clone().unwrap_or_else(|| {
                        invitation
                            .to_id
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| "unknown recipient".to_string())
                    }),
                    NotificationRuntimeAction::SentInvitation,
                ),
                (
                    aura_app::ui::types::InvitationDirection::Sent,
                    aura_app::ui::types::InvitationType::Chat,
                ) => (
                    "Sent Contact Invite",
                    format!(
                        "Contact invite to {}",
                        invitation
                            .to_name
                            .clone()
                            .unwrap_or_else(|| "unknown recipient".to_string())
                    ),
                    invitation
                        .message
                        .clone()
                        .unwrap_or_else(|| "Waiting for recipient".to_string()),
                    invitation.home_name.clone().unwrap_or_else(|| {
                        invitation
                            .to_id
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| "unknown recipient".to_string())
                    }),
                    NotificationRuntimeAction::SentInvitation,
                ),
                (
                    aura_app::ui::types::InvitationDirection::Sent,
                    aura_app::ui::types::InvitationType::Home,
                ) => (
                    "Sent Home Invite",
                    format!(
                        "Home invite to {}",
                        invitation
                            .to_name
                            .clone()
                            .unwrap_or_else(|| "unknown recipient".to_string())
                    ),
                    invitation
                        .message
                        .clone()
                        .unwrap_or_else(|| "Waiting for recipient".to_string()),
                    invitation.home_name.clone().unwrap_or_else(|| {
                        invitation
                            .to_id
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| "unknown recipient".to_string())
                    }),
                    NotificationRuntimeAction::SentInvitation,
                ),
            };
        items.push(NotificationRuntimeItem {
            id: invitation.id.clone(),
            kind_label: kind_label.to_string(),
            title,
            subtitle,
            detail,
            timestamp: invitation.created_at,
            action,
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
            action: NotificationRuntimeAction::RecoveryApproval,
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
            action: NotificationRuntimeAction::None,
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
    let runtime = build_notifications_runtime_view(invitations, recovery, error);
    controller.publish_runtime_notifications_projection(
        runtime
            .items
            .iter()
            .map(|item| (NotificationSelectionId(item.id.clone()), item.title.clone()))
            .collect(),
        Vec::new(),
    );
    runtime
}

fn selected_home_id_for_modal(
    runtime: &NeighborhoodRuntimeView,
    model: &UiModel,
) -> Option<String> {
    model
        .selected_home_id()
        .map(ToString::to_string)
        .filter(|id| !id.is_empty())
        .or_else(|| {
            runtime
                .homes
                .iter()
                .find(|home| Some(home.name.as_str()) == model.selected_home_name())
                .map(|home| home.id.clone())
                .filter(|id| !id.is_empty())
        })
        .or_else(|| (!runtime.active_home_id.is_empty()).then(|| runtime.active_home_id.clone()))
}

fn selected_contact_for_modal(
    runtime: &ContactsRuntimeView,
    model: &UiModel,
) -> Option<ContactsRuntimeContact> {
    let selected = model.selected_contact_authority_id()?;
    runtime
        .contacts
        .iter()
        .find(|contact| contact.authority_id == selected)
        .cloned()
}

fn effective_contacts_view(
    runtime: &ContactsRuntimeView,
    model: &UiModel,
) -> Vec<ContactsRuntimeContact> {
    let mut merged = runtime.contacts.clone();

    for contact in &model.contacts {
        if merged
            .iter()
            .any(|row| row.authority_id == contact.authority_id)
        {
            continue;
        }

        merged.push(ContactsRuntimeContact {
            authority_id: contact.authority_id,
            name: contact.name.clone(),
            nickname_hint: None,
            is_guardian: contact.is_guardian,
            is_member: false,
            is_online: false,
            confirmation: contact.confirmation,
        });
    }

    merged.sort_by(|left, right| left.name.cmp(&right.name));
    merged
}

fn harness_log(line: &str) {
    tracing::info!("{line}");
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
                        .or_else(|| {
                            model
                                .selected_device_modal()
                                .map(|state| state.candidate_name.as_str())
                        })
                        .unwrap_or("")
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
    add_device_ceremony_id: Option<CeremonyId>,
    add_device_is_complete: bool,
    add_device_has_failed: bool,
    modal_buffer: String,
    neighborhood_runtime: NeighborhoodRuntimeView,
    chat_runtime: ChatRuntimeView,
    contacts_runtime: ContactsRuntimeView,
    settings_runtime: SettingsRuntimeView,
    selected_home_id: Option<String>,
    selected_member_key: Option<NeighborhoodMemberSelectionKey>,
    rerender: Arc<dyn Fn() + Send + Sync>,
) -> bool {
    let current_model = controller.ui_model();
    let modal_text_value = current_model
        .as_ref()
        .and_then(|model| model.modal_text_value())
        .unwrap_or_else(|| modal_buffer.clone());
    match modal_state {
        Some(ModalState::AddDeviceStep1) => {
            let (
                add_device_step,
                add_device_ceremony_id,
                add_device_is_complete,
                add_device_has_failed,
                modal_buffer,
            ) = current_model
                .as_ref()
                .and_then(|model| match model.active_modal.as_ref() {
                    Some(ActiveModal::AddDevice(state)) => Some((
                        state.step,
                        state.ceremony_id.clone(),
                        state.is_complete,
                        state.has_failed,
                        state.name_input.clone(),
                    )),
                    _ => None,
                })
                .unwrap_or((
                    add_device_step,
                    add_device_ceremony_id,
                    add_device_is_complete,
                    add_device_has_failed,
                    modal_buffer,
                ));
            match add_device_step {
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
                                let status_handle = start.status_handle.clone();
                                controller.set_runtime_device_enrollment_ceremony(start.handle);
                                controller.set_runtime_device_enrollment_ceremony_id(
                                    start.ceremony_id.clone(),
                                );
                                controller.complete_runtime_device_enrollment_started(
                                    &name,
                                    &start.enrollment_code,
                                );

                                let controller_for_status = controller.clone();
                                let app_core_for_status = app_core.clone();
                                let rerender_for_status = rerender_for_start.clone();
                                spawn(async move {
                                    loop {
                                        let _ =
                                            time_workflows::sleep_ms(&app_core_for_status, 1_000)
                                                .await;
                                        match ceremony_workflows::get_key_rotation_ceremony_status(
                                            &app_core_for_status,
                                            &status_handle,
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
                            Err(error) => {
                                controller.runtime_error_toast(error.to_string());
                            }
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

                    let Some(_ceremony_id) = add_device_ceremony_id else {
                        controller.runtime_error_toast("No active enrollment ceremony");
                        rerender();
                        return true;
                    };

                    let app_core = controller.app_core().clone();
                    let rerender_for_status = rerender.clone();
                    spawn(async move {
                        match controller.runtime_device_enrollment_status_handle() {
                            Some(status_handle) => {
                                match ceremony_workflows::get_key_rotation_ceremony_status(
                                    &app_core,
                                    &status_handle,
                                )
                                .await
                                {
                                    Ok(status) => controller
                                        .update_runtime_device_enrollment_status(
                                            status.accepted_count,
                                            status.total_count,
                                            status.threshold,
                                            status.is_complete,
                                            status.has_failed,
                                            status.error_message,
                                        ),
                                    Err(error) => controller.runtime_error_toast(error.to_string()),
                                }
                            }
                            None => controller
                                .runtime_error_toast("No active enrollment ceremony handle"),
                        }
                        rerender_for_status();
                    });
                    true
                }
            }
        }
        Some(ModalState::CreateHome) => {
            let name = modal_text_value.trim().to_string();
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
            let code = modal_text_value.trim().to_string();
            if code.is_empty() {
                controller.runtime_error_toast("Invitation code is required");
                rerender();
                return true;
            }

            let submit_log = format!("accept_invitation submit start code_len={}", code.len());
            controller.push_log(&submit_log);
            harness_log(&submit_log);
            let app_core = controller.app_core().clone();
            let controller_for_import = controller.clone();
            let rerender_for_import = rerender.clone();
            spawn(async move {
                controller_for_import.push_log("accept_invitation import_details start");
                harness_log("accept_invitation import_details start");
                match invitation_workflows::import_invitation_details(&app_core, &code).await {
                    Ok(invitation) => {
                        let invitation_info = invitation.info().clone();
                        let invitation_kind = match &invitation_info.invitation_type {
                            InvitationBridgeType::DeviceEnrollment { .. } => "device_enrollment",
                            InvitationBridgeType::Contact { .. } => "contact",
                            InvitationBridgeType::Guardian { .. } => "guardian",
                            InvitationBridgeType::Channel { .. } => "channel",
                        };
                        let import_ok_log = format!(
                            "accept_invitation import_details ok invitation_id={} kind={}",
                            invitation.invitation_id(),
                            invitation_kind
                        );
                        controller_for_import.push_log(&import_ok_log);
                        harness_log(&import_ok_log);
                        controller_for_import.push_log("accept_invitation runtime_accept start");
                        harness_log("accept_invitation runtime_accept start");
                        let accepted = match &invitation_info.invitation_type {
                            InvitationBridgeType::DeviceEnrollment { .. } => {
                                invitation_workflows::accept_device_enrollment_invitation(
                                    &app_core,
                                    &invitation_info,
                                )
                                .await
                            }
                            _ => {
                                invitation_workflows::accept_invitation(&app_core, invitation).await
                            }
                        };

                        match accepted {
                            Ok(_) => {
                                controller_for_import
                                    .push_log("accept_invitation runtime_accept ok");
                                harness_log("accept_invitation runtime_accept ok");
                                controller_for_import.push_runtime_fact(
                                    RuntimeFact::RemoteFactsPulled {
                                        contact_count: 0,
                                        lan_peer_count: 0,
                                    },
                                );
                                if matches!(
                                    &invitation_info.invitation_type,
                                    InvitationBridgeType::DeviceEnrollment { .. }
                                ) {
                                    controller_for_import
                                        .push_log("accept_invitation refresh_settings start");
                                    harness_log("accept_invitation refresh_settings start");
                                    let _ = settings_workflows::refresh_settings_from_runtime(
                                        &app_core,
                                    )
                                    .await;
                                    controller_for_import
                                        .push_log("accept_invitation refresh_settings done");
                                    harness_log("accept_invitation refresh_settings done");
                                    controller.complete_runtime_modal_success(
                                        "Device enrollment complete",
                                    );
                                    controller_for_import
                                        .push_log("accept_invitation complete device_enrollment");
                                    harness_log("accept_invitation complete device_enrollment");
                                } else {
                                    if let InvitationBridgeType::Contact { nickname } =
                                        &invitation_info.invitation_type
                                    {
                                        let display_name = nickname
                                            .clone()
                                            .filter(|value| !value.trim().is_empty())
                                            .unwrap_or_else(|| {
                                                invitation_info.sender_id.to_string()
                                            });
                                        controller_for_import
                                            .complete_runtime_contact_invitation_acceptance(
                                                invitation_info.sender_id,
                                                display_name,
                                            );
                                        controller_for_import
                                            .push_log("accept_invitation complete generic");
                                        harness_log("accept_invitation complete generic");
                                        return;
                                    }
                                    match &invitation_info.invitation_type {
                                        InvitationBridgeType::Guardian { .. } => {
                                            controller_for_import.push_log(
                                                "accept_invitation refresh_contacts start",
                                            );
                                            harness_log("accept_invitation refresh_contacts start");
                                            let _ = load_contacts_runtime_view(
                                                controller_for_import.clone(),
                                            )
                                            .await;
                                            controller_for_import.push_log(
                                                "accept_invitation refresh_contacts done",
                                            );
                                            harness_log("accept_invitation refresh_contacts done");
                                        }
                                        InvitationBridgeType::Channel { .. } => {
                                            controller_for_import.push_runtime_fact(
                                                RuntimeFact::ChannelJoined {
                                                    channel: None,
                                                    source: Some("accepted_invitation".to_string()),
                                                },
                                            );
                                        }
                                        InvitationBridgeType::DeviceEnrollment { .. }
                                        | InvitationBridgeType::Contact { .. } => {}
                                    }
                                    controller_for_import
                                        .push_log("accept_invitation refresh_contacts start");
                                    harness_log("accept_invitation refresh_contacts start");
                                    let _ =
                                        load_contacts_runtime_view(controller_for_import.clone())
                                            .await;
                                    controller_for_import
                                        .push_log("accept_invitation refresh_contacts done");
                                    harness_log("accept_invitation refresh_contacts done");
                                    controller_for_import.complete_runtime_invitation_operation();
                                    controller_for_import
                                        .push_log("accept_invitation complete generic");
                                    harness_log("accept_invitation complete generic");
                                }
                            }
                            Err(error) => {
                                let error_log =
                                    format!("accept_invitation runtime_accept error={error}");
                                controller_for_import.push_log(&error_log);
                                harness_log(&error_log);
                                controller.runtime_error_toast(error.to_string());
                            }
                        }
                    }
                    Err(error) => {
                        let error_log = format!("accept_invitation import_details error={error}");
                        controller_for_import.push_log(&error_log);
                        harness_log(&error_log);
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                rerender_for_import();
            });
            true
        }
        Some(ModalState::ImportDeviceEnrollmentCode) => {
            let code = modal_text_value.trim().to_string();
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
                        let invitation_info = invitation.info().clone();
                        if !matches!(
                            &invitation_info.invitation_type,
                            InvitationBridgeType::DeviceEnrollment { .. }
                        ) {
                            controller
                                .runtime_error_toast("Code is not a device enrollment invitation");
                            rerender_for_import();
                            return;
                        }

                        if let Ok(runtime) = runtime_workflows::require_runtime(&app_core).await {
                            for _ in 0..8 {
                                runtime_workflows::converge_runtime(&runtime).await;
                                if runtime_workflows::ensure_runtime_peer_connectivity(
                                    &runtime,
                                    "device_enrollment_accept",
                                )
                                .await
                                .is_ok()
                                {
                                    break;
                                }
                                let _ = time_workflows::sleep_ms(&app_core, 250).await;
                            }
                        }

                        match invitation_workflows::accept_device_enrollment_invitation(
                            &app_core,
                            &invitation_info,
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
            let receiver = modal_text_value.trim().to_string();
            if receiver.is_empty() {
                controller.runtime_error_toast("Receiver authority id is required");
                rerender();
                return true;
            }

            let app_core = controller.app_core().clone();
            spawn(async move {
                tracing::info!("create_invitation submit start");
                let receiver_id = match receiver.parse::<AuthorityId>() {
                    Ok(value) => value,
                    Err(error) => {
                        tracing::warn!(error = %error, "create_invitation invalid receiver");
                        controller.runtime_error_toast(format!("Invalid authority id: {error}"));
                        rerender();
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
                    Ok(invitation) => {
                        tracing::info!(invitation_id = %invitation.invitation_id(), "create_invitation create_contact_invitation ok");
                        tracing::info!(invitation_id = %invitation.invitation_id(), "create_invitation export_invitation start");
                        match invitation_workflows::export_invitation(
                            &app_core,
                            invitation.invitation_id(),
                        )
                        .await
                        {
                            Ok(code) => {
                                tracing::info!("create_invitation export_invitation ok");
                                controller.write_clipboard(&code);
                                tracing::info!("create_invitation write_clipboard ok");
                                controller.push_runtime_fact(RuntimeFact::InvitationCodeReady {
                                    receiver_authority_id: Some(receiver_id.to_string()),
                                    source_operation: OperationId::invitation_create(),
                                    code: Some(code),
                                });
                                controller.complete_runtime_modal_operation_success(
                                    OperationId::invitation_create(),
                                    "Invitation code copied to clipboard",
                                );
                                tracing::info!("create_invitation operation succeeded");
                                tracing::info!("create_invitation complete");
                            }
                            Err(error) => {
                                tracing::warn!(error = %error, "create_invitation export_invitation failed");
                                controller.runtime_error_toast(error.to_string());
                            }
                        }
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "create_invitation create_contact_invitation failed");
                        controller.runtime_error_toast(error.to_string());
                    }
                }
                tracing::info!("create_invitation rerender");
            });
            true
        }
        Some(ModalState::CreateChannel)
            if matches!(
                current_model
                    .as_ref()
                    .and_then(|m| m.create_channel_modal().map(|state| state.step)),
                Some(CreateChannelWizardStep::Threshold)
            ) =>
        {
            let Some(model) = current_model.clone() else {
                return false;
            };
            let (selected_members, channel_name, channel_topic, channel_threshold) =
                match model.active_modal.as_ref() {
                    Some(ActiveModal::CreateChannel(state)) => (
                        state.selected_members.clone(),
                        state.name.clone(),
                        state.topic.clone(),
                        state.threshold,
                    ),
                    _ => (Vec::new(), String::new(), String::new(), 1),
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
                    .selected_contact_index()
                    .map(|_| ())
                    .map(|_| {
                        selected_members
                            .iter()
                            .filter_map(|idx| contacts_runtime.contacts.get(*idx))
                            .map(|contact| contact.authority_id.to_string())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_else(|| {
                        selected_members
                            .iter()
                            .filter_map(|idx| contacts_runtime.contacts.get(*idx))
                            .map(|contact| contact.authority_id.to_string())
                            .collect::<Vec<_>>()
                    });

                match messaging_workflows::create_channel(
                    &app_core,
                    channel_name.trim(),
                    (!channel_topic.trim().is_empty()).then(|| channel_topic.trim().to_string()),
                    &members,
                    channel_threshold,
                    timestamp_ms,
                )
                .await
                {
                    Ok(_) => controller.complete_runtime_modal_success(format!(
                        "Created '{}'",
                        channel_name.trim()
                    )),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender_for_create();
            });
            true
        }
        Some(ModalState::SetChannelTopic) => {
            let channel_name = chat_runtime.active_channel.trim().to_string();
            let topic = modal_text_value.trim().to_string();
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
            let value = modal_text_value.trim().to_string();
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
                .map(|model| matches!(model.screen, ScreenId::Settings))
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
                    let authority_id = contact.authority_id.to_string();
                    contacts_workflows::update_contact_nickname(
                        &app_core,
                        &authority_id,
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
                let authority_id = contact.authority_id.to_string();
                let timestamp_ms = match context_workflows::current_time_ms(&app_core).await {
                    Ok(value) => value,
                    Err(error) => {
                        controller.runtime_error_toast(error.to_string());
                        rerender_for_remove();
                        return;
                    }
                };
                match contacts_workflows::remove_contact(&app_core, &authority_id, timestamp_ms)
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
                current_model
                    .as_ref()
                    .and_then(|m| m.guardian_setup_modal().map(|state| state.step)),
                Some(ThresholdWizardStep::Ceremony)
            ) =>
        {
            let Some(model) = current_model.clone() else {
                return false;
            };
            let (selected_indices, threshold_k) = match model.active_modal.as_ref() {
                Some(ActiveModal::GuardianSetup(state)) => {
                    (state.selected_indices.clone(), state.threshold_k)
                }
                _ => (Vec::new(), 1),
            };
            let app_core = controller.app_core().clone();
            let rerender_for_guardians = rerender.clone();
            spawn(async move {
                let ids: Vec<AuthorityId> = selected_indices
                    .iter()
                    .filter_map(|idx| contacts_runtime.contacts.get(*idx))
                    .map(|contact| contact.authority_id)
                    .collect();
                let guardian_ids = ids;
                let threshold = match aura_core::types::FrostThreshold::new(u16::from(threshold_k))
                {
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
                    Ok(ceremony_handle) => {
                        let status_handle = ceremony_handle.status_handle();
                        match ceremony_workflows::get_key_rotation_ceremony_status(
                            &app_core,
                            &status_handle,
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
                current_model
                    .as_ref()
                    .and_then(|m| m.mfa_setup_modal().map(|state| state.step)),
                Some(ThresholdWizardStep::Ceremony)
            ) =>
        {
            let Some(model) = current_model else {
                return false;
            };
            let app_core = controller.app_core().clone();
            let rerender_for_mfa = rerender.clone();
            spawn(async move {
                let Some(mfa_state) = model.mfa_setup_modal() else {
                    rerender_for_mfa();
                    return;
                };
                let device_ids: Vec<String> = mfa_state
                    .selected_indices
                    .iter()
                    .filter_map(|idx| settings_runtime.devices.get(*idx))
                    .map(|device| device.id.clone())
                    .collect();
                let threshold =
                    match aura_core::types::FrostThreshold::new(u16::from(mfa_state.threshold_k)) {
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
                        controller.complete_runtime_modal_success("Multifactor ceremony started");
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
            let Some(member) = selected_member_key
                .as_ref()
                .and_then(|selected_key| {
                    neighborhood_runtime
                        .members
                        .iter()
                        .find(|member| neighborhood_member_selection_key(member) == *selected_key)
                })
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
                .get(model.selected_authority_index().unwrap_or_default())
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

            if !controller.request_authority_switch(authority.id) {
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
                .get(model.selected_contact_index().unwrap_or_default())
                .cloned()
            else {
                controller.runtime_error_toast("Select a contact first");
                rerender();
                return true;
            };
            let authority_id = contact.authority_id;
            let selected_level = match model.active_modal.as_ref() {
                Some(ActiveModal::AccessOverride(state)) => state.level,
                _ => AccessOverrideLevel::Limited,
            };
            let access_level = match selected_level {
                AccessOverrideLevel::Partial => AccessLevel::Partial,
                AccessOverrideLevel::Limited => AccessLevel::Limited,
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

            let (full_caps, partial_caps, limited_caps) = match model.active_modal.as_ref() {
                Some(ActiveModal::CapabilityConfig(state)) => (
                    state.full_caps.clone(),
                    state.partial_caps.clone(),
                    state.limited_caps.clone(),
                ),
                _ => (
                    DEFAULT_CAPABILITY_FULL.to_string(),
                    DEFAULT_CAPABILITY_PARTIAL.to_string(),
                    DEFAULT_CAPABILITY_LIMITED.to_string(),
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
    add_device_ceremony_id: Option<CeremonyId>,
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

    let Some(_ceremony_id) = add_device_ceremony_id else {
        return false;
    };

    let app_core = controller.app_core().clone();
    let rerender_for_cancel = rerender.clone();
    spawn(async move {
        match controller.take_runtime_device_enrollment_ceremony() {
            Some(handle) => {
                match ceremony_workflows::cancel_key_rotation_ceremony(&app_core, handle).await {
                    Ok(()) => {
                        controller.complete_runtime_modal_success("Device enrollment canceled");
                        controller.clear_runtime_device_enrollment_ceremony();
                    }
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
            }
            None => controller.runtime_error_toast("No active enrollment ceremony handle"),
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

        let result: Result<Option<String>, aura_core::AuraError> = if let Some(command_input) =
            trimmed.strip_prefix('/')
        {
            let raw = format!("/{command_input}");
            match parse_chat_command(&raw) {
                Ok(ChatCommand::Join { channel }) => {
                    let channel_for_selection = channel.clone();
                    controller_for_task.push_log(&format!("chat_join: start channel={channel}"));
                    messaging_workflows::join_channel_by_name(&app_core, &channel)
                        .await
                        .map(|_| {
                            controller_for_task.select_channel_by_name(&channel_for_selection);
                            controller_for_task.push_runtime_fact(RuntimeFact::ChannelJoined {
                                channel: Some(ChannelFactKey::named(channel_for_selection.clone())),
                                source: Some("join_command".to_string()),
                            });
                            controller_for_task.push_log(&format!(
                                "chat_join: success channel={channel_for_selection} selected={channel_for_selection}"
                            ));
                            Some(format!("joined #{}", channel.trim_start_matches('#')))
                        })
                }
                Ok(ChatCommand::Leave) => {
                    messaging_workflows::leave_channel_by_name(&app_core, &channel_name)
                        .await
                        .map(|_| Some("left channel".to_string()))
                }
                Ok(ChatCommand::Topic { text }) => messaging_workflows::set_topic_by_name(
                    &app_core,
                    &channel_name,
                    &text,
                    timestamp_ms,
                )
                .await
                .map(|_| Some("topic updated".to_string())),
                Ok(ChatCommand::Me { action }) => messaging_workflows::send_action_by_name(
                    &app_core,
                    &channel_name,
                    &action,
                    timestamp_ms,
                )
                .await
                .map(|_| Some("action sent".to_string())),
                Ok(ChatCommand::Msg { target, text }) => messaging_workflows::send_direct_message(
                    &app_core,
                    &target,
                    &text,
                    timestamp_ms,
                )
                .await
                .map(|_| Some("direct message sent".to_string())),
                Ok(ChatCommand::Nick { name }) => {
                    settings_workflows::update_nickname(&app_core, name)
                        .await
                        .map(|_| Some("nickname updated".to_string()))
                }
                Ok(ChatCommand::Invite { target }) => messaging_workflows::invite_user_to_channel(
                    &app_core,
                    &target,
                    &channel_name,
                    None,
                    None,
                )
                .await
                .map(|_| Some("invitation sent".to_string())),
                Ok(ChatCommand::Who) => {
                    query_workflows::list_participants(&app_core, &channel_name)
                        .await
                        .map(|participants| {
                            Some(if participants.is_empty() {
                                "No participants".to_string()
                            } else {
                                participants.join(", ")
                            })
                        })
                }
                Ok(ChatCommand::Whois { target }) => {
                    query_workflows::get_user_info(&app_core, &target)
                        .await
                        .map(|contact| {
                            let id = contact.id.to_string();
                            let name = if !contact.nickname.is_empty() {
                                contact.nickname
                            } else if let Some(value) = contact.nickname_suggestion {
                                value
                            } else {
                                id.chars().take(8).collect::<String>() + "..."
                            };
                            Some(format!("User: {name} ({id})"))
                        })
                }
                Ok(ChatCommand::Help { command }) => Ok(Some(match command {
                    Some(command_name) => {
                        if let Some(help) = command_help(&command_name) {
                            format!(
                                "/{name} {syntax} — {description}",
                                name = help.name,
                                syntax = help.syntax,
                                description = help.description
                            )
                        } else {
                            format!("Unknown command: {command_name}")
                        }
                    }
                    None => {
                        let commands = all_command_help()
                            .into_iter()
                            .take(8)
                            .map(|help| format!("/{}", help.name))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("Common commands: {commands}. Use /help <command> for details.")
                    }
                })),
                Ok(ChatCommand::Neighborhood { name }) => {
                    context_workflows::create_neighborhood(&app_core, name)
                        .await
                        .map(|_| Some("neighborhood updated".to_string()))
                }
                Ok(ChatCommand::NhAdd { home_id }) => {
                    context_workflows::add_home_to_neighborhood(&app_core, &home_id)
                        .await
                        .map(|_| Some("home added to neighborhood".to_string()))
                }
                Ok(ChatCommand::NhLink { home_id }) => {
                    context_workflows::link_home_one_hop_link(&app_core, &home_id)
                        .await
                        .map(|_| Some("home one-hop link linked".to_string()))
                }
                Ok(ChatCommand::HomeInvite { target }) => {
                    let home_id =
                        match context_workflows::current_home_id_or_fallback(&app_core).await {
                            Ok(home_id) => home_id.to_string(),
                            Err(error) => {
                                controller_for_task.runtime_error_toast(error.to_string());
                                rerender();
                                return;
                            }
                        };
                    let target_authority =
                        match query_workflows::resolve_contact(&app_core, &target).await {
                            Ok(contact) => contact.id,
                            Err(_) => match target.parse::<AuthorityId>() {
                                Ok(authority_id) => authority_id,
                                Err(error) => {
                                    controller_for_task.runtime_error_toast(format!(
                                        "Invalid authority id: {error}"
                                    ));
                                    rerender();
                                    return;
                                }
                            },
                        };
                    invitation_workflows::create_channel_invitation(
                        &app_core,
                        target_authority,
                        home_id,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .await
                    .map(|_| Some("home invitation sent".to_string()))
                }
                Ok(ChatCommand::HomeAccept) => {
                    invitation_workflows::accept_pending_home_invitation(&app_core)
                        .await
                        .map(|_| Some("home invitation accepted".to_string()))
                }
                Ok(ChatCommand::Kick { target, reason }) => moderation_workflows::kick_user(
                    &app_core,
                    &channel_name,
                    &target,
                    reason.as_deref(),
                    timestamp_ms,
                )
                .await
                .map(|_| Some("kick applied".to_string())),
                Ok(ChatCommand::Ban { target, reason }) => moderation_workflows::ban_user(
                    &app_core,
                    Some(&channel_name),
                    &target,
                    reason.as_deref(),
                    timestamp_ms,
                )
                .await
                .map(|_| Some("ban applied".to_string())),
                Ok(ChatCommand::Unban { target }) => {
                    moderation_workflows::unban_user(&app_core, Some(&channel_name), &target)
                        .await
                        .map(|_| Some("unban applied".to_string()))
                }
                Ok(ChatCommand::Mute { target, duration }) => moderation_workflows::mute_user(
                    &app_core,
                    Some(&channel_name),
                    &target,
                    duration.map(|value| value.as_secs()),
                    timestamp_ms,
                )
                .await
                .map(|_| Some("mute applied".to_string())),
                Ok(ChatCommand::Unmute { target }) => {
                    moderation_workflows::unmute_user(&app_core, Some(&channel_name), &target)
                        .await
                        .map(|_| Some("unmute applied".to_string()))
                }
                Ok(ChatCommand::Pin { message_id }) => {
                    moderation_workflows::pin_message(&app_core, &message_id)
                        .await
                        .map(|_| Some("message pinned".to_string()))
                }
                Ok(ChatCommand::Unpin { message_id }) => {
                    moderation_workflows::unpin_message(&app_core, &message_id)
                        .await
                        .map(|_| Some("message unpinned".to_string()))
                }
                Ok(ChatCommand::Op { target }) => {
                    moderator_workflows::grant_moderator(&app_core, Some(&channel_name), &target)
                        .await
                        .map(|_| Some("moderator granted".to_string()))
                }
                Ok(ChatCommand::Deop { target }) => {
                    moderator_workflows::revoke_moderator(&app_core, Some(&channel_name), &target)
                        .await
                        .map(|_| Some("moderator revoked".to_string()))
                }
                Ok(ChatCommand::Mode { channel, flags }) => {
                    settings_workflows::set_channel_mode(&app_core, channel, flags)
                        .await
                        .map(|_| Some("channel mode updated".to_string()))
                }
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
            .map(|_| {
                controller_for_task.push_runtime_fact(RuntimeFact::MessageCommitted {
                    channel: ChannelFactKey::named(channel_name.clone()),
                    content: trimmed.clone(),
                });
                None
            })
        };

        match result {
            Ok(message) => {
                controller_for_task.clear_input_buffer();
                if let Some(message) = message {
                    controller_for_task.push_log(&format!("chat_command: ok message={message}"));
                    controller_for_task.info_toast(message);
                }
            }
            Err(error) => {
                controller_for_task.push_log(&format!("chat_command: error {error}"));
                controller_for_task.runtime_error_toast(error.to_string());
            }
        }
        rerender();
    });

    controller.clear_input_buffer();
    true
}

fn handle_runtime_character_shortcut(
    controller: Arc<UiController>,
    model: &UiModel,
    neighborhood_runtime: &NeighborhoodRuntimeView,
    key: &str,
    rerender: Arc<dyn Fn() + Send + Sync>,
) -> bool {
    if model.input_mode || model.modal_state().is_some() {
        return false;
    }

    match (model.screen, key) {
        (ScreenId::Neighborhood, "m") => {
            let app_core = controller.app_core().clone();
            spawn(async move {
                match context_workflows::create_neighborhood(&app_core, "Neighborhood".to_string())
                    .await
                {
                    Ok(_) => controller.info_toast("Neighborhood ready"),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender();
            });
            true
        }
        (ScreenId::Neighborhood, "v") => {
            let Some(home_id) = selected_home_id_for_modal(neighborhood_runtime, model) else {
                controller.runtime_error_toast("Select a home first");
                rerender();
                return true;
            };
            let app_core = controller.app_core().clone();
            spawn(async move {
                match context_workflows::add_home_to_neighborhood(&app_core, &home_id).await {
                    Ok(_) => controller.info_toast("Home added to neighborhood"),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender();
            });
            true
        }
        (ScreenId::Neighborhood, "L") => {
            let Some(home_id) = selected_home_id_for_modal(neighborhood_runtime, model) else {
                controller.runtime_error_toast("Select a home first");
                rerender();
                return true;
            };
            let app_core = controller.app_core().clone();
            spawn(async move {
                match context_workflows::link_home_one_hop_link(&app_core, &home_id).await {
                    Ok(_) => controller.info_toast("Direct one-hop link created"),
                    Err(error) => controller.runtime_error_toast(error.to_string()),
                }
                rerender();
            });
            true
        }
        _ => false,
    }
}

#[component]
pub fn AuraUiRoot(controller: Arc<UiController>) -> Element {
    rsx! {
        ThemeProvider {
            theme: themes::neutral(),
            color_scheme: ColorScheme::Dark,
            style {
                r#"
                [data-slot="toaster"] {{
                    z-index: 2147483647 !important;
                    isolation: isolate !important;
                }}

                [data-slot="toast"] {{
                    z-index: 2147483647 !important;
                    min-height: 5rem !important;
                    padding-top: 1.25rem !important;
                    padding-bottom: 1.25rem !important;
                }}
                "#
            }
            div {
                id: ControlId::ToastRegion
                    .web_dom_id()
                    .required_dom_id("ToastRegion must define a web DOM id"),
                style: "--normal-bg: var(--popover); --normal-text: var(--popover-foreground); --normal-border: var(--border); position: relative; z-index: 2147483647;",
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
    let render_tick_value = render_tick();
    let mut last_toast_key = use_signal(|| None::<String>);
    let mut last_chat_selection_key = use_signal(|| None::<String>);
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

    let controller_for_chat_selection = controller.clone();
    let mut chat_for_selection_change = chat_runtime;
    use_effect(move || {
        let _ = render_tick();
        let Some(current_model) = controller_for_chat_selection.ui_model() else {
            return;
        };
        let selected_channel_key = current_model.selected_channel_name().map(str::to_string);
        if last_chat_selection_key() == selected_channel_key {
            return;
        }

        last_chat_selection_key.set(selected_channel_key);

        let controller_for_reload = controller_for_chat_selection.clone();
        spawn(async move {
            chat_for_selection_change
                .set(load_chat_runtime_view(controller_for_reload.clone()).await);
            controller_for_reload.request_rerender();
        });
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
            chat_for_initial.set(load_chat_runtime_view(controller_for_chat_initial.clone()).await);
            controller_for_chat_initial.request_rerender();
        });

        let mut contacts_for_initial = contacts_runtime;
        let controller_for_contacts_initial = controller_for_runtime.clone();
        spawn(async move {
            contacts_for_initial
                .set(load_contacts_runtime_view(controller_for_contacts_initial.clone()).await);
            controller_for_contacts_initial.request_rerender();
        });

        let mut settings_for_initial = settings_runtime;
        let controller_for_settings_initial = controller_for_runtime.clone();
        spawn(async move {
            settings_for_initial
                .set(load_settings_runtime_view(controller_for_settings_initial.clone()).await);
            controller_for_settings_initial.request_rerender();
        });

        let mut notifications_for_initial = notifications_runtime;
        let controller_for_notifications_initial = controller_for_runtime.clone();
        spawn(async move {
            notifications_for_initial.set(
                load_notifications_runtime_view(controller_for_notifications_initial.clone()).await,
            );
            controller_for_notifications_initial.request_rerender();
        });

        let mut runtime_for_neighborhood = neighborhood_runtime;
        let controller_for_neighborhood = controller_for_runtime.clone();
        spawn(async move {
            let Ok(mut stream) = ({
                let core = controller_for_neighborhood.app_core().read().await;
                core.subscribe(&*NEIGHBORHOOD_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                runtime_for_neighborhood
                    .set(load_neighborhood_runtime_view(controller_for_neighborhood.clone()).await);
                controller_for_neighborhood.request_rerender();
            }
        });

        let mut runtime_for_homes = neighborhood_runtime;
        let controller_for_homes = controller_for_runtime.clone();
        spawn(async move {
            let Ok(mut stream) = ({
                let core = controller_for_homes.app_core().read().await;
                core.subscribe(&*HOMES_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                runtime_for_homes
                    .set(load_neighborhood_runtime_view(controller_for_homes.clone()).await);
                controller_for_homes.request_rerender();
            }
        });

        let mut runtime_for_contacts = neighborhood_runtime;
        let controller_for_contacts = controller_for_runtime.clone();
        spawn(async move {
            let Ok(mut stream) = ({
                let core = controller_for_contacts.app_core().read().await;
                core.subscribe(&*CONTACTS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                runtime_for_contacts
                    .set(load_neighborhood_runtime_view(controller_for_contacts.clone()).await);
                controller_for_contacts.request_rerender();
            }
        });

        let mut contacts_for_contacts_signal = contacts_runtime;
        let controller_for_contacts_signal = controller_for_runtime.clone();
        spawn(async move {
            let Ok(mut stream) = ({
                let core = controller_for_contacts_signal.app_core().read().await;
                core.subscribe(&*CONTACTS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                contacts_for_contacts_signal
                    .set(load_contacts_runtime_view(controller_for_contacts_signal.clone()).await);
                controller_for_contacts_signal.request_rerender();
            }
        });

        let mut contacts_for_discovered_peers = contacts_runtime;
        let controller_for_discovered_peers = controller_for_runtime.clone();
        spawn(async move {
            let Ok(mut stream) = ({
                let core = controller_for_discovered_peers.app_core().read().await;
                core.subscribe(&*DISCOVERED_PEERS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                contacts_for_discovered_peers
                    .set(load_contacts_runtime_view(controller_for_discovered_peers.clone()).await);
                controller_for_discovered_peers.request_rerender();
            }
        });

        let mut runtime_for_chat = neighborhood_runtime;
        let controller_for_chat = controller_for_runtime.clone();
        spawn(async move {
            let Ok(mut stream) = ({
                let core = controller_for_chat.app_core().read().await;
                core.subscribe(&*CHAT_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                runtime_for_chat
                    .set(load_neighborhood_runtime_view(controller_for_chat.clone()).await);
                controller_for_chat.request_rerender();
            }
        });

        let mut chat_for_chat_signal = chat_runtime;
        let controller_for_chat_signal = controller_for_runtime.clone();
        spawn(async move {
            let Ok(mut stream) = ({
                let core = controller_for_chat_signal.app_core().read().await;
                core.subscribe(&*CHAT_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                chat_for_chat_signal
                    .set(load_chat_runtime_view(controller_for_chat_signal.clone()).await);
                controller_for_chat_signal.request_rerender();
            }
        });

        let mut runtime_for_network = neighborhood_runtime;
        let controller_for_network = controller_for_runtime.clone();
        spawn(async move {
            let Ok(mut stream) = ({
                let core = controller_for_network.app_core().read().await;
                core.subscribe(&*NETWORK_STATUS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                runtime_for_network
                    .set(load_neighborhood_runtime_view(controller_for_network.clone()).await);
                controller_for_network.request_rerender();
            }
        });

        let mut runtime_for_transport = neighborhood_runtime;
        let controller_for_transport = controller_for_runtime.clone();
        spawn(async move {
            let Ok(mut stream) = ({
                let core = controller_for_transport.app_core().read().await;
                core.subscribe(&*TRANSPORT_PEERS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                runtime_for_transport
                    .set(load_neighborhood_runtime_view(controller_for_transport.clone()).await);
                controller_for_transport.request_rerender();
            }
        });

        let mut settings_for_settings_signal = settings_runtime;
        let controller_for_settings_signal = controller_for_runtime.clone();
        spawn(async move {
            let Ok(mut stream) = ({
                let core = controller_for_settings_signal.app_core().read().await;
                core.subscribe(&*SETTINGS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                settings_for_settings_signal
                    .set(load_settings_runtime_view(controller_for_settings_signal.clone()).await);
                controller_for_settings_signal.request_rerender();
            }
        });

        let mut settings_for_recovery_signal = settings_runtime;
        let controller_for_recovery_signal = controller_for_runtime.clone();
        spawn(async move {
            let Ok(mut stream) = ({
                let core = controller_for_recovery_signal.app_core().read().await;
                core.subscribe(&*RECOVERY_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                settings_for_recovery_signal
                    .set(load_settings_runtime_view(controller_for_recovery_signal.clone()).await);
                controller_for_recovery_signal.request_rerender();
            }
        });

        let mut notifications_for_invites = notifications_runtime;
        let controller_for_invites = controller_for_runtime.clone();
        spawn(async move {
            let Ok(mut stream) = ({
                let core = controller_for_invites.app_core().read().await;
                core.subscribe(&*INVITATIONS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                notifications_for_invites
                    .set(load_notifications_runtime_view(controller_for_invites.clone()).await);
                controller_for_invites.request_rerender();
            }
        });

        let mut notifications_for_recovery = notifications_runtime;
        let controller_for_notifications_recovery = controller_for_runtime.clone();
        spawn(async move {
            let Ok(mut stream) = ({
                let core = controller_for_notifications_recovery
                    .app_core()
                    .read()
                    .await;
                core.subscribe(&*RECOVERY_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                notifications_for_recovery.set(
                    load_notifications_runtime_view(controller_for_notifications_recovery.clone())
                        .await,
                );
                controller_for_notifications_recovery.request_rerender();
            }
        });

        let mut notifications_for_errors = notifications_runtime;
        let controller_for_errors = controller_for_runtime.clone();
        spawn(async move {
            let Ok(mut stream) = ({
                let core = controller_for_errors.app_core().read().await;
                core.subscribe(&*ERROR_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                notifications_for_errors
                    .set(load_notifications_runtime_view(controller_for_errors.clone()).await);
                controller_for_errors.request_rerender();
            }
        });

        let controller_for_authoritative_operations = controller_for_runtime.clone();
        spawn(async move {
            let Ok(mut stream) = ({
                let core = controller_for_authoritative_operations
                    .app_core()
                    .read()
                    .await;
                core.subscribe(&*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL)
            }) else {
                return;
            };

            while stream.recv().await.is_ok() {
                let facts = {
                    let core = controller_for_authoritative_operations
                        .app_core()
                        .read()
                        .await;
                    core.read(&*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL)
                        .await
                        .unwrap_or_default()
                };
                for (operation_id, _instance_id, _causality, status) in
                    bridged_operation_statuses(&facts)
                {
                    controller_for_authoritative_operations
                        .apply_authoritative_operation_status(operation_id, status);
                }
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
    let should_exit_insert_mode_from_shell =
        matches!(model.screen, ScreenId::Chat) && model.input_mode;
    let shell_header_exit_input_controller = controller.clone();
    let shell_footer_exit_input_controller = controller.clone();
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
    let keydown_selected_member_key = model.selected_neighborhood_member_key.clone();
    let modal_selected_member_key = model.selected_neighborhood_member_key.clone();
    let modal = modal_view(&model, &chat_runtime_snapshot);
    let modal_state = model.modal_state();
    let add_device_modal_state = model.add_device_modal().cloned();
    let cancel_add_device_ceremony_id = add_device_modal_state
        .as_ref()
        .and_then(|state| state.ceremony_id.clone());
    let rerender = schedule_update();
    controller.set_rerender_callback(rerender.clone());
    let keydown_rerender = rerender.clone();
    let cancel_rerender = rerender.clone();
    let dedicated_primary_rerender = rerender.clone();
    let generic_confirm_rerender = rerender.clone();
    let semantic_snapshot = runtime_semantic_snapshot(
        &model,
        &runtime_snapshot,
        &chat_runtime_snapshot,
        &contacts_runtime_snapshot,
        &settings_runtime_snapshot,
        &notifications_runtime_snapshot,
    );
    controller.set_ui_snapshot(semantic_snapshot);
    let keydown_controller = controller.clone();
    rsx! {
        main {
            id: ControlId::AppRoot
                .web_dom_id()
                .required_dom_id("AppRoot must define a web DOM id"),
            "data-render-tick": "{render_tick_value}",
            class: "relative flex min-h-screen flex-col overflow-y-auto bg-background text-foreground font-sans outline-none lg:h-[100dvh] lg:min-h-[100dvh] lg:overflow-hidden",
            tabindex: 0,
            autofocus: true,
            onmounted: move |mounted| {
                spawn(async move {
                    let _ = mounted.data().set_focus(true).await;
                });
            },
            onkeydown: move |event| {
                if should_skip_global_key(keydown_controller.as_ref(), event.data().as_ref()) {
                    return;
                }
                if let Key::Character(text) = event.data().key() {
                    if handle_runtime_character_shortcut(
                        keydown_controller.clone(),
                        &model,
                        &keydown_runtime_snapshot,
                        &text,
                        keydown_rerender.clone(),
                    ) {
                        event.prevent_default();
                        return;
                    }
                }
                if matches!(event.data().key(), Key::Enter)
                    && matches!(model.screen, ScreenId::Chat)
                    && model.input_mode
                    && submit_runtime_chat_input(
                        keydown_controller.clone(),
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
                                keydown_controller.clone(),
                                modal_state,
                                add_device_modal_state
                                    .as_ref()
                                    .map(|state| state.step)
                                    .unwrap_or(AddDeviceWizardStep::Name),
                                add_device_modal_state
                                    .as_ref()
                                    .and_then(|state| state.ceremony_id.clone()),
                                add_device_modal_state
                                    .as_ref()
                                    .map(|state| state.is_complete)
                                    .unwrap_or(false),
                                add_device_modal_state
                                    .as_ref()
                                    .map(|state| state.has_failed)
                                    .unwrap_or(false),
                                model.modal_text_value().unwrap_or_default(),
                        keydown_runtime_snapshot.clone(),
                        keydown_chat_runtime.clone(),
                        keydown_contacts_runtime.clone(),
                        keydown_settings_runtime.clone(),
                        selected_home_id_for_modal(&keydown_runtime_snapshot, &keydown_model),
                        keydown_selected_member_key.clone(),
                        keydown_rerender.clone(),
                    )
                {
                    event.prevent_default();
                    return;
                }
                if handle_keydown(keydown_controller.as_ref(), event.data().as_ref()) {
                    event.prevent_default();
                    render_tick.set(render_tick() + 1);
                }
            },
            nav {
                id: ControlId::NavRoot
                    .web_dom_id()
                    .required_dom_id("NavRoot must define a web DOM id"),
                class: "bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/80",
                onclick: move |_| {
                    if should_exit_insert_mode_from_shell {
                        shell_header_exit_input_controller.exit_input_mode();
                        render_tick.set(render_tick() + 1);
                    }
                },
                div {
                    class: "relative flex items-end px-4 pt-6 pb-0 sm:px-6",
                    div {
                        class: "absolute bottom-0 left-4 z-10 flex items-center justify-start gap-3 sm:left-6",
                        button {
                            r#type: "button",
                            id: "aura-nav-brand",
                            class: "inline-flex h-8 items-center justify-center whitespace-nowrap px-6 text-xs font-sans font-bold uppercase leading-none tracking-[0.12em] text-foreground cursor-pointer hover:text-muted-foreground transition-colors",
                            onclick: {
                                move |_| {
                                    controller.set_screen(ScreenId::Neighborhood);
                                    render_tick.set(render_tick() + 1);
                                }
                            },
                            "AURA"
                        }
                    }
                    div {
                        class: "w-full min-w-0 overflow-x-auto px-16 [::-webkit-scrollbar]:hidden sm:px-24",
                        div {
                            class: "mx-auto flex h-8 min-w-max items-center justify-center gap-2",
                            for (screen, label, is_active) in screen_tabs(model.screen) {
                                button {
                                    r#type: "button",
                                    id: nav_button_id(screen),
                                    class: nav_tab_class(is_active),
                                    onclick: {
                                        let controller = controller.clone();
                                        move |_| {
                                            let before_log = format!(
                                                "nav_click start screen={}",
                                                screen.help_label()
                                            );
                                            controller.push_log(&before_log);
                                            harness_log(&before_log);
                                            controller.set_screen(screen);
                                            let after_log = format!(
                                                "nav_click done screen={}",
                                                screen.help_label()
                                            );
                                            controller.push_log(&after_log);
                                            harness_log(&after_log);
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
                class: "flex-1 px-4 py-4 sm:px-6 sm:py-5 lg:min-h-0 lg:overflow-hidden",
                {render_screen_content(
                    &model,
                    &runtime_snapshot,
                    &chat_runtime_snapshot,
                    &contacts_runtime_snapshot,
                    &settings_runtime_snapshot,
                    &notifications_runtime_snapshot,
                    controller.clone(),
                    render_tick,
                    theme,
                    resolved_scheme,
                )}
            }

            div {
                id: ControlId::ModalRegion
                    .web_dom_id()
                    .required_dom_id("ModalRegion must define a web DOM id"),
                class: "contents",
                if let Some(modal) = modal {
                    if let Some(add_device_state) = model.add_device_modal() {
                        if !matches!(add_device_state.step, AddDeviceWizardStep::Name) {
                            UiDeviceEnrollmentModal {
                            modal_id: ModalId::AddDevice,
                            title: if matches!(add_device_state.step, AddDeviceWizardStep::ShareCode) {
                                "Add Device — Step 2 of 3".to_string()
                            } else {
                                "Add Device — Step 3 of 3".to_string()
                            },
                            enrollment_code: add_device_state.enrollment_code.clone(),
                            ceremony_id: add_device_state
                                .ceremony_id
                                .as_ref()
                                .map(ToString::to_string),
                            device_name: add_device_state.device_name.clone(),
                            accepted_count: add_device_state.accepted_count,
                            total_count: add_device_state.total_count,
                            threshold: add_device_state.threshold,
                            is_complete: add_device_state.is_complete,
                            has_failed: add_device_state.has_failed,
                            error_message: add_device_state.error_message.clone(),
                            copied: add_device_state.code_copied,
                            primary_label: if matches!(add_device_state.step, AddDeviceWizardStep::ShareCode) {
                                "Next".to_string()
                            } else if add_device_state.is_complete || add_device_state.has_failed {
                                "Close".to_string()
                            } else {
                                "Refresh".to_string()
                            },
                            on_cancel: {
                                let controller = controller.clone();
                                let add_device_modal_state = add_device_modal_state.clone();
                                move |_| {
                                    if !cancel_runtime_modal_action(
                                        controller.clone(),
                                        modal_state,
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.step)
                                            .unwrap_or(AddDeviceWizardStep::Name),
                                        cancel_add_device_ceremony_id.clone(),
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.is_complete)
                                            .unwrap_or(false),
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.has_failed)
                                            .unwrap_or(false),
                                        cancel_rerender.clone(),
                                    ) {
                                        controller.send_key_named("esc", 1);
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            },
                            on_copy: {
                                let controller = controller.clone();
                                let enrollment_code = add_device_state.enrollment_code.clone();
                                move |_| {
                                    controller.write_clipboard(&enrollment_code);
                                    controller.mark_add_device_code_copied();
                                    controller.info_toast("Copied to clipboard");
                                    render_tick.set(render_tick() + 1);
                                }
                            },
                            on_primary: {
                                let controller = controller.clone();
                                let add_device_modal_state = add_device_modal_state.clone();
                                move |_| {
                                    if !submit_runtime_modal_action(
                                        controller.clone(),
                                        modal_state,
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.step)
                                            .unwrap_or(AddDeviceWizardStep::Name),
                                        add_device_modal_state
                                            .as_ref()
                                            .and_then(|state| state.ceremony_id.clone()),
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.is_complete)
                                            .unwrap_or(false),
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.has_failed)
                                            .unwrap_or(false),
                                        modal_model.modal_text_value().unwrap_or_default(),
                                        modal_runtime_snapshot.clone(),
                                        modal_chat_runtime.clone(),
                                        modal_contacts_runtime.clone(),
                                        modal_settings_runtime.clone(),
                                        selected_home_id_for_modal(&modal_runtime_snapshot, &modal_model),
                                        modal_selected_member_key.clone(),
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
                                let add_device_modal_state = add_device_modal_state.clone();
                                move |_| {
                                    if !cancel_runtime_modal_action(
                                        controller.clone(),
                                        modal_state,
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.step)
                                            .unwrap_or(AddDeviceWizardStep::Name),
                                        cancel_add_device_ceremony_id.clone(),
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.is_complete)
                                            .unwrap_or(false),
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.has_failed)
                                            .unwrap_or(false),
                                        cancel_rerender.clone(),
                                    ) {
                                        controller.send_key_named("esc", 1);
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            },
                            on_confirm: {
                                let controller = controller.clone();
                                let add_device_modal_state = add_device_modal_state.clone();
                                move |_| {
                                    if !submit_runtime_modal_action(
                                        controller.clone(),
                                        modal_state,
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.step)
                                            .unwrap_or(AddDeviceWizardStep::Name),
                                        add_device_modal_state
                                            .as_ref()
                                            .and_then(|state| state.ceremony_id.clone()),
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.is_complete)
                                            .unwrap_or(false),
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.has_failed)
                                            .unwrap_or(false),
                                        modal_model.modal_text_value().unwrap_or_default(),
                                        modal_runtime_snapshot.clone(),
                                        modal_chat_runtime.clone(),
                                        modal_contacts_runtime.clone(),
                                        modal_settings_runtime.clone(),
                                        selected_home_id_for_modal(&modal_runtime_snapshot, &modal_model),
                                        modal_selected_member_key.clone(),
                                        generic_confirm_rerender.clone(),
                                    ) {
                                        controller.send_key_named("enter", 1);
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            },
                            on_input_change: {
                                let controller = controller.clone();
                                move |(field_id, value): (FieldId, String)| {
                                    controller.set_modal_field_value(field_id, &value);
                                    render_tick.set(render_tick() + 1);
                                }
                            },
                            on_input_focus: {
                                let controller = controller.clone();
                                move |field_id: FieldId| {
                                    controller.set_modal_active_field(field_id);
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                        }
                    } else if matches!(modal_state, Some(ModalState::SwitchAuthority)) {
                        UiAuthorityPickerModal {
                        modal_id: ModalId::SwitchAuthority,
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
                                id: authority.id,
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
                            let add_device_modal_state = add_device_modal_state.clone();
                            move |_| {
                                if !submit_runtime_modal_action(
                                    controller.clone(),
                                    modal_state,
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.step)
                                        .unwrap_or(AddDeviceWizardStep::Name),
                                    add_device_modal_state
                                        .as_ref()
                                        .and_then(|state| state.ceremony_id.clone()),
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.is_complete)
                                        .unwrap_or(false),
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.has_failed)
                                        .unwrap_or(false),
                                    modal_model.modal_text_value().unwrap_or_default(),
                                    modal_runtime_snapshot.clone(),
                                    modal_chat_runtime.clone(),
                                    modal_contacts_runtime.clone(),
                                    modal_settings_runtime.clone(),
                                    selected_home_id_for_modal(&modal_runtime_snapshot, &modal_model),
                                    modal_selected_member_key.clone(),
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
                            let add_device_modal_state = add_device_modal_state.clone();
                            move |_| {
                                if !cancel_runtime_modal_action(
                                    controller.clone(),
                                    modal_state,
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.step)
                                        .unwrap_or(AddDeviceWizardStep::Name),
                                    cancel_add_device_ceremony_id.clone(),
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.is_complete)
                                        .unwrap_or(false),
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.has_failed)
                                        .unwrap_or(false),
                                    cancel_rerender.clone(),
                                ) {
                                    controller.send_key_named("esc", 1);
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        },
                        on_confirm: {
                            let controller = controller.clone();
                            let add_device_modal_state = add_device_modal_state.clone();
                            move |_| {
                                if !submit_runtime_modal_action(
                                    controller.clone(),
                                    modal_state,
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.step)
                                        .unwrap_or(AddDeviceWizardStep::Name),
                                    add_device_modal_state
                                        .as_ref()
                                        .and_then(|state| state.ceremony_id.clone()),
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.is_complete)
                                        .unwrap_or(false),
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.has_failed)
                                        .unwrap_or(false),
                                    modal_model.modal_text_value().unwrap_or_default(),
                                    modal_runtime_snapshot.clone(),
                                    modal_chat_runtime.clone(),
                                    modal_contacts_runtime.clone(),
                                    modal_settings_runtime.clone(),
                                    selected_home_id_for_modal(&modal_runtime_snapshot, &modal_model),
                                    modal_selected_member_key.clone(),
                                    generic_confirm_rerender.clone(),
                                ) {
                                    controller.send_key_named("enter", 1);
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        },
                        on_input_change: {
                            let controller = controller.clone();
                            move |(field_id, value): (FieldId, String)| {
                                controller.set_modal_field_value(field_id, &value);
                                render_tick.set(render_tick() + 1);
                            }
                        },
                        on_input_focus: {
                            let controller = controller.clone();
                            move |field_id: FieldId| {
                                controller.set_modal_active_field(field_id);
                                render_tick.set(render_tick() + 1);
                            }
                        }
                        }
                    }
                }
            }

            div {
                onclick: move |_| {
                    if should_exit_insert_mode_from_shell {
                        shell_footer_exit_input_controller.exit_input_mode();
                        render_tick.set(render_tick() + 1);
                    }
                },
                UiFooter {
                    left: String::new(),
                    network_status: footer_network_status,
                    peer_count: footer_peer_count,
                    online_count: footer_online_count,
                }
            }
        }
    }
}

#[allow(non_snake_case)]
fn NeighborhoodScreen(
    model: &UiModel,
    runtime: &NeighborhoodRuntimeView,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
) -> Element {
    let is_detail = matches!(model.neighborhood_mode, NeighborhoodMode::Detail);
    let selected_home = model
        .selected_home_name()
        .map(str::to_string)
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
        if let Some(home) = model.selected_home.as_ref() {
            home_rows.push(NeighborhoodRuntimeHome {
                id: home.id.clone(),
                name: home.name.clone(),
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
            id: model
                .selected_home_id()
                .map(str::to_string)
                .unwrap_or_else(|| {
                    if selected_home == "Neighborhood" {
                        model.authority_id.clone()
                    } else {
                        format!("home-{}", selected_home.to_lowercase().replace(' ', "-"))
                    }
                }),
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
        .selected_neighborhood_member_key
        .as_ref()
        .and_then(|selected| {
            display_members
                .iter()
                .position(|member| neighborhood_member_selection_key(member) == *selected)
        })
        .unwrap_or(0)
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
            class: "grid w-full gap-3 lg:grid-cols-12 lg:h-full lg:min-h-0 lg:[grid-template-rows:minmax(0,1fr)]",
            UiCard {
                title: if is_detail { "Home".to_string() } else { "Map".to_string() },
                subtitle: Some(if is_detail {
                    format!("Access: {access_label} ({hop_hint})")
                } else {
                    "Explore your neighboring network".to_string()
                }),
                extra_class: Some("lg:col-span-4".to_string()),
                if is_detail {
                    UiCardBody {
                        extra_class: Some("gap-3".to_string()),
                        div {
                            class: "rounded-sm bg-background/60 px-3 py-3",
                            p { class: "m-0 text-xs font-sans font-semibold uppercase tracking-[0.08em] text-muted-foreground", "Home" }
                            p { class: "m-0 mt-1 text-sm text-foreground", "Name: {selected_home}" }
                            p {
                                class: "m-0 mt-1 text-xs text-muted-foreground",
                                "Members/Participants: {member_count} • Access: {access_label} • Mode: {social_mode_label}"
                            }
                        }
                        if show_detail_lists {
                            div {
                                class: "grid flex-1 gap-4 lg:min-h-0 md:grid-cols-2",
                                div {
                                    class: "flex lg:min-h-0 min-w-0 flex-col overflow-hidden rounded-sm bg-background/60 px-3 py-3",
                                    p { class: "m-0 text-xs font-sans font-semibold uppercase tracking-[0.08em] text-muted-foreground", "Channels" }
                                    div {
                                        class: "mt-3 flex-1 lg:min-h-0 min-w-0 overflow-y-auto pr-1",
                                        if display_channels.is_empty() {
                                            p { class: "m-0 text-sm text-muted-foreground", "No channels" }
                                        } else {
                                            div { class: "aura-list space-y-2 min-w-0",
                                for (channel_name, channel_topic, is_selected) in &display_channels {
                                                    button {
                                                        r#type: "button",
                                                        id: list_item_dom_id(ListId::Channels, channel_name),
                                                        class: "block w-full min-w-0 text-left",
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
                                    class: "flex lg:min-h-0 flex-col rounded-sm bg-background/60 px-3 py-3",
                                    p { class: "m-0 text-xs font-sans font-semibold uppercase tracking-[0.08em] text-muted-foreground", "Members & Participants" }
                                    div {
                                        class: "mt-3 flex-1 lg:min-h-0 overflow-y-auto pr-1",
                                        div { class: "aura-list space-y-2",
                                            for (idx, member) in display_members.iter().enumerate() {
                                                button {
                                                    r#type: "button",
                                                    id: list_item_dom_id(
                                                        ListId::NeighborhoodMembers,
                                                        &neighborhood_member_selection_key(member).0,
                                                    ),
                                                    class: "block w-full text-left",
                                                    onclick: {
                                                        let controller = controller.clone();
                                                        let member_key = neighborhood_member_selection_key(member);
                                                        move |_| {
                                                            controller.set_selected_neighborhood_member_key(Some(member_key.clone()));
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
                                class: "flex flex-1 items-center justify-center rounded-sm bg-background/40 px-4 py-6 text-center",
                                p {
                                    class: "m-0 text-sm text-muted-foreground",
                                    "Partial/Limited view: full channel and membership details are hidden until Full access is active."
                                }
                            }
                        }
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                                UiButton {
                                    label: "Back To Map".to_string(),
                                    variant: ButtonVariant::Secondary,
                                    onclick: move |_| {
                                        detail_back_controller.send_key_named("esc", 1);
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                                UiButton {
                                    label: format!("Enter As: {access_label}"),
                                    variant: ButtonVariant::Secondary,
                                    width_class: Some("w-[9rem]".to_string()),
                                    onclick: move |_| {
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
                                        width_class: Some("w-[10rem]".to_string()),
                                        onclick: move |_| {
                                            detail_moderator_controller.send_action_keys("o");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                    UiButton {
                                        label: "Access Override".to_string(),
                                        variant: ButtonVariant::Secondary,
                                        onclick: move |_| {
                                            detail_access_override_controller.send_action_keys("x");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                    UiButton {
                                        label: "Capability Config".to_string(),
                                        variant: ButtonVariant::Secondary,
                                        onclick: move |_| {
                                            detail_capability_controller.send_action_keys("p");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else {
                    UiCardBody {
                        extra_class: Some("gap-3".to_string()),
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
                                class: Some("flex-1 min-h-[16rem] border-0 bg-background/40".to_string()),
                                EmptyHeader {
                                    EmptyTitle { "No home yet" }
                                    EmptyDescription { "Create a new home or accept an invitation to join." }
                                }
                            }
                        } else {
                            div {
                                class: "flex-1 lg:min-h-0 overflow-y-auto pr-1",
                                div { class: "aura-list space-y-2",
                                    for home in &home_rows {
                                        button {
                                            r#type: "button",
                                            id: list_item_dom_id(ListId::Homes, &home.id),
                                            class: "block w-full text-left",
                                            onclick: {
                                                let controller = controller.clone();
                                                let home_id = home.id.clone();
                                                let home_name = home.name.clone();
                                                move |_| {
                                                    controller.select_home(home_id.clone(), home_name.clone());
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
                            class: "rounded-sm bg-background/60 px-3 py-3",
                            p { class: "m-0 text-xs font-sans font-semibold uppercase tracking-[0.08em] text-muted-foreground", "Traversal" }
                            p {
                                class: "m-0 mt-1 text-sm text-foreground",
                                "Can enter: Limited, Partial, Full"
                            }
                            p {
                                class: "m-0 mt-1 text-xs text-muted-foreground",
                                "Current depth is {access_label} ({hop_hint}). Select a home, then enter it."
                            }
                        }
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                                if can_enter_selected_home {
                                    UiButton {
                                        label: "Enter Home".to_string(),
                                        variant: ButtonVariant::Primary,
                                        onclick: {
                                            let controller = map_enter_controller;
                                            let home_name = selected_home.clone();
                                            let depth = model.access_depth;
                                            let target_home_id = enter_target_home_id;
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
                                    id: Some(ControlId::NeighborhoodNewHomeButton.web_dom_id().required_dom_id("ControlId::NeighborhoodNewHomeButton must define a web DOM id").to_string()),
                                    label: "New Home".to_string(),
                                    variant: ButtonVariant::Primary,
                                    onclick: move |_| {
                                        map_new_home_controller.send_action_keys("n");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                                UiButton {
                                    id: Some(
                                        ControlId::NeighborhoodAcceptInvitationButton
                                            .web_dom_id()
                                            .required_dom_id("ControlId::NeighborhoodAcceptInvitationButton must define a web DOM id")
                                            .to_string(),
                                    ),
                                    label: "Accept Invitation".to_string(),
                                    variant: ButtonVariant::Secondary,
                                    onclick: move |_| {
                                        map_accept_invitation_controller.send_action_keys("a");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                                UiButton {
                                    id: Some(
                                        ControlId::NeighborhoodEnterAsButton
                                            .web_dom_id()
                                            .required_dom_id("ControlId::NeighborhoodEnterAsButton must define a web DOM id")
                                            .to_string(),
                                    ),
                                    label: format!("Enter As: {access_label}"),
                                    variant: ButtonVariant::Secondary,
                                    width_class: Some("w-[9rem]".to_string()),
                                    onclick: move |_| {
                                        map_depth_controller.send_action_keys("d");
                                        render_tick.set(render_tick() + 1);
                                    }
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
                    class: "aura-list grid gap-2 md:grid-cols-2 md:gap-x-5",
                    UiListItem {
                        label: format!("Neighborhood: {display_neighborhood_name}"),
                        secondary: Some(format!("Selected home: {selected_home}")),
                        active: false,
                    }
                    UiListItem {
                        label: format!("Home ID: {selected_home_id}"),
                        secondary: Some("Authority-scoped identifier".to_string()),
                        active: false,
                    }
                    UiListItem {
                        label: format!("Access: {access_label} ({hop_hint})"),
                        secondary: Some(format!(
                            "{social_mode_label} • {} access",
                            model.access_depth.label()
                        )),
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

#[allow(non_snake_case)]
fn ChatScreen(
    model: &UiModel,
    runtime: &ChatRuntimeView,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
) -> Element {
    let active_channel = model
        .selected_channel_name()
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            (!runtime.active_channel.trim().is_empty()).then(|| runtime.active_channel.clone())
        })
        .unwrap_or_else(|| NOTE_TO_SELF_CHANNEL_NAME.to_string());
    let topic = runtime
        .channels
        .iter()
        .find(|channel| channel.name.eq_ignore_ascii_case(&active_channel))
        .map(|channel| channel.topic.clone())
        .unwrap_or_else(|| model.selected_channel_topic().to_string());
    let is_input_mode = model.input_mode;
    let composer_text = model.input_buffer.clone();
    let new_group_controller = controller.clone();
    let composer_container_focus_controller = controller.clone();
    let composer_field_focus_controller = controller.clone();
    let composer_input_controller = controller.clone();
    let composer_keydown_controller = controller.clone();
    let send_message_controller = controller.clone();
    let exit_insert_mode_controller = controller.clone();
    let composer_value = composer_text.clone();
    let composer_active_channel = active_channel.clone();
    let composer_submit_text = composer_text.clone();
    let runtime_channels = if runtime.loaded {
        runtime.channels.clone()
    } else {
        model
            .channels
            .iter()
            .map(|channel| ChatRuntimeChannel {
                id: channel.name.clone(),
                name: channel.name.clone(),
                topic: channel.topic.clone(),
                unread_count: 0,
                last_message: None,
                member_count: 0,
                is_dm: false,
            })
            .collect()
    };

    rsx! {
        div {
            class: "grid w-full gap-3 lg:grid-cols-12 lg:h-full lg:min-h-0 lg:[grid-template-rows:minmax(0,1fr)]",
            onclick: move |_| {
                if is_input_mode {
                    exit_insert_mode_controller.exit_input_mode();
                    render_tick.set(render_tick() + 1);
                }
            },
            UiCard {
                title: "Channels".to_string(),
                subtitle: Some("E2EE and forward secure".to_string()),
                extra_class: Some("lg:col-span-4".to_string()),
                UiCardBody {
                    extra_class: Some("gap-2".to_string()),
                    ScrollArea {
                        class: Some("flex-1 lg:min-h-0 pr-1".to_string()),
                        ScrollAreaViewport {
                            class: Some("flex flex-col gap-2".to_string()),
                            for channel in &runtime_channels {
                                UiListButton {
                                    id: Some(list_item_dom_id(ListId::Channels, &channel.id)),
                                    label: channel.name.clone(),
                                    active: channel.name.eq_ignore_ascii_case(&active_channel),
                                    extra_class: Some("pt-px pb-0".to_string()),
                                    onclick: {
                                        let controller = controller.clone();
                                        let channel_name = channel.name.clone();
                                        move |_| {
                                            controller.select_channel_by_name(&channel_name);
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    UiCardFooter {
                        extra_class: None,
                        div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                            UiButton {
                                id: Some(
                                    ControlId::ChatNewGroupButton
                                        .web_dom_id()
                                        .required_dom_id("ControlId::ChatNewGroupButton must define a web DOM id")
                                        .to_string(),
                                ),
                                label: "New Group".to_string(),
                                variant: ButtonVariant::Primary,
                                onclick: move |_| {
                                    new_group_controller.send_action_keys("n");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                    }
                }
            }

            div {
                class: "lg:col-span-8 h-full min-h-0",
                onclick: move |event| event.stop_propagation(),
                UiCard {
                    title: active_channel.clone(),
                    subtitle: Some(if topic.is_empty() { "No topic set".to_string() } else { topic }),
                    extra_class: None,
                    UiCardBody {
                        extra_class: Some("!-mt-6".to_string()),
                        div {
                            class: "flex-1 lg:min-h-0 overflow-y-auto pr-1",
                            div {
                                class: "flex min-h-full flex-col justify-end gap-3",
                                if runtime.messages.is_empty() {
                                    Empty {
                                        class: Some("h-full flex-1 border-0 bg-background/40".to_string()),
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
                        UiCardFooter {
                            extra_class: Some("!px-3".to_string()),
                            div {
                                class: "grid h-full w-full grid-cols-[minmax(0,1fr)_auto] items-stretch gap-2",
                                div {
                                    class: "flex h-full min-w-0 items-center rounded-sm bg-background/80 px-3",
                                    onclick: move |_| {
                                        if !is_input_mode {
                                            composer_container_focus_controller.send_action_keys("i");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    },
                                    textarea {
                                        id: FieldId::ChatInput
                                            .web_dom_id()
                                            .required_dom_id("FieldId::ChatInput must define a web DOM id"),
                                        class: "h-full w-full resize-none overflow-hidden border-0 bg-transparent py-2 text-sm text-foreground outline-none placeholder:text-muted-foreground",
                                        value: "{composer_value}",
                                        readonly: !is_input_mode,
                                        placeholder: if is_input_mode {
                                            "Type a message and press Enter to send"
                                        } else {
                                            "Click here or press 𝒊 to start typing"
                                        },
                                        onfocus: move |_| {
                                            if !is_input_mode {
                                                composer_field_focus_controller.send_action_keys("i");
                                                render_tick.set(render_tick() + 1);
                                            }
                                        },
                                        oninput: move |event| {
                                            composer_input_controller.set_input_buffer(event.value());
                                        },
                                        onkeydown: move |event| {
                                            event.stop_propagation();
                                            if matches!(event.data().key(), Key::Enter)
                                                && !event.data().modifiers().contains(Modifiers::SHIFT)
                                            {
                                                event.prevent_default();
                                                let _ = submit_runtime_chat_input(
                                                    composer_keydown_controller.clone(),
                                                    composer_active_channel.clone(),
                                                    composer_submit_text.clone(),
                                                    schedule_update(),
                                                );
                                                render_tick.set(render_tick() + 1);
                                                return;
                                            }
                                            if matches!(event.data().key(), Key::Escape) {
                                                event.prevent_default();
                                                composer_keydown_controller.send_key_named("esc", 1);
                                                render_tick.set(render_tick() + 1);
                                            }
                                        },
                                    }
                                }
                                div {
                                    class: "flex h-full min-w-[4.5rem] flex-col items-end justify-end gap-1",
                                    UiButton {
                                        id: Some(
                                            ControlId::ChatSendMessageButton
                                                .web_dom_id()
                                                .required_dom_id("ControlId::ChatSendMessageButton must define a web DOM id")
                                                .to_string()
                                        ),
                                        label: "Send".to_string(),
                                        variant: if is_input_mode { ButtonVariant::Primary } else { ButtonVariant::Secondary },
                                        onclick: move |_| {
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
                        "rounded-[1.75rem] bg-primary px-4 py-1.5 text-sm text-primary-foreground shadow-sm"
                    } else {
                        "rounded-[1.75rem] border border-border bg-muted px-4 py-1.5 text-sm text-foreground shadow-sm"
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

#[allow(non_snake_case)]
fn ContactsScreen(
    model: &UiModel,
    runtime: &ContactsRuntimeView,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
) -> Element {
    let contacts = effective_contacts_view(runtime, model);
    let selected_contact_id = model.selected_contact_authority_id();
    let selected_contact = selected_contact_id
        .and_then(|authority_id| {
            contacts
                .iter()
                .find(|contact| contact.authority_id == authority_id)
                .cloned()
        })
        .or_else(|| contacts.first().cloned());
    let selected_name = selected_contact
        .as_ref()
        .map(|contact| contact.name.clone())
        .unwrap_or_else(|| "none".to_string());
    let invite_controller = controller.clone();
    let accept_invitation_controller = controller.clone();
    let start_chat_controller = controller.clone();
    let invite_to_channel_controller = controller.clone();
    let edit_controller = controller.clone();
    let remove_controller = controller.clone();
    rsx! {
        div {
            class: "grid w-full gap-3 lg:grid-cols-12 lg:h-full lg:min-h-0 lg:[grid-template-rows:minmax(0,1fr)]",
            UiCard {
                title: format!("Contacts ({})", contacts.len()),
                subtitle: Some("People you know".to_string()),
                extra_class: Some("lg:col-span-4".to_string()),
                UiCardBody {
                    extra_class: Some("gap-3".to_string()),
                    div {
                        class: "rounded-sm bg-background/60 px-3 py-3",
                        div {
                            class: "flex items-center gap-3",
                            p { class: "m-0 text-xs font-sans font-semibold uppercase tracking-[0.08em] text-muted-foreground", "LAN Peers" }
                            p {
                                class: "m-0 text-xs text-muted-foreground",
                                "updates automatically"
                            }
                        }
                        if runtime.lan_peers.is_empty() {
                            p { class: "m-0 mt-3 text-sm text-muted-foreground", "No LAN peers discovered yet." }
                        } else {
                            div { class: "mt-3 space-y-2",
                                for peer in &runtime.lan_peers {
                                    div {
                                        class: "flex items-center gap-2",
                                        div { class: "min-w-0 flex-1",
                                            UiListItem {
                                                label: peer.authority_id.to_string(),
                                                secondary: Some(if peer.invited {
                                                    format!("{} • invitation pending", peer.address)
                                                } else {
                                                    peer.address.clone()
                                                }),
                                                active: false,
                                            }
                                        }
                                        UiButton {
                                            label: if peer.invited {
                                                "Pending".to_string()
                                            } else {
                                                "Invite".to_string()
                                            },
                                            variant: if peer.invited {
                                                ButtonVariant::Secondary
                                            } else {
                                                ButtonVariant::Primary
                                            },
                                            width_class: Some("w-[6.5rem]".to_string()),
                                            onclick: {
                                                let controller = controller.clone();
                                                let authority_id = peer.authority_id;
                                                let label = peer.authority_id.to_string();
                                                move |_| {
                                                    controller.open_create_invitation_modal(
                                                        Some(&authority_id),
                                                        Some(&label),
                                                    );
                                                    render_tick.set(render_tick() + 1);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div {
                        class: "flex-1 lg:min-h-0",
                        if contacts.is_empty() {
                            Empty {
                                class: Some("h-full border-0 bg-background".to_string()),
                                EmptyHeader {
                                    EmptyTitle { "No contacts yet" }
                                    EmptyDescription { "Use the invitation flow to add contacts." }
                                }
                            }
                        } else {
                            ScrollArea {
                                class: Some("h-full pr-1".to_string()),
                                ScrollAreaViewport {
                                    class: Some("aura-list space-y-2".to_string()),
                                    for contact in contacts.iter() {
                                        button {
                                            r#type: "button",
                                            id: list_item_dom_id(
                                                ListId::Contacts,
                                                &contact.authority_id.to_string(),
                                            ),
                                            class: "block w-full text-left",
                                            onclick: {
                                                let controller = controller.clone();
                                                let authority_id = contact.authority_id;
                                                move |_| {
                                                    controller.set_selected_contact_authority_id(authority_id);
                                                    render_tick.set(render_tick() + 1);
                                                }
                                            },
                                            UiListItem {
                                                label: contact.name.clone(),
                                                secondary: Some(
                                                    if contact.is_guardian {
                                                        "Guardian".to_string()
                                                    } else if matches!(
                                                        contact.confirmation,
                                                        ConfirmationState::PendingLocal
                                                    ) {
                                                        "Pending confirmation".to_string()
                                                    } else if contact.is_member {
                                                        "Member".to_string()
                                                    } else if contact.is_online {
                                                        "Online".to_string()
                                                    } else {
                                                        "\u{00A0}".to_string()
                                                    }
                                                ),
                                                active: selected_contact_id == Some(contact.authority_id),
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    UiCardFooter {
                        extra_class: None,
                        div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                            UiButton {
                                id: Some(
                                    ControlId::ContactsAcceptInvitationButton
                                        .web_dom_id()
                                        .required_dom_id("ControlId::ContactsAcceptInvitationButton must define a web DOM id")
                                        .to_string(),
                                ),
                                label: "Accept Invitation".to_string(),
                                variant: ButtonVariant::Secondary,
                                onclick: move |_| {
                                    accept_invitation_controller.send_action_keys("a");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                            UiButton {
                                id: Some(
                                    ControlId::ContactsCreateInvitationButton
                                        .web_dom_id()
                                        .required_dom_id("ControlId::ContactsCreateInvitationButton must define a web DOM id")
                                        .to_string(),
                                ),
                                label: "Create Invitation".to_string(),
                                variant: ButtonVariant::Primary,
                                onclick: {
                                    let controller = invite_controller;
                                    let selected_contact = selected_contact.clone();
                                    move |_| {
                                        if let Some(contact) = &selected_contact {
                                            let controller = controller.clone();
                                            let app_core = controller.app_core().clone();
                                            let authority_id = contact.authority_id;
                                            spawn(async move {
                                                match invitation_workflows::create_contact_invitation(
                                                    &app_core,
                                                    authority_id,
                                                    None,
                                                    None,
                                                    None,
                                                )
                                                .await
                                                {
                                                    Ok(invitation) => {
                                                        match invitation_workflows::export_invitation(
                                                            &app_core,
                                                            invitation.invitation_id(),
                                                        )
                                                        .await
                                                        {
                                                            Ok(code) => {
                                                                controller.write_clipboard(&code);
                                                                controller.push_runtime_fact(
                                                                    RuntimeFact::InvitationCodeReady {
                                                                        receiver_authority_id: Some(authority_id.to_string()),
                                                                        source_operation: OperationId::invitation_create(),
                                                                        code: Some(code),
                                                                    },
                                                                );
                                                                controller.info_toast(
                                                                    "Invitation code copied to clipboard",
                                                                );
                                                            }
                                                            Err(error) => controller
                                                                .runtime_error_toast(error.to_string()),
                                                        }
                                                    }
                                                    Err(error) => {
                                                        controller.runtime_error_toast(error.to_string());
                                                    }
                                                }
                                            });
                                        } else {
                                            controller.open_create_invitation_modal(None, Some("New contact"));
                                        }
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            UiCard {
                title: "Details".to_string(),
                subtitle: Some(format!("Selected: {selected_name}")),
                extra_class: Some("lg:col-span-8".to_string()),
                if let Some(contact) = selected_contact {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        id: format!("aura-contact-selected-{}", dom_slug(&contact.name)),
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
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                                UiButton {
                                    id: Some(
                                        ControlId::ContactsStartChatButton
                                            .web_dom_id()
                                            .required_dom_id("ControlId::ContactsStartChatButton must define a web DOM id")
                                            .to_string(),
                                    ),
                                    label: "Start Chat".to_string(),
                                    variant: ButtonVariant::Primary,
                                    onclick: {
                                        let authority_id = contact.authority_id;
                                        move |_| {
                                            let controller = start_chat_controller.clone();
                                            let app_core = controller.app_core().clone();
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
                                                    &authority_id.to_string(),
                                                    timestamp_ms,
                                                ).await {
                                                    Ok(channel_id) => {
                                                        controller.set_screen(ScreenId::Chat);
                                                        controller.select_channel_by_name(&channel_id);
                                                    }
                                                    Err(error) => controller.runtime_error_toast(error.to_string()),
                                                }
                                            });
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                                UiButton {
                                    id: Some(
                                        ControlId::ContactsInviteToChannelButton
                                            .web_dom_id()
                                            .required_dom_id(
                                                "ControlId::ContactsInviteToChannelButton must define a web DOM id",
                                            )
                                            .to_string(),
                                    ),
                                    label: "Invite to Channel".to_string(),
                                    variant: ButtonVariant::Secondary,
                                    onclick: {
                                        let authority_id = contact.authority_id;
                                        move |_| {
                                            let controller = invite_to_channel_controller.clone();
                                            let app_core = controller.app_core().clone();
                                            let selected_channel_name = controller
                                                .ui_model()
                                                .and_then(|model| {
                                                    model
                                                        .selected_channel_name()
                                                        .map(str::to_string)
                                                });
                                            spawn(async move {
                                                let Some(channel_name) = selected_channel_name else {
                                                    controller.runtime_error_toast("Select a channel first");
                                                    return;
                                                };
                                                match messaging_workflows::invite_user_to_channel(
                                                    &app_core,
                                                    &authority_id.to_string(),
                                                    &channel_name,
                                                    None,
                                                    None,
                                                )
                                                .await {
                                                    Ok(_) => controller.info_toast("channel invitation sent"),
                                                    Err(error) => controller.runtime_error_toast(error.to_string()),
                                                }
                                            });
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                                UiButton {
                                    id: Some(
                                        ControlId::ContactsEditNicknameButton
                                            .web_dom_id()
                                            .required_dom_id(
                                                "ControlId::ContactsEditNicknameButton must define a web DOM id",
                                            )
                                            .to_string(),
                                    ),
                                    label: "Edit Nickname".to_string(),
                                    variant: ButtonVariant::Secondary,
                                    onclick: move |_| {
                                        edit_controller.send_action_keys("e");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                                UiButton {
                                    id: Some(
                                        ControlId::ContactsRemoveContactButton
                                            .web_dom_id()
                                            .required_dom_id(
                                                "ControlId::ContactsRemoveContactButton must define a web DOM id",
                                            )
                                            .to_string(),
                                    ),
                                    label: "Remove Contact".to_string(),
                                    variant: ButtonVariant::Secondary,
                                    onclick: move |_| {
                                        remove_controller.send_action_keys("r");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                        }
                    }
                } else {
                    Empty {
                        class: Some("h-full border-0 bg-background".to_string()),
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

#[allow(non_snake_case)]
fn NotificationsScreen(
    model: &UiModel,
    runtime: &NotificationsRuntimeView,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
) -> Element {
    let selected = runtime
        .items
        .get(model.selected_notification_index().unwrap_or_default())
        .cloned();
    rsx! {
        div {
            class: "grid w-full gap-3 lg:grid-cols-12 lg:h-full lg:min-h-0 lg:[grid-template-rows:minmax(0,1fr)]",
            UiCard {
                title: "Notifications".to_string(),
                subtitle: Some("Runtime events".to_string()),
                extra_class: Some("lg:col-span-4".to_string()),
                if runtime.items.is_empty() {
                    Empty {
                        class: Some("h-full border-0 bg-background".to_string()),
                        EmptyHeader {
                            EmptyTitle { "No notifications" }
                            EmptyDescription { "Runtime events will appear here." }
                        }
                    }
                } else {
                    ScrollArea {
                        class: Some("flex-1 lg:min-h-0 pr-1".to_string()),
                        ScrollAreaViewport {
                            class: Some("aura-list space-y-2".to_string()),
                            for (idx, entry) in runtime.items.iter().enumerate() {
                                button {
                                    r#type: "button",
                                    id: list_item_dom_id(ListId::Notifications, &entry.id),
                                    class: "block w-full text-left",
                                    onclick: {
                                        let controller = controller.clone();
                                        let item_count = runtime.items.len();
                                        move |_| {
                                            controller.set_selected_notification_index(idx, item_count);
                                            render_tick.set(render_tick() + 1);
                                        }
                                    },
                                    UiListItem {
                                        label: entry.title.clone(),
                                        secondary: Some(entry.kind_label.clone()),
                                        active: model.selected_notification_index() == Some(idx),
                                    }
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
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
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
                        UiCardFooter {
                            extra_class: None,
                            div {
                                class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                                match item.action {
                                    NotificationRuntimeAction::ReceivedInvitation => {
                                        let accept_controller = controller.clone();
                                        let accept_invitation_id = item.id.clone();
                                        let decline_invitation_id = item.id;
                                        rsx! {
                                        UiButton {
                                            label: "Accept".to_string(),
                                            variant: ButtonVariant::Primary,
                                            onclick: {
                                                move |_| {
                                                    let controller = accept_controller.clone();
                                                    let app_core = controller.app_core().clone();
                                                    let mut tick = render_tick;
                                                    let invitation_id = accept_invitation_id.clone();
                                                    spawn(async move {
                                                        match invitation_workflows::accept_invitation_by_str(&app_core, &invitation_id).await {
                                                            Ok(()) => controller.complete_runtime_modal_success("Invitation accepted"),
                                                            Err(error) => controller.runtime_error_toast(error.to_string()),
                                                        }
                                                        tick.set(tick() + 1);
                                                    });
                                                }
                                            }
                                        }
                                        UiButton {
                                            label: "Decline".to_string(),
                                            variant: ButtonVariant::Secondary,
                                            onclick: {
                                                move |_| {
                                                    let controller = controller.clone();
                                                    let app_core = controller.app_core().clone();
                                                    let mut tick = render_tick;
                                                    let invitation_id = decline_invitation_id.clone();
                                                    spawn(async move {
                                                        match invitation_workflows::decline_invitation_by_str(&app_core, &invitation_id).await {
                                                            Ok(()) => controller.complete_runtime_modal_success("Invitation declined"),
                                                            Err(error) => controller.runtime_error_toast(error.to_string()),
                                                        }
                                                        tick.set(tick() + 1);
                                                    });
                                                }
                                            }
                                        }
                                    }
                                    },
                                    NotificationRuntimeAction::SentInvitation => rsx! {
                                        UiButton {
                                            label: "Copy Code".to_string(),
                                            variant: ButtonVariant::Primary,
                                            onclick: {
                                                let invitation_id = item.id;
                                                move |_| {
                                                    let controller = controller.clone();
                                                    let app_core = controller.app_core().clone();
                                                    let mut tick = render_tick;
                                                    let invitation_id = invitation_id.clone();
                                                    spawn(async move {
                                                        match invitation_workflows::export_invitation_by_str(&app_core, &invitation_id).await {
                                                            Ok(code) => {
                                                                controller.write_clipboard(&code);
                                                                controller.complete_runtime_modal_success("Invitation code copied to clipboard");
                                                            }
                                                            Err(error) => controller.runtime_error_toast(error.to_string()),
                                                        }
                                                        tick.set(tick() + 1);
                                                    });
                                                }
                                            }
                                        }
                                    },
                                    NotificationRuntimeAction::RecoveryApproval => rsx! {
                                        UiButton {
                                            label: "Approve Recovery".to_string(),
                                            variant: ButtonVariant::Primary,
                                            onclick: {
                                                let ceremony_id = item.id;
                                                move |_| {
                                                    let controller = controller.clone();
                                                    let app_core = controller.app_core().clone();
                                                    let mut tick = render_tick;
                                                    let ceremony_id = ceremony_id.clone();
                                                    spawn(async move {
                                                        match recovery_workflows::approve_recovery(
                                                            &app_core,
                                                            &CeremonyId::new(ceremony_id),
                                                        ).await {
                                                            Ok(()) => controller.complete_runtime_modal_success("Recovery approved"),
                                                            Err(error) => controller.runtime_error_toast(error.to_string()),
                                                        }
                                                        tick.set(tick() + 1);
                                                    });
                                                }
                                            }
                                        }
                                    },
                                    NotificationRuntimeAction::None => rsx! {},
                                }
                            }
                        }
                    }
                } else {
                    Empty {
                        class: Some("h-full border-0 bg-background".to_string()),
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

fn neighborhood_member_selection_key(
    member: &NeighborhoodRuntimeMember,
) -> NeighborhoodMemberSelectionKey {
    if !member.authority_id.is_empty() {
        NeighborhoodMemberSelectionKey(format!("authority:{}", member.authority_id))
    } else {
        NeighborhoodMemberSelectionKey(format!("name:{}", member.name))
    }
}

#[allow(non_snake_case)]
fn SettingsScreen(
    model: &UiModel,
    runtime: &SettingsRuntimeView,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
    mut theme: dioxus_shadcn::theme::ThemeContext,
    resolved_scheme: ColorScheme,
) -> Element {
    rsx! {
        div {
            class: "grid w-full gap-3 lg:grid-cols-12 lg:h-full lg:min-h-0 lg:[grid-template-rows:minmax(0,1fr)]",
            UiCard {
                title: "Settings".to_string(),
                subtitle: Some("Manage your account".to_string()),
                extra_class: Some("lg:col-span-4".to_string()),
                UiCardBody {
                    extra_class: Some("gap-2".to_string()),
                    for section in SettingsSection::ALL {
                        UiListButton {
                            id: Some(list_item_dom_id(ListId::SettingsSections, section.dom_id())),
                            label: section.title().to_string(),
                            active: section == model.settings_section,
                            extra_class: Some("pt-px pb-0".to_string()),
                            onclick: {
                                let controller = controller.clone();
                                move |_| {
                                    controller.set_settings_section(section);
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                    }
                }
            }

            UiCard {
                title: model.settings_section.title().to_string(),
                subtitle: Some(model.settings_section.subtitle().to_string()),
                extra_class: Some("lg:col-span-8".to_string()),
                if matches!(model.settings_section, SettingsSection::Profile) {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        div {
                            class: "aura-list flex flex-col gap-2",
                            UiListItem {
                                label: format!("Nickname: {}", runtime.nickname),
                                secondary: Some("Suggestion for what contacts should call you".to_string()),
                                active: false,
                            }
                            UiListItem {
                                label: format!("Authority: {}", runtime.authority_id),
                                secondary: Some("local".to_string()),
                                active: false,
                            }
                        }
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                            UiButton {
                                id: Some(
                                    ControlId::SettingsEditNicknameButton
                                        .web_dom_id()
                                        .required_dom_id(
                                            "ControlId::SettingsEditNicknameButton must define a web DOM id"
                                        )
                                        .to_string(),
                                ),
                                label: "Edit Nickname".to_string(),
                                variant: ButtonVariant::Primary,
                                onclick: {
                                    let controller = controller.clone();
                                    move |_| {
                                        controller.set_settings_section(SettingsSection::Profile);
                                        controller.send_action_keys("e");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                            }
                        }
                    }
                }
                if matches!(model.settings_section, SettingsSection::GuardianThreshold) {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        div {
                            class: "aura-list flex flex-col gap-2",
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
                        }
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                            UiButton {
                                id: Some(
                                    ControlId::SettingsConfigureThresholdButton
                                        .web_dom_id()
                                        .required_dom_id(
                                            "ControlId::SettingsConfigureThresholdButton must define a web DOM id"
                                        )
                                        .to_string(),
                                ),
                                label: "Configure Threshold".to_string(),
                                variant: ButtonVariant::Primary,
                                onclick: {
                                    let controller = controller.clone();
                                    move |_| {
                                        controller.set_settings_section(SettingsSection::GuardianThreshold);
                                        controller.send_action_keys("t");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                            }
                        }
                    }
                }
                if matches!(model.settings_section, SettingsSection::RequestRecovery) {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        div {
                            class: "aura-list flex flex-col gap-2",
                            UiListItem {
                                label: format!("Last status: {}", runtime.active_recovery_label),
                                secondary: Some(format!("Pending approvals to review: {}", runtime.pending_recovery_requests)),
                                active: false,
                            }
                        }
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                            UiButton {
                                id: Some(
                                    ControlId::SettingsRequestRecoveryButton
                                        .web_dom_id()
                                        .required_dom_id(
                                            "ControlId::SettingsRequestRecoveryButton must define a web DOM id"
                                        )
                                        .to_string(),
                                ),
                                label: "Request Recovery".to_string(),
                                variant: ButtonVariant::Primary,
                                onclick: {
                                    let controller = controller.clone();
                                    move |_| {
                                        controller.set_settings_section(SettingsSection::RequestRecovery);
                                        controller.send_action_keys("s");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                            }
                        }
                    }
                }
                if matches!(model.settings_section, SettingsSection::Devices) {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        div {
                            class: "aura-list flex flex-col gap-2",
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
                        }
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                            UiButton {
                                id: Some(
                                    ControlId::SettingsAddDeviceButton
                                        .web_dom_id()
                                        .required_dom_id("ControlId::SettingsAddDeviceButton must define a web DOM id")
                                        .to_string(),
                                ),
                                label: "Add Device".to_string(),
                                variant: ButtonVariant::Primary,
                                onclick: {
                                    let controller = controller.clone();
                                    move |_| {
                                        controller.set_settings_section(SettingsSection::Devices);
                                        controller.send_action_keys("a");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                            UiButton {
                                id: Some(
                                    ControlId::SettingsImportDeviceCodeButton
                                        .web_dom_id()
                                        .required_dom_id("ControlId::SettingsImportDeviceCodeButton must define a web DOM id")
                                        .to_string(),
                                ),
                                label: "Import Code".to_string(),
                                variant: ButtonVariant::Secondary,
                                onclick: {
                                    let controller = controller.clone();
                                    move |_| {
                                        controller.set_settings_section(SettingsSection::Devices);
                                        controller.send_action_keys("i");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                            UiButton {
                                id: Some(
                                    ControlId::SettingsRemoveDeviceButton
                                        .web_dom_id()
                                        .required_dom_id("ControlId::SettingsRemoveDeviceButton must define a web DOM id")
                                        .to_string(),
                                ),
                                label: "Remove Device".to_string(),
                                variant: ButtonVariant::Secondary,
                                onclick: {
                                    let controller = controller.clone();
                                    move |_| {
                                        controller.set_settings_section(SettingsSection::Devices);
                                        controller.send_action_keys("r");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                            }
                        }
                    }
                }
                if matches!(model.settings_section, SettingsSection::Authority) {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        div {
                            class: "aura-list flex flex-col gap-2",
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
                                    onclick: {
                                        let controller = controller.clone();
                                        let authority_id = authority.id;
                                        move |_| {
                                            if authority.is_current {
                                                return;
                                            }
                                            let _ = controller.request_authority_switch(authority_id);
                                        }
                                    }
                                }
                            }
                            UiListItem {
                                label: "Multifactor".to_string(),
                                secondary: Some(format!("Policy: {}", runtime.mfa_policy)),
                                active: false,
                            }
                        }
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                            UiButton {
                                id: Some(
                                    ControlId::SettingsSwitchAuthorityButton
                                        .web_dom_id()
                                        .required_dom_id(
                                            "ControlId::SettingsSwitchAuthorityButton must define a web DOM id"
                                        )
                                        .to_string(),
                                ),
                                label: "Switch Authority".to_string(),
                                variant: ButtonVariant::Primary,
                                onclick: {
                                    let controller = controller.clone();
                                    move |_| {
                                        controller.set_settings_section(SettingsSection::Authority);
                                        controller.send_action_keys("s");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                            UiButton {
                                id: Some(
                                    ControlId::SettingsConfigureMfaButton
                                        .web_dom_id()
                                        .required_dom_id(
                                            "ControlId::SettingsConfigureMfaButton must define a web DOM id"
                                        )
                                        .to_string(),
                                ),
                                label: "Configure MFA".to_string(),
                                variant: ButtonVariant::Secondary,
                                onclick: {
                                    let controller = controller;
                                    move |_| {
                                        controller.set_settings_section(SettingsSection::Authority);
                                        controller.send_action_keys("m");
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                            }
                        }
                    }
                }
                if matches!(model.settings_section, SettingsSection::Appearance) {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        div {
                            class: "aura-list flex flex-col gap-2",
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
                        }
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                            UiButton {
                                id: Some(
                                    ControlId::SettingsToggleThemeButton
                                        .web_dom_id()
                                        .required_dom_id(
                                            "ControlId::SettingsToggleThemeButton must define a web DOM id"
                                        )
                                        .to_string(),
                                ),
                                label: match resolved_scheme {
                                    ColorScheme::Light => "Switch to Dark".to_string(),
                                    _ => "Switch to Light".to_string(),
                                },
                                variant: ButtonVariant::Primary,
                                width_class: Some("w-[9.5rem]".to_string()),
                                onclick: move |_| {
                                    theme.toggle_color_scheme();
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                            }
                        }
                    }
                }
                if matches!(model.settings_section, SettingsSection::Info) {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        div {
                            class: "aura-list flex flex-col gap-2",
                            UiListItem {
                                label: "Storage: IndexedDB".to_string(),
                                secondary: Some(
                                    "Browser-backed local persistence for this device.".to_string()
                                ),
                                active: false,
                            }
                        }
                    }
                }
            }
        }
    }
}

fn screen_tabs(active: ScreenId) -> Vec<(ScreenId, &'static str, bool)> {
    [
        (
            ScreenId::Neighborhood,
            "Neighborhood",
            active == ScreenId::Neighborhood,
        ),
        (ScreenId::Chat, "Chat", active == ScreenId::Chat),
        (ScreenId::Contacts, "Contacts", active == ScreenId::Contacts),
        (
            ScreenId::Notifications,
            "Notifications",
            active == ScreenId::Notifications,
        ),
        (ScreenId::Settings, "Settings", active == ScreenId::Settings),
    ]
    .to_vec()
}

fn nav_button_id(screen: ScreenId) -> &'static str {
    match screen {
        ScreenId::Onboarding => ControlId::OnboardingRoot
            .web_dom_id()
            .required_dom_id("OnboardingRoot must define a web DOM id"),
        ScreenId::Neighborhood => ControlId::NavNeighborhood
            .web_dom_id()
            .required_dom_id("NavNeighborhood must define a web DOM id"),
        ScreenId::Chat => ControlId::NavChat
            .web_dom_id()
            .required_dom_id("NavChat must define a web DOM id"),
        ScreenId::Contacts => ControlId::NavContacts
            .web_dom_id()
            .required_dom_id("NavContacts must define a web DOM id"),
        ScreenId::Notifications => ControlId::NavNotifications
            .web_dom_id()
            .required_dom_id("NavNotifications must define a web DOM id"),
        ScreenId::Settings => ControlId::NavSettings
            .web_dom_id()
            .required_dom_id("NavSettings must define a web DOM id"),
    }
}

fn dom_slug(value: &str) -> String {
    let mut slug = String::with_capacity(value.len());
    let mut previous_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            slug.push('-');
            previous_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn nav_tab_class(is_active: bool) -> &'static str {
    if is_active {
        "inline-flex h-8 items-center justify-center whitespace-nowrap rounded-sm bg-accent px-3 text-xs font-sans uppercase leading-none tracking-[0.08em] text-foreground"
    } else {
        "inline-flex h-8 items-center justify-center whitespace-nowrap rounded-sm px-3 text-xs font-sans uppercase leading-none tracking-[0.08em] text-muted-foreground hover:bg-accent hover:text-foreground"
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
        ScreenId::Onboarding => rsx! {
            div {
                id: ControlId::Screen(aura_app::ui::contract::ScreenId::Onboarding)
                    .web_dom_id()
                    .required_dom_id("Screen(Onboarding) must define a web DOM id"),
                class: "w-full lg:h-full lg:min-h-0",
                {OnboardingScreen()}
            }
        },
        ScreenId::Neighborhood => rsx! {
            div {
                id: ControlId::Screen(aura_app::ui::contract::ScreenId::Neighborhood)
                    .web_dom_id()
                    .required_dom_id("Screen(Neighborhood) must define a web DOM id"),
                class: "w-full lg:h-full lg:min-h-0",
                {NeighborhoodScreen(model, neighborhood_runtime, controller, render_tick)}
            }
        },
        ScreenId::Chat => rsx! {
            div {
                id: ControlId::Screen(aura_app::ui::contract::ScreenId::Chat)
                    .web_dom_id()
                    .required_dom_id("Screen(Chat) must define a web DOM id"),
                class: "w-full lg:h-full lg:min-h-0",
                {ChatScreen(model, chat_runtime, controller, render_tick)}
            }
        },
        ScreenId::Contacts => rsx! {
            div {
                id: ControlId::Screen(aura_app::ui::contract::ScreenId::Contacts)
                    .web_dom_id()
                    .required_dom_id("Screen(Contacts) must define a web DOM id"),
                class: "w-full lg:h-full lg:min-h-0",
                {ContactsScreen(model, contacts_runtime, controller, render_tick)}
            }
        },
        ScreenId::Notifications => rsx! {
            div {
                id: ControlId::Screen(aura_app::ui::contract::ScreenId::Notifications)
                    .web_dom_id()
                    .required_dom_id("Screen(Notifications) must define a web DOM id"),
                class: "w-full lg:h-full lg:min-h-0",
                {NotificationsScreen(model, notifications_runtime, controller, render_tick)}
            }
        },
        ScreenId::Settings => rsx! {
            div {
                id: ControlId::Screen(aura_app::ui::contract::ScreenId::Settings)
                    .web_dom_id()
                    .required_dom_id("Screen(Settings) must define a web DOM id"),
                class: "w-full lg:h-full lg:min-h-0",
                {SettingsScreen(
                    model,
                    settings_runtime,
                    controller,
                    render_tick,
                    theme,
                    resolved_scheme,
                )}
            }
        },
    }
}

#[component]
fn OnboardingScreen() -> Element {
    rsx! {
        div {
            class: "w-full lg:h-full lg:min-h-0"
        }
    }
}

fn upsert_snapshot_list(
    snapshot: &mut UiSnapshot,
    list_id: ListId,
    items: Vec<ListItemSnapshot>,
    selected_item_id: Option<String>,
) {
    snapshot.lists.retain(|list| list.id != list_id);
    snapshot
        .selections
        .retain(|selection| selection.list != list_id);
    if items.is_empty() {
        return;
    }
    snapshot.lists.push(ListSnapshot { id: list_id, items });
    if let Some(item_id) = selected_item_id {
        snapshot.selections.push(SelectionSnapshot {
            list: list_id,
            item_id,
        });
    }
}

fn upsert_snapshot_operation(
    snapshot: &mut UiSnapshot,
    operation_id: OperationId,
    state: OperationState,
) {
    snapshot
        .operations
        .retain(|operation| operation.id != operation_id);
    snapshot.operations.push(OperationSnapshot {
        id: operation_id,
        instance_id: OperationInstanceId("synthetic-operation".to_string()),
        state,
    });
}

fn screen_readiness(
    screen: ScreenId,
    _neighborhood_runtime: &NeighborhoodRuntimeView,
    _chat_runtime: &ChatRuntimeView,
    _contacts_runtime: &ContactsRuntimeView,
    _settings_runtime: &SettingsRuntimeView,
    _notifications_runtime: &NotificationsRuntimeView,
) -> UiReadiness {
    match screen {
        ScreenId::Onboarding => UiReadiness::Loading,
        ScreenId::Neighborhood
        | ScreenId::Chat
        | ScreenId::Contacts
        | ScreenId::Notifications
        | ScreenId::Settings => UiReadiness::Ready,
    }
}

fn runtime_semantic_snapshot(
    model: &UiModel,
    neighborhood_runtime: &NeighborhoodRuntimeView,
    chat_runtime: &ChatRuntimeView,
    contacts_runtime: &ContactsRuntimeView,
    settings_runtime: &SettingsRuntimeView,
    notifications_runtime: &NotificationsRuntimeView,
) -> UiSnapshot {
    let mut snapshot = model.semantic_snapshot();
    snapshot.readiness = screen_readiness(
        model.screen,
        neighborhood_runtime,
        chat_runtime,
        contacts_runtime,
        settings_runtime,
        notifications_runtime,
    );

    if let Some(add_device_state) = model.add_device_modal() {
        let operation_state = match add_device_state.step {
            AddDeviceWizardStep::Name => OperationState::Idle,
            AddDeviceWizardStep::ShareCode | AddDeviceWizardStep::Confirm => {
                if add_device_state.has_failed {
                    OperationState::Failed
                } else if add_device_state.is_complete {
                    OperationState::Succeeded
                } else {
                    OperationState::Submitting
                }
            }
        };
        upsert_snapshot_operation(
            &mut snapshot,
            OperationId::device_enrollment(),
            operation_state,
        );
    }

    let homes = neighborhood_runtime
        .homes
        .iter()
        .map(|home| ListItemSnapshot {
            id: home.id.clone(),
            selected: model.selected_home_id() == Some(home.id.as_str()),
            confirmation: ConfirmationState::Confirmed,
            is_current: false,
        })
        .collect::<Vec<_>>();
    let selected_home_id = model.selected_home_id().map(str::to_string).or_else(|| {
        neighborhood_runtime
            .homes
            .iter()
            .find(|home| home.name == neighborhood_runtime.active_home_name)
            .map(|home| home.id.clone())
    });
    upsert_snapshot_list(&mut snapshot, ListId::Homes, homes, selected_home_id);

    let members = neighborhood_runtime
        .members
        .iter()
        .map(|member| {
            let member_key = neighborhood_member_selection_key(member);
            ListItemSnapshot {
                id: member_key.0.clone(),
                selected: model.selected_neighborhood_member_key.as_ref() == Some(&member_key),
                confirmation: ConfirmationState::Confirmed,
                is_current: false,
            }
        })
        .collect::<Vec<_>>();
    let selected_member_id = model
        .selected_neighborhood_member_key
        .as_ref()
        .map(|key| key.0.clone());
    upsert_snapshot_list(
        &mut snapshot,
        ListId::NeighborhoodMembers,
        members,
        selected_member_id,
    );

    let channels = if chat_runtime.loaded {
        chat_runtime
            .channels
            .iter()
            .map(|channel| ListItemSnapshot {
                id: channel.id.clone(),
                selected: channel
                    .name
                    .eq_ignore_ascii_case(&chat_runtime.active_channel),
                confirmation: ConfirmationState::Confirmed,
                is_current: false,
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let selected_channel_id = if chat_runtime.loaded {
        chat_runtime
            .channels
            .iter()
            .find(|channel| {
                channel
                    .name
                    .eq_ignore_ascii_case(&chat_runtime.active_channel)
            })
            .map(|channel| channel.id.clone())
    } else {
        None
    };
    if !channels.is_empty() {
        upsert_snapshot_list(
            &mut snapshot,
            ListId::Channels,
            channels,
            selected_channel_id,
        );
    }

    let contacts = if contacts_runtime.loaded {
        effective_contacts_view(contacts_runtime, model)
            .iter()
            .map(|contact| ListItemSnapshot {
                id: contact.authority_id.to_string(),
                selected: model.selected_contact_authority_id() == Some(contact.authority_id),
                confirmation: contact.confirmation,
                is_current: false,
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if !contacts.is_empty() {
        upsert_snapshot_list(
            &mut snapshot,
            ListId::Contacts,
            contacts,
            model
                .selected_contact_authority_id()
                .map(|id| id.to_string()),
        );
    }

    let devices = settings_runtime
        .devices
        .iter()
        .map(|device| ListItemSnapshot {
            id: device.id.clone(),
            selected: device.is_current,
            confirmation: ConfirmationState::Confirmed,
            is_current: device.is_current,
        })
        .collect::<Vec<_>>();
    upsert_snapshot_list(&mut snapshot, ListId::Devices, devices, None);

    let authorities = settings_runtime
        .authorities
        .iter()
        .map(|authority| ListItemSnapshot {
            id: authority.id.to_string(),
            selected: model.selected_authority_id == Some(authority.id),
            confirmation: ConfirmationState::Confirmed,
            is_current: false,
        })
        .collect::<Vec<_>>();
    if !authorities.is_empty() {
        upsert_snapshot_list(
            &mut snapshot,
            ListId::Authorities,
            authorities,
            model.selected_authority_id.map(|id| id.to_string()),
        );
    }

    let notifications = notifications_runtime
        .items
        .iter()
        .map(|item| ListItemSnapshot {
            id: item.id.clone(),
            selected: model.selected_notification_id.as_ref().map(|id| &id.0) == Some(&item.id),
            confirmation: ConfirmationState::Confirmed,
            is_current: false,
        })
        .collect::<Vec<_>>();
    if !notifications.is_empty() {
        upsert_snapshot_list(
            &mut snapshot,
            ListId::Notifications,
            notifications,
            model
                .selected_notification_id
                .as_ref()
                .map(|id| id.0.clone()),
        );
    }

    snapshot.messages = chat_runtime
        .messages
        .iter()
        .enumerate()
        .map(|(idx, message)| MessageSnapshot {
            id: format!("chat-message-{idx}"),
            content: message.content.clone(),
        })
        .collect();
    snapshot.quiescence = aura_app::ui_contract::QuiescenceSnapshot::derive(
        snapshot.readiness,
        snapshot.open_modal,
        &snapshot.operations,
    );

    snapshot
}

fn active_modal_title(model: &UiModel) -> Option<String> {
    let modal = model.modal_state()?;
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

fn modal_view(model: &UiModel, chat_runtime: &ChatRuntimeView) -> Option<ModalView> {
    let modal = model.modal_state()?;
    let title = active_modal_title(model).unwrap_or_else(|| "Modal".to_string());
    let mut details = Vec::new();
    let mut keybind_rows = Vec::new();
    let mut inputs = Vec::new();

    match modal {
        ModalState::Help => {
            let (help_details, help_keybind_rows) = help_modal_content(model.screen);
            details = help_details;
            keybind_rows = help_keybind_rows;
        }
        ModalState::CreateInvitation => {
            if model
                .create_invitation_modal()
                .and_then(|state| state.receiver_label.as_ref())
                .is_some()
            {
                details.push(
                    "Review or adjust the authority id, then press Enter to generate and copy the code."
                        .to_string(),
                );
            } else {
                details.push("Create an invite code for a contact.".to_string());
                details.push("Enter the target authority id, then press Enter to generate and copy the code.".to_string());
            }
            inputs.push(ModalInputView {
                label: "Receiver Authority ID".to_string(),
                field_id: FieldId::InvitationReceiver,
                value: model.modal_text_value().unwrap_or_default(),
            });
        }
        ModalState::AcceptInvitation => {
            details.push("Paste an invite code, then press Enter.".to_string());
            inputs.push(ModalInputView {
                label: "Invite Code".to_string(),
                field_id: FieldId::InvitationCode,
                value: model.modal_text_value().unwrap_or_default(),
            });
        }
        ModalState::CreateHome => {
            details.push("Enter a new home name and press Enter.".to_string());
            inputs.push(ModalInputView {
                label: "Home Name".to_string(),
                field_id: FieldId::HomeName,
                value: model.modal_text_value().unwrap_or_default(),
            });
        }
        ModalState::CreateChannel => {
            if let Some(state) = model.create_channel_modal() {
                match state.step {
                    CreateChannelWizardStep::Details => {
                        let active = match state.active_field {
                            CreateChannelDetailsField::Name => "Group Name",
                            CreateChannelDetailsField::Topic => "Topic (optional)",
                        };
                        details.push("Step 1 of 3: Configure group details.".to_string());
                        details.push(format!("Active field: {active} (Tab to switch)"));
                        inputs.push(ModalInputView {
                            label: "Group Name".to_string(),
                            field_id: FieldId::CreateChannelName,
                            value: state.name.clone(),
                        });
                        inputs.push(ModalInputView {
                            label: "Topic (optional)".to_string(),
                            field_id: FieldId::CreateChannelTopic,
                            value: state.topic.clone(),
                        });
                    }
                    CreateChannelWizardStep::Members => {
                        details.push("Step 2 of 3: Select members to invite.".to_string());
                        if model.contacts.is_empty() {
                            details.push("No contacts available.".to_string());
                        } else {
                            for (idx, contact) in model.contacts.iter().enumerate() {
                                let focused = if idx == state.member_focus { ">" } else { " " };
                                let selected = if state.selected_members.contains(&idx) {
                                    "[x]"
                                } else {
                                    "[ ]"
                                };
                                details.push(format!("{focused} {selected} {}", contact.name));
                            }
                        }
                        details.push(
                            "Use ↑/↓ to move, Space to toggle, Enter to continue.".to_string(),
                        );
                    }
                    CreateChannelWizardStep::Threshold => {
                        let participant_total = state.selected_members.len().saturating_add(1);
                        details.push("Step 3 of 3: Set threshold.".to_string());
                        details.push(format!("Participants (including you): {participant_total}"));
                        details.push("Use ↑/↓ to adjust, Enter to create.".to_string());
                        inputs.push(ModalInputView {
                            label: "Threshold".to_string(),
                            field_id: FieldId::ThresholdInput,
                            value: model.modal_text_value().unwrap_or_default(),
                        });
                    }
                }
            }
        }
        ModalState::SetChannelTopic => {
            details.push("Set a topic for the selected channel.".to_string());
            inputs.push(ModalInputView {
                label: "Channel Topic".to_string(),
                field_id: FieldId::CreateChannelTopic,
                value: model.modal_text_value().unwrap_or_default(),
            });
        }
        ModalState::ChannelInfo => {
            let active_channel = if chat_runtime.active_channel.is_empty() {
                model
                    .selected_channel_name()
                    .unwrap_or(NOTE_TO_SELF_CHANNEL_NAME)
                    .to_string()
            } else {
                chat_runtime.active_channel.clone()
            };
            if let Some(channel) = chat_runtime
                .channels
                .iter()
                .find(|channel| channel.name.eq_ignore_ascii_case(&active_channel))
            {
                details.push(format!("Channel: #{}", channel.name));
                details.push(format!(
                    "Type: {}",
                    if channel.is_dm {
                        "Direct Message"
                    } else {
                        "Group Channel"
                    }
                ));
                details.push(format!(
                    "Topic: {}",
                    if channel.topic.trim().is_empty() {
                        "No topic set".to_string()
                    } else {
                        channel.topic.clone()
                    }
                ));
                details.push(format!("Unread messages: {}", channel.unread_count));
                details.push(format!("Visible messages: {}", chat_runtime.messages.len()));
                if channel.member_count > 0 {
                    details.push(format!("Known members: {}", channel.member_count));
                }
                if let Some(last_message) = &channel.last_message {
                    details.push(format!("Latest message: {last_message}"));
                }
            } else {
                details.push("No channel selected.".to_string());
            }
        }
        ModalState::EditNickname => {
            details.push("Update your nickname suggestion and press Enter.".to_string());
            inputs.push(ModalInputView {
                label: "Nickname".to_string(),
                field_id: FieldId::Nickname,
                value: model.modal_text_value().unwrap_or_default(),
            });
        }
        ModalState::RemoveContact => {
            details.push("Remove the selected contact from this authority.".to_string());
            details.push("Press Enter to confirm.".to_string());
        }
        ModalState::GuardianSetup => {
            if let Some(state) = model.guardian_setup_modal() {
                match state.step {
                    ThresholdWizardStep::Selection => {
                        details.push("Step 1 of 3: Select guardians.".to_string());
                        if model.contacts.is_empty() {
                            details.push("No contacts available.".to_string());
                        } else {
                            for (idx, contact) in model.contacts.iter().enumerate() {
                                let focused = if idx == state.focus_index { ">" } else { " " };
                                let selected = if state.selected_indices.contains(&idx) {
                                    "[x]"
                                } else {
                                    "[ ]"
                                };
                                details.push(format!("{focused} {selected} {}", contact.name));
                            }
                        }
                        details.push(
                            "Use ↑/↓ to move, Space to toggle, Enter to continue.".to_string(),
                        );
                    }
                    ThresholdWizardStep::Threshold => {
                        details.push("Step 2 of 3: Choose threshold.".to_string());
                        details.push(format!("Selected guardians: {}", state.selected_count));
                        details.push("Enter k (approvals required).".to_string());
                        inputs.push(ModalInputView {
                            label: "Threshold (k)".to_string(),
                            field_id: FieldId::ThresholdInput,
                            value: model.modal_text_value().unwrap_or_default(),
                        });
                    }
                    ThresholdWizardStep::Ceremony => {
                        details.push("Step 3 of 3: Ready to start ceremony.".to_string());
                        details.push(format!(
                            "Will start guardian setup with {} of {} approvals.",
                            state.threshold_k, state.selected_count
                        ));
                        details.push("Press Enter to start.".to_string());
                    }
                }
            }
        }
        ModalState::RequestRecovery => {
            details.push("Request guardian-assisted recovery for this authority.".to_string());
            details.push("Press Enter to notify your configured guardians.".to_string());
        }
        ModalState::AddDeviceStep1 => {
            if let Some(state) = model.add_device_modal() {
                match state.step {
                    AddDeviceWizardStep::Name => {
                        details
                            .push("Step 1 of 3: Name the device you want to invite.".to_string());
                        details.push("This is the new device, not the current one.".to_string());
                        details.push(
                            "Press Enter to generate an out-of-band enrollment code.".to_string(),
                        );
                        if !state.name_input.trim().is_empty() {
                            details.push(format!("Draft name: {}", state.name_input));
                        }
                        inputs.push(ModalInputView {
                            label: "Device Name".to_string(),
                            field_id: FieldId::DeviceName,
                            value: model.modal_text_value().unwrap_or_default(),
                        });
                    }
                    AddDeviceWizardStep::ShareCode => {
                        details.push(
                            "Step 2 of 3: Share this code out-of-band with that device."
                                .to_string(),
                        );
                        details.push(format!("Enrollment Code: {}", state.enrollment_code));
                        details.push("Press c to copy, then press Enter when shared.".to_string());
                        if let Some(ceremony_id) = state.ceremony_id.as_ref() {
                            details.push(format!("Ceremony: {ceremony_id}"));
                        }
                    }
                    AddDeviceWizardStep::Confirm => {
                        details.push(
                            "Step 3 of 3: Waiting for the new device to import the code."
                                .to_string(),
                        );
                        details.push(format!(
                            "Device '{}': {} of {} confirmations ({})",
                            state.device_name,
                            state.accepted_count,
                            state.total_count.max(1),
                            state.threshold.max(1)
                        ));
                        if let Some(error) = &state.error_message {
                            details.push(format!("Error: {error}"));
                        } else if state.has_failed {
                            details.push("The enrollment ceremony failed.".to_string());
                        } else if state.is_complete {
                            details.push("Enrollment ceremony complete. The new device is now part of this authority.".to_string());
                        } else {
                            details.push("Leave this dialog open to monitor progress, or press Esc to cancel the ceremony.".to_string());
                        }
                    }
                }
            }
        }
        ModalState::ImportDeviceEnrollmentCode => {
            details.push("Import a device enrollment code and press Enter.".to_string());
            inputs.push(ModalInputView {
                label: "Enrollment Code".to_string(),
                field_id: FieldId::DeviceImportCode,
                value: model.modal_text_value().unwrap_or_default(),
            });
        }
        ModalState::SelectDeviceToRemove => {
            details.push("Select the device to remove.".to_string());
            details.push(format!(
                "Selected: {}",
                model
                    .secondary_device_name()
                    .or_else(|| model
                        .selected_device_modal()
                        .map(|state| state.candidate_name.as_str()))
                    .unwrap_or("Secondary device")
            ));
            details.push("Press Enter to continue.".to_string());
        }
        ModalState::ConfirmRemoveDevice => {
            details.push(format!(
                "Remove \"{}\" from this authority?",
                model
                    .secondary_device_name()
                    .or_else(|| model
                        .selected_device_modal()
                        .map(|state| state.candidate_name.as_str()))
                    .unwrap_or("Secondary device")
            ));
            details.push("Press Enter to confirm removal.".to_string());
        }
        ModalState::MfaSetup => {
            if let Some(state) = model.mfa_setup_modal() {
                match state.step {
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
                            let focused = if idx == state.focus_index { ">" } else { " " };
                            let selected = if state.selected_indices.contains(&idx) {
                                "[x]"
                            } else {
                                "[ ]"
                            };
                            details.push(format!("{focused} {selected} {device}"));
                        }
                        details.push(
                            "Use ↑/↓ to move, Space to toggle, Enter to continue.".to_string(),
                        );
                    }
                    ThresholdWizardStep::Threshold => {
                        details.push("Step 2 of 3: Configure signing threshold.".to_string());
                        details.push(format!("Selected devices: {}", state.selected_count));
                        details.push("Enter required signatures (k).".to_string());
                        inputs.push(ModalInputView {
                            label: "Threshold (k)".to_string(),
                            field_id: FieldId::ThresholdInput,
                            value: model.modal_text_value().unwrap_or_default(),
                        });
                    }
                    ThresholdWizardStep::Ceremony => {
                        details.push("Step 3 of 3: Ready to start MFA ceremony.".to_string());
                        details.push(format!(
                            "Will start MFA with {} of {} signatures.",
                            state.threshold_k, state.selected_count
                        ));
                        details.push("Press Enter to start.".to_string());
                    }
                }
            }
        }
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
                    let focused = if model.selected_authority_index() == Some(idx) {
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
            let level = match model.active_modal.as_ref() {
                Some(ActiveModal::AccessOverride(state)) => state.level.label(),
                _ => AccessOverrideLevel::Limited.label(),
            };
            details.push("Apply a per-home access override for the selected contact.".to_string());
            details.push(format!("Selected contact: {selected_contact}"));
            details.push(format!("Access level: {level}"));
            details.push("Use ↑/↓ to select a contact. Tab toggles Limited/Partial.".to_string());
            details.push("Press Enter to apply the override to the current home.".to_string());
        }
        ModalState::CapabilityConfig => {
            let (active, full_caps, partial_caps, limited_caps) = model
                .capability_config_modal()
                .map(|state| {
                    (
                        state.active_tier.label(),
                        state.full_caps.as_str(),
                        state.partial_caps.as_str(),
                        state.limited_caps.as_str(),
                    )
                })
                .unwrap_or((
                    CapabilityTier::Full.label(),
                    DEFAULT_CAPABILITY_FULL,
                    DEFAULT_CAPABILITY_PARTIAL,
                    DEFAULT_CAPABILITY_LIMITED,
                ));
            details.push("Configure per-home capabilities for each access level.".to_string());
            details.push("Tab switches fields. Enter saves to the current home.".to_string());
            details.push(format!("Editing: {active}"));
            details.push(format!("Full: {full_caps}"));
            details.push(format!("Partial: {partial_caps}"));
            details.push(format!("Limited: {limited_caps}"));
            let field_id = match model
                .capability_config_modal()
                .map(|state| state.active_tier)
            {
                Some(CapabilityTier::Partial) => FieldId::CapabilityPartial,
                Some(CapabilityTier::Limited) => FieldId::CapabilityLimited,
                _ => FieldId::CapabilityFull,
            };
            inputs.push(ModalInputView {
                label: format!("{active} Capabilities"),
                field_id,
                value: model.modal_text_value().unwrap_or_default(),
            });
        }
    }

    let enter_label = match modal {
        ModalState::Help | ModalState::ChannelInfo => "Close".to_string(),
        ModalState::CreateChannel => match model.create_channel_modal().map(|state| state.step) {
            Some(CreateChannelWizardStep::Threshold) => "Create".to_string(),
            _ => "Next".to_string(),
        },
        ModalState::AddDeviceStep1 => match model.add_device_modal().map(|state| state.step) {
            Some(AddDeviceWizardStep::ShareCode) => "Next".to_string(),
            Some(AddDeviceWizardStep::Confirm) => {
                if model
                    .add_device_modal()
                    .map(|state| state.is_complete || state.has_failed)
                    .unwrap_or(false)
                {
                    "Close".to_string()
                } else {
                    "Refresh".to_string()
                }
            }
            _ => "Generate Code".to_string(),
        },
        ModalState::GuardianSetup => match model.guardian_setup_modal().map(|state| state.step) {
            Some(ThresholdWizardStep::Ceremony) => "Start".to_string(),
            _ => "Next".to_string(),
        },
        ModalState::MfaSetup => match model.mfa_setup_modal().map(|state| state.step) {
            Some(ThresholdWizardStep::Ceremony) => "Start".to_string(),
            _ => "Next".to_string(),
        },
        ModalState::SwitchAuthority => "Switch".to_string(),
        ModalState::AccessOverride => "Apply".to_string(),
        ModalState::CapabilityConfig => "Save".to_string(),
        _ => "Confirm".to_string(),
    };

    Some(ModalView {
        modal_id: modal.contract_id(),
        title,
        details,
        keybind_rows,
        inputs,
        enter_label,
    })
}

fn help_modal_content(screen: ScreenId) -> (Vec<String>, Vec<(String, String)>) {
    let details = match screen {
        ScreenId::Onboarding => vec![
            "Onboarding reference".to_string(),
            "Create or import a local account before entering the main application.".to_string(),
        ],
        ScreenId::Neighborhood => vec![
            "Neighborhood reference".to_string(),
            "Browse homes, access depth, and neighborhood detail views.".to_string(),
        ],
        ScreenId::Chat => vec![
            "Chat reference".to_string(),
            "Navigate channels, compose messages, and manage channel metadata.".to_string(),
        ],
        ScreenId::Contacts => vec![
            "Contacts reference".to_string(),
            "Manage invitations, nicknames, guardians, and direct-message handoff.".to_string(),
        ],
        ScreenId::Notifications => vec![
            "Notifications reference".to_string(),
            "Review pending notices and move through the notification feed.".to_string(),
        ],
        ScreenId::Settings => vec![
            "Settings reference".to_string(),
            "Adjust profile, recovery, devices, authority, and appearance.".to_string(),
        ],
    };

    let keybind_rows = match screen {
        ScreenId::Onboarding => vec![
            (
                "type".to_string(),
                "Enter account name or import code".to_string(),
            ),
            (
                "enter".to_string(),
                "Submit the active onboarding form".to_string(),
            ),
        ],
        ScreenId::Neighborhood => vec![
            ("1-5".to_string(), "Switch screens".to_string()),
            ("tab / shift+tab".to_string(), "Cycle screens".to_string()),
            ("enter".to_string(), "Toggle map/detail view".to_string()),
            ("a".to_string(), "Accept home invitation".to_string()),
            ("n".to_string(), "Create home".to_string()),
            ("d".to_string(), "Cycle access depth".to_string()),
            ("esc".to_string(), "Close modal / back out".to_string()),
        ],
        ScreenId::Chat => vec![
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
        ScreenId::Contacts => vec![
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
            (
                "i".to_string(),
                "Invite selected contact to current channel".to_string(),
            ),
            ("e".to_string(), "Edit nickname".to_string()),
            ("g".to_string(), "Configure guardians".to_string()),
            ("c".to_string(), "Open DM for selected contact".to_string()),
            ("r".to_string(), "Remove contact".to_string()),
        ],
        ScreenId::Notifications => vec![
            ("1-5".to_string(), "Switch screens".to_string()),
            ("tab / shift+tab".to_string(), "Cycle screens".to_string()),
            (
                "up / down".to_string(),
                "Move notification selection".to_string(),
            ),
            (
                "click actions".to_string(),
                "Accept, decline, export, or approve from the detail pane".to_string(),
            ),
            ("esc".to_string(), "Close modal".to_string()),
        ],
        ScreenId::Settings => vec![
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
    let _ = modal;
    model.modal_accepts_text()
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
    let Some(modal) = model.modal_state() else {
        return false;
    };
    if !modal_accepts_text(&model, modal) {
        return false;
    }
    !matches!(event.key(), Key::Enter | Key::Escape)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn runtime_projection_loaders_do_not_synthesize_authoritative_readiness() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let app_path = repo_root.join("crates/aura-ui/src/app.rs");
        let source = std::fs::read_to_string(&app_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", app_path.display()));

        let contacts_start = source
            .find("async fn load_contacts_runtime_view")
            .unwrap_or_else(|| panic!("missing load_contacts_runtime_view"));
        let settings_start = source[contacts_start..]
            .find("fn build_settings_runtime_view")
            .map(|offset| contacts_start + offset)
            .unwrap_or_else(|| panic!("missing build_settings_runtime_view"));
        let contacts_branch = &source[contacts_start..settings_start];

        let notifications_start = source
            .find("async fn load_notifications_runtime_view")
            .unwrap_or_else(|| panic!("missing load_notifications_runtime_view"));
        let next_fn = source[notifications_start..]
            .find("fn selected_home_id_for_modal")
            .map(|offset| notifications_start + offset)
            .unwrap_or_else(|| panic!("missing selected_home_id_for_modal"));
        let notifications_branch = &source[notifications_start..next_fn];

        assert!(!contacts_branch.contains("RuntimeFact::ContactLinkReady"));
        assert!(!notifications_branch.contains("RuntimeFact::PendingHomeInvitationReady"));
    }
}
