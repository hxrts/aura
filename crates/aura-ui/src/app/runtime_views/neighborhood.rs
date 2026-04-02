use crate::model::UiController;
use aura_app::ui::signals::{
    CHAT_SIGNAL, CONTACTS_SIGNAL, DISCOVERED_PEERS_SIGNAL, HOMES_SIGNAL, NEIGHBORHOOD_SIGNAL,
    NETWORK_STATUS_SIGNAL,
};
use aura_app::ui::types::{
    format_network_status_with_severity, ChatState, ContactsState, HomeRole, HomesState,
    NeighborhoodState,
};
use aura_app::views::chat::is_note_to_self_channel_name;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::ChannelId;
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct NeighborhoodRuntimeHome {
    pub(in crate::app) id: String,
    pub(in crate::app) name: String,
    pub(in crate::app) member_count: Option<u32>,
    pub(in crate::app) can_enter: bool,
    pub(in crate::app) is_local: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct NeighborhoodRuntimeMember {
    pub(in crate::app) authority_id: String,
    pub(in crate::app) name: String,
    pub(in crate::app) role_label: String,
    pub(in crate::app) is_self: bool,
    pub(in crate::app) is_online: bool,
    pub(in crate::app) is_moderator: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct NeighborhoodRuntimeChannel {
    pub(in crate::app) id: String,
    pub(in crate::app) name: String,
    pub(in crate::app) topic: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct NeighborhoodRuntimeView {
    pub(in crate::app) loaded: bool,
    pub(in crate::app) neighborhood_name: String,
    pub(in crate::app) active_home_name: String,
    pub(in crate::app) active_home_id: String,
    pub(in crate::app) homes: Vec<NeighborhoodRuntimeHome>,
    pub(in crate::app) members: Vec<NeighborhoodRuntimeMember>,
    pub(in crate::app) channels: Vec<NeighborhoodRuntimeChannel>,
    pub(in crate::app) network_status: String,
    pub(in crate::app) reachable_peers: usize,
    pub(in crate::app) online_contacts: usize,
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
    reachable_peers: usize,
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
        reachable_peers,
        online_contacts,
    }
}

pub(in crate::app) async fn load_neighborhood_runtime_view(
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
    let reachable_peers = {
        let core = controller.app_core().read().await;
        core.read(&*DISCOVERED_PEERS_SIGNAL)
            .await
            .unwrap_or_default()
            .peers
            .len()
    };

    build_neighborhood_runtime_view(
        &authority_id,
        neighborhood,
        homes,
        contacts,
        chat,
        network_status,
        reachable_peers,
    )
}
