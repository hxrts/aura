use crate::model::{NotificationSelectionId, UiController};
use aura_app::signal_defs::{DiscoveredPeersState, SettingsState};
use aura_app::ui::contract::ConfirmationState;
use aura_app::ui::signals::{
    CHAT_SIGNAL, CONTACTS_SIGNAL, DISCOVERED_PEERS_SIGNAL, ERROR_SIGNAL, HOMES_SIGNAL,
    INVITATIONS_SIGNAL, NEIGHBORHOOD_SIGNAL, NETWORK_STATUS_SIGNAL, RECOVERY_SIGNAL,
    SETTINGS_SIGNAL, TRANSPORT_PEERS_SIGNAL,
};
use aura_app::ui::types::{
    format_network_status_with_severity, AppError, ChatState, ContactsState, HomeRole, HomesState,
    InvitationsState, NeighborhoodState, RecoveryState,
};
use aura_app::ui_contract::{ChannelFactKey, RuntimeFact};
use aura_app::views::chat::is_note_to_self_channel_name;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::types::identifiers::AuthorityId;
use aura_core::ChannelId;
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct NeighborhoodRuntimeHome {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) member_count: Option<u32>,
    pub(super) can_enter: bool,
    pub(super) is_local: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct NeighborhoodRuntimeMember {
    pub(super) authority_id: String,
    pub(super) name: String,
    pub(super) role_label: String,
    pub(super) is_self: bool,
    pub(super) is_online: bool,
    pub(super) is_moderator: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct NeighborhoodRuntimeChannel {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) topic: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct NeighborhoodRuntimeView {
    pub(super) loaded: bool,
    pub(super) neighborhood_name: String,
    pub(super) active_home_name: String,
    pub(super) active_home_id: String,
    pub(super) homes: Vec<NeighborhoodRuntimeHome>,
    pub(super) members: Vec<NeighborhoodRuntimeMember>,
    pub(super) channels: Vec<NeighborhoodRuntimeChannel>,
    pub(super) network_status: String,
    pub(super) transport_peers: usize,
    pub(super) online_contacts: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ChatRuntimeChannel {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) topic: String,
    pub(super) unread_count: u32,
    pub(super) last_message: Option<String>,
    pub(super) member_count: u32,
    pub(super) is_dm: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ChatRuntimeMessage {
    pub(super) sender_name: String,
    pub(super) content: String,
    pub(super) is_own: bool,
    pub(super) delivery_status: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ChatRuntimeView {
    pub(super) loaded: bool,
    pub(super) active_channel: String,
    pub(super) channels: Vec<ChatRuntimeChannel>,
    pub(super) messages: Vec<ChatRuntimeMessage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ContactsRuntimeContact {
    pub(super) authority_id: AuthorityId,
    pub(super) name: String,
    pub(super) nickname_hint: Option<String>,
    pub(super) is_guardian: bool,
    pub(super) is_member: bool,
    pub(super) is_online: bool,
    pub(super) confirmation: ConfirmationState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ContactsRuntimePeer {
    pub(super) authority_id: AuthorityId,
    pub(super) address: String,
    pub(super) invited: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ContactsRuntimeView {
    pub(super) loaded: bool,
    pub(super) contacts: Vec<ContactsRuntimeContact>,
    pub(super) lan_peers: Vec<ContactsRuntimePeer>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SettingsRuntimeDevice {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) is_current: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SettingsRuntimeAuthority {
    pub(super) id: AuthorityId,
    pub(super) label: String,
    pub(super) is_current: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SettingsRuntimeView {
    pub(super) loaded: bool,
    pub(super) nickname: String,
    pub(super) authority_id: String,
    pub(super) threshold_k: u8,
    pub(super) threshold_n: u8,
    pub(super) guardian_count: usize,
    pub(super) active_recovery_label: String,
    pub(super) pending_recovery_requests: usize,
    pub(super) guardian_binding_count: usize,
    pub(super) mfa_policy: String,
    pub(super) devices: Vec<SettingsRuntimeDevice>,
    pub(super) authorities: Vec<SettingsRuntimeAuthority>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct NotificationRuntimeItem {
    pub(super) id: String,
    pub(super) kind_label: String,
    pub(super) title: String,
    pub(super) subtitle: String,
    pub(super) detail: String,
    pub(super) timestamp: u64,
    pub(super) action: NotificationRuntimeAction,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) enum NotificationRuntimeAction {
    #[default]
    None,
    ReceivedInvitation,
    SentInvitation,
    RecoveryApproval,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct NotificationsRuntimeView {
    pub(super) loaded: bool,
    pub(super) items: Vec<NotificationRuntimeItem>,
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
            id: channel.id.to_string(),
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
    network_status: aura_app::ui::signals::NetworkStatus,
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

pub(super) async fn load_neighborhood_runtime_view(
    controller: Arc<UiController>,
) -> NeighborhoodRuntimeView {
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

fn build_chat_runtime_view(chat: ChatState, selected_channel_id: Option<&str>) -> ChatRuntimeView {
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

    let active_channel = selected_channel_id
        .and_then(|channel_id| {
            channels
                .iter()
                .find(|channel| channel.id.eq_ignore_ascii_case(channel_id))
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

pub(super) async fn load_chat_runtime_view(controller: Arc<UiController>) -> ChatRuntimeView {
    fn saturating_u32(value: usize) -> u32 {
        u32::try_from(value).unwrap_or(u32::MAX)
    }

    let (chat, authority_id) = {
        let core = controller.app_core().read().await;
        let mut merged = core.read(&*CHAT_SIGNAL).await.unwrap_or_default();
        let authority_id = core.authority().cloned();
        if let Some(authority_id) = authority_id {
            merged.ensure_note_to_self_channel(authority_id);
        }
        (merged, authority_id)
    };
    let selected_channel_id = controller
        .ui_model()
        .and_then(|model| model.selected_channel_id().map(str::to_string));
    let runtime = build_chat_runtime_view(chat.clone(), selected_channel_id.as_deref());
    controller.push_log(&format!(
        "load_chat_runtime_view: selected={:?} active={} channels={}",
        selected_channel_id,
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
            .map(|channel| {
                (
                    channel.id.clone(),
                    channel.name.clone(),
                    channel.topic.clone(),
                )
            })
            .collect(),
        runtime_facts,
    );
    runtime
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
        .filter(|peer| peer.method == aura_app::ui::signals::DiscoveredPeerMethod::Lan)
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

pub(super) async fn load_contacts_runtime_view(
    controller: Arc<UiController>,
) -> ContactsRuntimeView {
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

pub(super) async fn load_settings_runtime_view(
    controller: Arc<UiController>,
) -> SettingsRuntimeView {
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

pub(super) async fn load_notifications_runtime_view(
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
