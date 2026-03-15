//! iocraft hook helpers for long-lived reactive subscriptions.
//!
//! Keep shell.rs focused on wiring and rendering by extracting the
//! signal-subscription use_future homes here.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use iocraft::prelude::*;

use aura_app::ui::signals::{
    ConnectionStatus, DiscoveredPeer, DiscoveredPeerMethod, NetworkStatus,
    AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL, CHAT_SIGNAL, CONNECTION_STATUS_SIGNAL, CONTACTS_SIGNAL,
    DISCOVERED_PEERS_SIGNAL, HOMES_SIGNAL, INVITATIONS_SIGNAL, NEIGHBORHOOD_SIGNAL,
    NETWORK_STATUS_SIGNAL, RECOVERY_SIGNAL, SETTINGS_SIGNAL, TRANSPORT_PEERS_SIGNAL,
};
use aura_app::ui::types::{ChatState, ContactsState, HomesState};
use aura_app::ui_contract::{
    bridged_operation_statuses, AuthoritativeSemanticFact, RuntimeEventKind,
};
use aura_core::AuthorityId;

use crate::tui::chat_scope::{
    active_home_scope_id, effective_home_scope_id, is_dm_like_channel, scoped_channels,
};
use crate::tui::hooks::{subscribe_signal_with_retry, AppCoreContext};
use crate::tui::semantic_lifecycle::authoritative_operation_status_update;
use crate::tui::types::{Channel, Contact, Device, Invitation, Message, PendingRequest};
use crate::tui::updates::{UiUpdate, UiUpdateSender};

fn publish_ui_update(tx: &UiUpdateSender, update: UiUpdate) {
    if tx.try_send(update.clone()).is_err() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(update).await;
        });
    }
}

fn publish_ui_updates_ordered(
    tx: &UiUpdateSender,
    ordered_gate: &Arc<tokio::sync::Mutex<()>>,
    updates: Vec<UiUpdate>,
) {
    if updates.is_empty() {
        return;
    }

    let tx = tx.clone();
    let ordered_gate = Arc::clone(ordered_gate);
    tokio::spawn(async move {
        let _guard = ordered_gate.lock().await;
        for update in updates {
            if tx.try_send(update.clone()).is_err() {
                let _ = tx.send(update).await;
            }
        }
    });
}

fn authoritative_runtime_replace_kinds() -> Vec<RuntimeEventKind> {
    vec![
        RuntimeEventKind::ContactLinkReady,
        RuntimeEventKind::PendingHomeInvitationReady,
        RuntimeEventKind::ChannelMembershipReady,
        RuntimeEventKind::RecipientPeersResolved,
        RuntimeEventKind::MessageDeliveryReady,
    ]
}

fn is_dm_like_shared_channel(channel: &Channel) -> bool {
    channel.name.to_ascii_lowercase().starts_with("dm:")
        || channel
            .topic
            .as_deref()
            .map(|topic| topic.to_ascii_lowercase().starts_with("direct messages"))
            .unwrap_or(false)
}

/// Shared authority id state for UI dispatch handlers.
#[derive(Clone, Default)]
pub struct SharedAuthorityId(Arc<RwLock<Option<AuthorityId>>>);

impl SharedAuthorityId {
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(None)))
    }

    pub fn read(
        &self,
    ) -> std::sync::LockResult<std::sync::RwLockReadGuard<'_, Option<AuthorityId>>> {
        self.0.read()
    }

    pub fn write(
        &self,
    ) -> std::sync::LockResult<std::sync::RwLockWriteGuard<'_, Option<AuthorityId>>> {
        self.0.write()
    }
}

/// Create a shared authority id holder and subscribe it to SETTINGS_SIGNAL.
pub fn use_authority_id_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
) -> SharedAuthorityId {
    let shared_ref = hooks.use_ref(SharedAuthorityId::new);
    let shared: SharedAuthorityId = shared_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let authority_id = shared.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*SETTINGS_SIGNAL, move |settings_state| {
                if let Ok(mut guard) = authority_id.write() {
                    *guard = settings_state.authority_id.parse::<AuthorityId>().ok();
                }
            })
            .await;
        }
    });

    shared
}

pub struct NavStatusSignals {
    pub network_status: State<NetworkStatus>,
    /// Online contacts (people you know who are currently online)
    pub known_online: State<usize>,
    /// Transport-level peers (active network connections)
    pub transport_peers: State<usize>,
    pub now_ms: State<Option<u64>>,
}

pub fn use_nav_status_signals(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    initial_network_status: NetworkStatus,
    initial_known_online: usize,
    initial_transport_peers: usize,
) -> NavStatusSignals {
    let network_status = hooks.use_state(|| initial_network_status);
    let known_online = hooks.use_state(|| initial_known_online);
    let transport_peers = hooks.use_state(|| initial_transport_peers);
    let now_ms = hooks.use_state(|| None::<u64>);

    // Subscribe to unified network status signal
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut network_status = network_status.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*NETWORK_STATUS_SIGNAL, move |status| {
                if network_status.get() != status {
                    network_status.set(status);
                }
            })
            .await;
        }
    });

    // Subscribe to connection status for online contacts count
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut known_online = known_online.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CONNECTION_STATUS_SIGNAL, move |status| {
                let count = match status {
                    ConnectionStatus::Online { peer_count } => peer_count,
                    _ => 0,
                };
                if known_online.get() != count {
                    known_online.set(count);
                }
            })
            .await;
        }
    });

    // Subscribe to transport peers signal
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut transport_peers = transport_peers.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*TRANSPORT_PEERS_SIGNAL, move |count| {
                if transport_peers.get() != count {
                    transport_peers.set(count);
                }
            })
            .await;
        }
    });

    // Keep a best-effort physical clock for relative-time UI formatting.
    // This must come from the runtime/effects system (not OS clock).
    // Exits after 200 consecutive failures (~3+ minutes) to avoid running
    // forever during shutdown.
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut now_ms = now_ms.clone();
        async move {
            let mut consecutive_failures = 0u32;
            loop {
                let runtime = app_core.raw().read().await.runtime().cloned();
                if let Some(runtime) = runtime {
                    if let Ok(ts) = runtime.current_time_ms().await {
                        let next = Some(ts);
                        if now_ms.get() != next {
                            now_ms.set(next);
                        }
                        consecutive_failures = 0;
                    } else {
                        consecutive_failures += 1;
                    }
                } else {
                    consecutive_failures += 1;
                }
                if consecutive_failures > 200 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1000)).await;
            }
        }
    });

    NavStatusSignals {
        network_status,
        known_online,
        transport_peers,
        now_ms,
    }
}

/// Shared contacts state that can be read by closures without re-rendering.
///
/// This wraps Arc<RwLock<Vec<Contact>>> instead of State<T> because:
/// 1. Dispatch handler closures need to read current contacts at invocation time.
/// 2. We do not want every contacts update to trigger shell re-renders.
/// 3. The closure captures the Arc, not the data, so it always reads fresh data.
#[derive(Clone, Default)]
pub struct SharedContacts(Arc<RwLock<Vec<Contact>>>);

impl SharedContacts {
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(Vec::new())))
    }

    pub fn read(&self) -> std::sync::LockResult<std::sync::RwLockReadGuard<'_, Vec<Contact>>> {
        self.0.read()
    }

    pub fn write(&self) -> std::sync::LockResult<std::sync::RwLockWriteGuard<'_, Vec<Contact>>> {
        self.0.write()
    }
}

/// Shared discovered peers state that can be read by closures without re-rendering.
#[derive(Clone, Default)]
pub struct SharedDiscoveredPeers(Arc<RwLock<Vec<DiscoveredPeer>>>);

impl SharedDiscoveredPeers {
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(Vec::new())))
    }

    pub fn read(
        &self,
    ) -> std::sync::LockResult<std::sync::RwLockReadGuard<'_, Vec<DiscoveredPeer>>> {
        self.0.read()
    }

    pub fn write(
        &self,
    ) -> std::sync::LockResult<std::sync::RwLockWriteGuard<'_, Vec<DiscoveredPeer>>> {
        self.0.write()
    }
}

/// Create a shared discovered peers holder and subscribe it to DISCOVERED_PEERS_SIGNAL.
///
/// Returns an Arc that closures can capture. The subscription updates the Arc's
/// contents whenever discovery changes, so readers always get current data.
///
/// If `update_tx` is provided, sends `LanPeersCountChanged` whenever the LAN peer count changes.
pub fn use_discovered_peers_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    update_tx: Option<UiUpdateSender>,
) -> SharedDiscoveredPeers {
    let shared_ref = hooks.use_ref(SharedDiscoveredPeers::new);
    let shared: SharedDiscoveredPeers = shared_ref.read().clone();
    let last_lan_count_ref = hooks.use_ref(|| Arc::new(AtomicUsize::new(usize::MAX)));
    let last_lan_count = last_lan_count_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let peers = shared.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*DISCOVERED_PEERS_SIGNAL, move |peers_state| {
                let lan_peers: Vec<_> = peers_state
                    .peers
                    .iter()
                    .filter(|p| p.method == DiscoveredPeerMethod::Lan)
                    .cloned()
                    .collect();

                let new_count = lan_peers.len();

                if let Ok(mut guard) = peers.write() {
                    *guard = lan_peers;
                }

                if let Some(ref tx) = update_tx {
                    let previous = last_lan_count.swap(new_count, Ordering::Relaxed);
                    if previous != new_count {
                        publish_ui_update(tx, UiUpdate::LanPeersCountChanged(new_count));
                    }
                }
            })
            .await;
        }
    });

    shared
}

/// Create a shared contacts holder and subscribe it to CONTACTS_SIGNAL.
///
/// Returns an Arc that closures can capture. The subscription updates the Arc's
/// contents whenever contacts change, so readers always get current data.
///
/// Uses std::sync::RwLock so dispatch handlers can read synchronously.
///
/// If `update_tx` is provided, sends `ContactCountChanged` whenever the contact count changes.
/// This keeps `TuiState.contacts.contact_count` in sync for keyboard navigation.
pub fn use_contacts_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    update_tx: Option<UiUpdateSender>,
) -> SharedContacts {
    // Create the shared contacts holder - use_ref ensures it persists across renders.
    let shared_contacts_ref = hooks.use_ref(SharedContacts::new);
    let shared_contacts: SharedContacts = shared_contacts_ref.read().clone();
    let last_contact_count_ref = hooks.use_ref(|| Arc::new(AtomicUsize::new(usize::MAX)));
    let last_contact_count = last_contact_count_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let contacts = shared_contacts.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CONTACTS_SIGNAL, move |contacts_state| {
                let contact_list: Vec<Contact> =
                    contacts_state.all_contacts().map(Contact::from).collect();
                let new_count = contact_list.len();

                if let Ok(mut guard) = contacts.write() {
                    *guard = contact_list;
                }

                // Send contact count update for keyboard navigation
                if let Some(ref tx) = update_tx {
                    let previous = last_contact_count.swap(new_count, Ordering::Relaxed);
                    if previous != new_count {
                        publish_ui_update(tx, UiUpdate::ContactCountChanged(new_count));
                    }
                }
            })
            .await;
        }
    });

    shared_contacts
}

/// Shared devices state (account devices) that can be read by closures without re-rendering.
#[derive(Clone, Default)]
pub struct SharedDevices(Arc<RwLock<Vec<Device>>>);

impl SharedDevices {
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(Vec::new())))
    }

    pub fn read(&self) -> std::sync::LockResult<std::sync::RwLockReadGuard<'_, Vec<Device>>> {
        self.0.read()
    }

    pub fn write(&self) -> std::sync::LockResult<std::sync::RwLockWriteGuard<'_, Vec<Device>>> {
        self.0.write()
    }
}

/// Create a shared devices holder and subscribe it to SETTINGS_SIGNAL.
pub fn use_devices_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    update_tx: Option<UiUpdateSender>,
) -> SharedDevices {
    let shared_devices_ref = hooks.use_ref(SharedDevices::new);
    let shared_devices: SharedDevices = shared_devices_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let devices = shared_devices.clone();
        let update_tx = update_tx.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*SETTINGS_SIGNAL, move |settings_state| {
                let list: Vec<Device> = settings_state
                    .devices
                    .iter()
                    .map(|d| Device {
                        id: d.id.to_string(),
                        name: d.name.clone(),
                        is_current: d.is_current,
                        last_seen: d.last_seen,
                    })
                    .collect();
                if let Ok(mut guard) = devices.write() {
                    *guard = list;
                }
                if settings_state.devices.len() >= 2 {
                    if let Some(tx) = update_tx.as_ref() {
                        publish_ui_update(tx, UiUpdate::RuntimeBootstrapFinalized);
                    }
                }
            })
            .await;
        }
    });

    shared_devices
}

/// Shared messages state that can be read by closures without re-rendering.
///
/// This uses Arc<RwLock<Vec<Message>>> instead of State<T> because:
/// 1. Dispatch handler closures need to look up messages by ID (e.g., for retry).
/// 2. We do not want every message update to trigger shell re-renders.
/// 3. The closure captures the Arc, not the data, so it always reads fresh data.
pub type SharedMessages = Arc<RwLock<Vec<Message>>>;

/// Create a shared messages holder and subscribe it to CHAT_SIGNAL.
///
/// Returns an Arc that closures can capture. The subscription updates the Arc's
/// contents whenever chat state changes, so readers always get current data.
///
/// Uses std::sync::RwLock so dispatch handlers can read synchronously.
pub fn use_messages_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    selected_channel_id: Arc<RwLock<Option<String>>>,
) -> SharedMessages {
    // Create the shared messages holder - use_ref ensures it persists across renders.
    let shared_messages_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_messages: SharedMessages = shared_messages_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let messages = shared_messages.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CHAT_SIGNAL, move |chat_state| {
                let channel_id = {
                    selected_channel_id
                        .read()
                        .ok()
                        .and_then(|guard| guard.clone())
                };

                // Get messages for that channel (or empty if none selected)
                let message_list: Vec<Message> = if let Some(channel_id) = channel_id {
                    if let Some(cid) = chat_state
                        .all_channels()
                        .find(|channel| channel.id.to_string() == channel_id)
                        .map(|channel| channel.id)
                    {
                        chat_state
                            .messages_for_channel(&cid)
                            .iter()
                            .map(Message::from)
                            .collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };

                if let Ok(mut guard) = messages.write() {
                    *guard = message_list;
                }
            })
            .await;
        }
    });

    shared_messages
}

/// Shared channels state that can be read by closures without re-rendering.
///
/// Used to map selected channel index -> channel ID for send operations.
pub type SharedChannels = Arc<RwLock<Vec<Channel>>>;

fn merge_transient_channels(
    incoming: &ChatState,
    previous: &ChatState,
    _selected_channel_id: Option<&str>,
) -> ChatState {
    if incoming.channel_count() == 0 && previous.channel_count() > 0 {
        let had_dm_like = previous.all_channels().any(is_dm_like_channel);
        if had_dm_like {
            // Runtime reductions may briefly publish an empty snapshot during convergence.
            // Preserve DM-like channels in that transient case, but still allow explicit
            // non-DM channel leaves to converge to an empty channel list.
            return previous.clone();
        }
    }

    let mut merged = incoming.clone();

    for channel in previous.all_channels() {
        if !is_dm_like_channel(channel) || merged.has_channel(&channel.id) {
            continue;
        }

        merged.upsert_channel(channel.clone());
        for message in previous.messages_for_channel(&channel.id) {
            merged.apply_message(channel.id, message.clone());
        }
    }

    merged
}

#[cfg(test)]
fn scoped_channel_snapshot(
    chat_state: &ChatState,
    active_scope: Option<&str>,
) -> (Vec<Channel>, usize) {
    let scoped = scoped_channels(chat_state, active_scope);
    let message_count = scoped
        .iter()
        .map(|channel| chat_state.messages_for_channel(&channel.id).len())
        .sum();
    let channels = scoped.into_iter().map(Channel::from).collect();
    (channels, message_count)
}

fn publish_scoped_channels(
    channels: &SharedChannels,
    selected_channel_id: Option<&str>,
    update_tx: &Option<UiUpdateSender>,
    last_channel_count: &Arc<AtomicUsize>,
    last_message_count: &Arc<AtomicUsize>,
    last_channel_signature: &Arc<RwLock<Option<String>>>,
    _transport_peer_count: usize,
    _discovered_peer_ids: &[AuthorityId],
    _self_authority: Option<AuthorityId>,
    _homes_state: &HomesState,
    _contacts_state: &ContactsState,
    chat_state: &ChatState,
    active_scope: Option<&str>,
) {
    let effective_scope = effective_home_scope_id(chat_state, active_scope, selected_channel_id);
    let scoped = scoped_channels(chat_state, effective_scope.as_deref());
    let message_count = scoped
        .iter()
        .map(|channel| chat_state.messages_for_channel(&channel.id).len())
        .sum();
    let mut channel_list: Vec<Channel> = scoped.iter().copied().map(Channel::from).collect();
    if let Some(selected_channel_id) = selected_channel_id {
        let already_present = channel_list
            .iter()
            .any(|channel| channel.id == selected_channel_id);
        if !already_present {
            let preserved = channels.read().ok().and_then(|guard| {
                guard
                    .iter()
                    .find(|channel| {
                        channel.id == selected_channel_id && is_dm_like_shared_channel(channel)
                    })
                    .cloned()
            });
            if let Some(channel) = preserved {
                channel_list.push(channel);
                channel_list.sort_by(|left, right| left.name.cmp(&right.name));
            }
        }
    }
    let channel_count = channel_list.len();
    let channel_signature = channel_list
        .iter()
        .map(|channel| channel.id.as_str())
        .collect::<Vec<_>>()
        .join("|");
    if let Ok(mut guard) = channels.write() {
        *guard = channel_list;
    }

    if let Some(tx) = update_tx {
        let channel_signature_changed = {
            let mut guard = last_channel_signature
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let changed = guard.as_deref() != Some(channel_signature.as_str());
            *guard = Some(channel_signature);
            changed
        };
        let channel_changed =
            last_channel_count.swap(channel_count, Ordering::Relaxed) != channel_count;
        let message_changed =
            last_message_count.swap(message_count, Ordering::Relaxed) != message_count;
        if !(channel_changed || message_changed || channel_signature_changed) {
            return;
        }
        publish_ui_update(
            tx,
            UiUpdate::ChatStateUpdated {
                channel_count,
                message_count,
                selected_index: None,
            },
        );
    }
}

/// Create a shared channels holder and subscribe it to CHAT_SIGNAL.
pub fn use_channels_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    shared_authority_id: SharedAuthorityId,
    selected_channel_id: Arc<RwLock<Option<String>>>,
    update_tx: Option<UiUpdateSender>,
) -> SharedChannels {
    let shared_channels_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_channels: SharedChannels = shared_channels_ref.read().clone();
    let active_scope_ref = hooks.use_ref(|| Arc::new(RwLock::new(None::<String>)));
    let active_scope: Arc<RwLock<Option<String>>> = active_scope_ref.read().clone();
    let latest_chat_state_ref = hooks.use_ref(|| Arc::new(RwLock::new(ChatState::default())));
    let latest_chat_state: Arc<RwLock<ChatState>> = latest_chat_state_ref.read().clone();
    let last_channel_count_ref = hooks.use_ref(|| Arc::new(AtomicUsize::new(usize::MAX)));
    let last_channel_count = last_channel_count_ref.read().clone();
    let last_message_count_ref = hooks.use_ref(|| Arc::new(AtomicUsize::new(usize::MAX)));
    let last_message_count = last_message_count_ref.read().clone();
    let last_channel_signature_ref = hooks.use_ref(|| Arc::new(RwLock::new(None::<String>)));
    let last_channel_signature = last_channel_signature_ref.read().clone();
    let latest_transport_peer_count_ref = hooks.use_ref(|| Arc::new(AtomicUsize::new(0)));
    let latest_transport_peer_count = latest_transport_peer_count_ref.read().clone();
    let latest_contacts_state_ref =
        hooks.use_ref(|| Arc::new(RwLock::new(ContactsState::default())));
    let latest_contacts_state = latest_contacts_state_ref.read().clone();
    let latest_homes_state_ref = hooks.use_ref(|| Arc::new(RwLock::new(HomesState::default())));
    let latest_homes_state = latest_homes_state_ref.read().clone();
    let latest_discovered_peers_ref =
        hooks.use_ref(|| Arc::new(RwLock::new(Vec::<AuthorityId>::new())));
    let latest_discovered_peers = latest_discovered_peers_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let channels = shared_channels.clone();
        let active_scope = active_scope.clone();
        let latest_chat_state = latest_chat_state.clone();
        let shared_authority_id = shared_authority_id.clone();
        let update_tx = update_tx.clone();
        let last_channel_count = last_channel_count.clone();
        let last_message_count = last_message_count.clone();
        let last_channel_signature = last_channel_signature.clone();
        let latest_transport_peer_count = latest_transport_peer_count.clone();
        let latest_contacts_state = latest_contacts_state.clone();
        let latest_homes_state = latest_homes_state.clone();
        let latest_discovered_peers = latest_discovered_peers.clone();
        let selected_channel_id = selected_channel_id.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CHAT_SIGNAL, move |chat_state| {
                let mut stabilized = latest_chat_state
                    .read()
                    .ok()
                    .map(|previous| {
                        let selected_channel = selected_channel_id
                            .read()
                            .ok()
                            .and_then(|guard| guard.clone());
                        merge_transient_channels(
                            &chat_state,
                            &previous,
                            selected_channel.as_deref(),
                        )
                    })
                    .unwrap_or_else(|| chat_state.clone());
                if let Some(authority_id) = shared_authority_id
                    .read()
                    .ok()
                    .and_then(|guard| guard.clone())
                {
                    stabilized.ensure_note_to_self_channel(authority_id);
                }
                tracing::debug!(
                    "CHAT_SIGNAL_UPDATE: incoming={} stabilized={}",
                    chat_state.channel_count(),
                    stabilized.channel_count()
                );
                let channel_summary = stabilized
                    .all_channels()
                    .map(|channel| {
                        format!(
                            "{}|is_dm={}|name={}|topic={}",
                            channel.id,
                            channel.is_dm,
                            channel.name,
                            channel.topic.clone().unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ; ");
                tracing::debug!("CHAT_SIGNAL_CHANNELS: {channel_summary}");

                if let Ok(mut guard) = latest_chat_state.write() {
                    *guard = stabilized.clone();
                }

                let scope = active_scope.read().ok().and_then(|g| g.clone());
                let authority_id = shared_authority_id.read().ok().and_then(|guard| *guard);
                let contacts_state = latest_contacts_state
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let transport_peer_count = latest_transport_peer_count.load(Ordering::Relaxed);
                let discovered_peer_ids = latest_discovered_peers
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let homes_state = latest_homes_state
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let selected_channel = selected_channel_id
                    .read()
                    .ok()
                    .and_then(|guard| guard.clone());
                publish_scoped_channels(
                    &channels,
                    selected_channel.as_deref(),
                    &update_tx,
                    &last_channel_count,
                    &last_message_count,
                    &last_channel_signature,
                    transport_peer_count,
                    &discovered_peer_ids,
                    authority_id,
                    &homes_state,
                    &contacts_state,
                    &stabilized,
                    scope.as_deref(),
                );
            })
            .await;
        }
    });

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let channels = shared_channels.clone();
        let latest_chat_state = latest_chat_state.clone();
        let active_scope = active_scope.clone();
        let update_tx = update_tx.clone();
        let last_channel_count = last_channel_count.clone();
        let last_message_count = last_message_count.clone();
        let last_channel_signature = last_channel_signature.clone();
        let latest_transport_peer_count = latest_transport_peer_count.clone();
        let latest_contacts_state = latest_contacts_state.clone();
        let latest_homes_state = latest_homes_state.clone();
        let latest_discovered_peers = latest_discovered_peers.clone();
        let shared_authority_id = shared_authority_id.clone();
        let selected_channel_id = selected_channel_id.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*SETTINGS_SIGNAL, move |settings_state| {
                let authority_id = settings_state.authority_id.parse::<AuthorityId>().ok();
                if let Ok(mut guard) = shared_authority_id.write() {
                    *guard = authority_id;
                }

                let Some(authority_id) = authority_id else {
                    return;
                };

                let mut chat_state = latest_chat_state
                    .read()
                    .ok()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                chat_state.ensure_note_to_self_channel(authority_id);
                if let Ok(mut guard) = latest_chat_state.write() {
                    *guard = chat_state.clone();
                }
                let scope = active_scope.read().ok().and_then(|g| g.clone());
                let contacts_state = latest_contacts_state
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let transport_peer_count = latest_transport_peer_count.load(Ordering::Relaxed);
                let discovered_peer_ids = latest_discovered_peers
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let homes_state = latest_homes_state
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let selected_channel = selected_channel_id
                    .read()
                    .ok()
                    .and_then(|guard| guard.clone());
                publish_scoped_channels(
                    &channels,
                    selected_channel.as_deref(),
                    &update_tx,
                    &last_channel_count,
                    &last_message_count,
                    &last_channel_signature,
                    transport_peer_count,
                    &discovered_peer_ids,
                    Some(authority_id),
                    &homes_state,
                    &contacts_state,
                    &chat_state,
                    scope.as_deref(),
                );
            })
            .await;
        }
    });

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let channels = shared_channels.clone();
        let latest_chat_state = latest_chat_state.clone();
        let active_scope = active_scope.clone();
        let update_tx = update_tx.clone();
        let last_channel_count = last_channel_count.clone();
        let last_message_count = last_message_count.clone();
        let last_channel_signature = last_channel_signature.clone();
        let latest_transport_peer_count = latest_transport_peer_count.clone();
        let latest_contacts_state = latest_contacts_state.clone();
        let latest_homes_state = latest_homes_state.clone();
        let latest_discovered_peers = latest_discovered_peers.clone();
        let shared_authority_id = shared_authority_id.clone();
        let selected_channel_id = selected_channel_id.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*NEIGHBORHOOD_SIGNAL, move |neighborhood| {
                let scope = active_home_scope_id(&neighborhood);
                if let Ok(mut guard) = active_scope.write() {
                    *guard = Some(scope.clone());
                }

                let chat_state = latest_chat_state
                    .read()
                    .ok()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                let authority_id = shared_authority_id.read().ok().and_then(|guard| *guard);
                let contacts_state = latest_contacts_state
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let transport_peer_count = latest_transport_peer_count.load(Ordering::Relaxed);
                let discovered_peer_ids = latest_discovered_peers
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let homes_state = latest_homes_state
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let selected_channel = selected_channel_id
                    .read()
                    .ok()
                    .and_then(|guard| guard.clone());
                publish_scoped_channels(
                    &channels,
                    selected_channel.as_deref(),
                    &update_tx,
                    &last_channel_count,
                    &last_message_count,
                    &last_channel_signature,
                    transport_peer_count,
                    &discovered_peer_ids,
                    authority_id,
                    &homes_state,
                    &contacts_state,
                    &chat_state,
                    Some(scope.as_str()),
                );
            })
            .await;
        }
    });

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let channels = shared_channels.clone();
        let latest_chat_state = latest_chat_state.clone();
        let active_scope = active_scope.clone();
        let update_tx = update_tx.clone();
        let last_channel_count = last_channel_count.clone();
        let last_message_count = last_message_count.clone();
        let last_channel_signature = last_channel_signature.clone();
        let latest_transport_peer_count = latest_transport_peer_count.clone();
        let latest_contacts_state = latest_contacts_state.clone();
        let latest_homes_state = latest_homes_state.clone();
        let latest_discovered_peers = latest_discovered_peers.clone();
        let shared_authority_id = shared_authority_id.clone();
        let selected_channel_id = selected_channel_id.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CONTACTS_SIGNAL, move |contacts_state| {
                if let Ok(mut guard) = latest_contacts_state.write() {
                    *guard = contacts_state.clone();
                }
                let chat_state = latest_chat_state
                    .read()
                    .ok()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                let scope = active_scope.read().ok().and_then(|g| g.clone());
                let authority_id = shared_authority_id.read().ok().and_then(|guard| *guard);
                let transport_peer_count = latest_transport_peer_count.load(Ordering::Relaxed);
                let discovered_peer_ids = latest_discovered_peers
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let homes_state = latest_homes_state
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let selected_channel = selected_channel_id
                    .read()
                    .ok()
                    .and_then(|guard| guard.clone());
                publish_scoped_channels(
                    &channels,
                    selected_channel.as_deref(),
                    &update_tx,
                    &last_channel_count,
                    &last_message_count,
                    &last_channel_signature,
                    transport_peer_count,
                    &discovered_peer_ids,
                    authority_id,
                    &homes_state,
                    &contacts_state,
                    &chat_state,
                    scope.as_deref(),
                );
            })
            .await;
        }
    });

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let channels = shared_channels.clone();
        let latest_chat_state_handle = latest_chat_state.clone();
        let active_scope_handle = active_scope.clone();
        let update_tx = update_tx.clone();
        let last_channel_count_handle = last_channel_count.clone();
        let last_message_count_handle = last_message_count.clone();
        let last_channel_signature_handle = last_channel_signature.clone();
        let latest_transport_peer_count_handle = latest_transport_peer_count.clone();
        let latest_contacts_state_handle = latest_contacts_state.clone();
        let latest_homes_state_handle = latest_homes_state.clone();
        let latest_discovered_peers_handle = latest_discovered_peers.clone();
        let shared_authority_id_handle = shared_authority_id.clone();
        let selected_channel_id_handle = selected_channel_id.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*HOMES_SIGNAL, move |homes_state| {
                if let Ok(mut guard) = latest_homes_state_handle.write() {
                    *guard = homes_state.clone();
                }
                let chat_state = latest_chat_state_handle
                    .read()
                    .ok()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                let scope = active_scope_handle.read().ok().and_then(|g| g.clone());
                let authority_id = shared_authority_id_handle
                    .read()
                    .ok()
                    .and_then(|guard| *guard);
                let transport_peer_count =
                    latest_transport_peer_count_handle.load(Ordering::Relaxed);
                let discovered_peer_ids = latest_discovered_peers_handle
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let contacts_state = latest_contacts_state_handle
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let selected_channel = selected_channel_id_handle
                    .read()
                    .ok()
                    .and_then(|guard| guard.clone());
                publish_scoped_channels(
                    &channels,
                    selected_channel.as_deref(),
                    &update_tx,
                    &last_channel_count_handle,
                    &last_message_count_handle,
                    &last_channel_signature_handle,
                    transport_peer_count,
                    &discovered_peer_ids,
                    authority_id,
                    &homes_state,
                    &contacts_state,
                    &chat_state,
                    scope.as_deref(),
                );
            })
            .await;
        }
    });

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let channels = shared_channels.clone();
        let latest_chat_state = latest_chat_state.clone();
        let active_scope = active_scope.clone();
        let update_tx = update_tx.clone();
        let last_channel_count = last_channel_count.clone();
        let last_message_count = last_message_count.clone();
        let last_channel_signature = last_channel_signature.clone();
        let latest_transport_peer_count = latest_transport_peer_count.clone();
        let latest_contacts_state = latest_contacts_state.clone();
        let latest_homes_state = latest_homes_state.clone();
        let latest_discovered_peers = latest_discovered_peers.clone();
        let shared_authority_id = shared_authority_id.clone();
        let selected_channel_id = selected_channel_id.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*TRANSPORT_PEERS_SIGNAL, move |count| {
                latest_transport_peer_count.store(count, Ordering::Relaxed);
                let chat_state = latest_chat_state
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let scope = active_scope.read().ok().and_then(|guard| guard.clone());
                let authority_id = shared_authority_id.read().ok().and_then(|guard| *guard);
                let contacts_state = latest_contacts_state
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let homes_state = latest_homes_state
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let discovered_peer_ids = latest_discovered_peers
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let selected_channel = selected_channel_id
                    .read()
                    .ok()
                    .and_then(|guard| guard.clone());
                publish_scoped_channels(
                    &channels,
                    selected_channel.as_deref(),
                    &update_tx,
                    &last_channel_count,
                    &last_message_count,
                    &last_channel_signature,
                    count,
                    &discovered_peer_ids,
                    authority_id,
                    &homes_state,
                    &contacts_state,
                    &chat_state,
                    scope.as_deref(),
                );
            })
            .await;
        }
    });

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let channels = shared_channels.clone();
        let latest_chat_state = latest_chat_state.clone();
        let active_scope = active_scope.clone();
        let update_tx = update_tx.clone();
        let last_channel_count = last_channel_count.clone();
        let last_message_count = last_message_count.clone();
        let last_channel_signature = last_channel_signature.clone();
        let latest_transport_peer_count = latest_transport_peer_count.clone();
        let latest_contacts_state = latest_contacts_state.clone();
        let latest_homes_state = latest_homes_state.clone();
        let latest_discovered_peers = latest_discovered_peers.clone();
        let shared_authority_id = shared_authority_id.clone();
        let selected_channel_id = selected_channel_id.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*DISCOVERED_PEERS_SIGNAL, move |peers_state| {
                let discovered_peer_ids = peers_state
                    .peers
                    .iter()
                    .map(|peer| peer.authority_id)
                    .collect::<Vec<_>>();
                if let Ok(mut guard) = latest_discovered_peers.write() {
                    *guard = discovered_peer_ids.clone();
                }

                let chat_state = latest_chat_state
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let scope = active_scope.read().ok().and_then(|guard| guard.clone());
                let authority_id = shared_authority_id.read().ok().and_then(|guard| *guard);
                let contacts_state = latest_contacts_state
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let homes_state = latest_homes_state
                    .read()
                    .ok()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let transport_peer_count = latest_transport_peer_count.load(Ordering::Relaxed);
                let selected_channel = selected_channel_id
                    .read()
                    .ok()
                    .and_then(|guard| guard.clone());
                publish_scoped_channels(
                    &channels,
                    selected_channel.as_deref(),
                    &update_tx,
                    &last_channel_count,
                    &last_message_count,
                    &last_channel_signature,
                    transport_peer_count,
                    &discovered_peer_ids,
                    authority_id,
                    &homes_state,
                    &contacts_state,
                    &chat_state,
                    scope.as_deref(),
                );
            })
            .await;
        }
    });

    shared_channels
}

/// Shared invitations state that can be read by closures without re-rendering.
///
/// Used to map selected invitation index -> invitation ID for accept/decline/export.
pub type SharedInvitations = Arc<RwLock<Vec<Invitation>>>;

/// Create a shared invitations holder and subscribe it to INVITATIONS_SIGNAL.
pub fn use_invitations_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    _update_tx: Option<UiUpdateSender>,
) -> SharedInvitations {
    let shared_invitations_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_invitations: SharedInvitations = shared_invitations_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let invitations = shared_invitations.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*INVITATIONS_SIGNAL, move |inv_state| {
                let all: Vec<Invitation> = inv_state
                    .all_pending()
                    .iter()
                    .chain(inv_state.all_sent().iter())
                    .chain(inv_state.all_history().iter())
                    .map(Invitation::from)
                    .collect();

                if let Ok(mut guard) = invitations.write() {
                    *guard = all;
                }
            })
            .await;
        }
    });

    shared_invitations
}

pub fn use_authoritative_semantic_facts_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    update_tx: Option<UiUpdateSender>,
) {
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let ordered_gate = Arc::new(tokio::sync::Mutex::new(()));
        async move {
            subscribe_signal_with_retry(
                app_core,
                &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
                move |facts| {
                    let Some(ref tx) = update_tx else {
                        return;
                    };
                    let mut updates = Vec::new();
                    for (operation_id, instance_id, status) in bridged_operation_statuses(&facts) {
                        updates.push(authoritative_operation_status_update(
                            operation_id,
                            instance_id,
                            status,
                        ));
                    }
                    let mapped = facts
                        .iter()
                        .filter_map(AuthoritativeSemanticFact::runtime_fact_bridge)
                        .collect::<Vec<_>>();
                    let facts = mapped.into_iter().map(|(_, fact)| fact).collect::<Vec<_>>();
                    updates.push(UiUpdate::RuntimeFactsUpdated {
                        replace_kinds: authoritative_runtime_replace_kinds(),
                        facts,
                    });
                    publish_ui_updates_ordered(tx, &ordered_gate, updates);
                },
            )
            .await;
        }
    });
}

/// Shared neighborhood home IDs (in display order).
///
/// Used to map neighborhood grid index -> home ID for EnterHome.
pub type SharedNeighborhoodHomes = Arc<RwLock<Vec<String>>>;

/// Create a shared neighborhood homes holder and subscribe it to NEIGHBORHOOD_SIGNAL.
pub fn use_neighborhood_homes_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
) -> SharedNeighborhoodHomes {
    let shared_homes_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_homes: SharedNeighborhoodHomes = shared_homes_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let homes = shared_homes.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*NEIGHBORHOOD_SIGNAL, move |n| {
                let mut ids: Vec<String> = Vec::with_capacity(n.neighbor_count() + 1);
                ids.push(n.home_home_id.to_string());
                ids.extend(
                    n.all_neighbors()
                        .filter(|b| b.id != n.home_home_id)
                        .map(|b| b.id.to_string()),
                );
                if let Ok(mut guard) = homes.write() {
                    *guard = ids;
                }
            })
            .await;
        }
    });

    shared_homes
}

/// Shared current-home metadata used by neighborhood state machine navigation.
#[derive(Clone, Copy, Debug, Default)]
pub struct NeighborhoodHomeMeta {
    pub member_count: usize,
    pub moderator_actions_enabled: bool,
}

pub type SharedNeighborhoodHomeMeta = Arc<RwLock<NeighborhoodHomeMeta>>;

/// Create shared current-home metadata from HOMES_SIGNAL.
pub fn use_neighborhood_home_meta_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
) -> SharedNeighborhoodHomeMeta {
    let shared_meta_ref = hooks.use_ref(|| Arc::new(RwLock::new(NeighborhoodHomeMeta::default())));
    let shared_meta: SharedNeighborhoodHomeMeta = shared_meta_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let meta = shared_meta.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*HOMES_SIGNAL, move |homes_state| {
                let snapshot = homes_state
                    .current_home()
                    .map(|home| NeighborhoodHomeMeta {
                        member_count: home.members.len(),
                        moderator_actions_enabled: home.is_admin(),
                    })
                    .unwrap_or_default();
                if let Ok(mut guard) = meta.write() {
                    *guard = snapshot;
                }
            })
            .await;
        }
    });

    shared_meta
}

/// Shared pending recovery requests.
///
/// Used to map selected request index -> request ID for approvals.
pub type SharedPendingRequests = Arc<RwLock<Vec<PendingRequest>>>;

/// Create a shared pending requests holder and subscribe it to RECOVERY_SIGNAL.
pub fn use_pending_requests_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
) -> SharedPendingRequests {
    let shared_requests_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_requests: SharedPendingRequests = shared_requests_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let requests = shared_requests.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*RECOVERY_SIGNAL, move |r| {
                let pending: Vec<PendingRequest> = r
                    .pending_requests()
                    .iter()
                    .map(PendingRequest::from)
                    .collect();
                if let Ok(mut guard) = requests.write() {
                    *guard = pending;
                }
            })
            .await;
        }
    });

    shared_requests
}

/// Subscribe to notifications-related signals and emit count updates.
pub fn use_notifications_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    update_tx: Option<UiUpdateSender>,
) {
    let invite_count = Arc::new(AtomicUsize::new(0));
    let recovery_count = Arc::new(AtomicUsize::new(0));
    let last_total = Arc::new(AtomicUsize::new(usize::MAX));

    let send_total = |tx: &Option<UiUpdateSender>,
                      invites: &Arc<AtomicUsize>,
                      recovery: &Arc<AtomicUsize>,
                      last_total: &Arc<AtomicUsize>| {
        if let Some(ref tx) = tx {
            let total = invites.load(Ordering::Relaxed) + recovery.load(Ordering::Relaxed);
            let previous = last_total.swap(total, Ordering::Relaxed);
            if previous != total {
                publish_ui_update(tx, UiUpdate::NotificationsCountChanged(total));
            }
        }
    };

    // Invitations
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let invite_count = invite_count.clone();
        let recovery_count = recovery_count.clone();
        let last_total = last_total.clone();
        let update_tx = update_tx.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*INVITATIONS_SIGNAL, move |state| {
                invite_count.store(state.pending_received_count(), Ordering::Relaxed);
                send_total(&update_tx, &invite_count, &recovery_count, &last_total);
            })
            .await;
        }
    });

    // Recovery requests
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*RECOVERY_SIGNAL, move |state| {
                recovery_count.store(state.pending_requests().len(), Ordering::Relaxed);
                send_total(&update_tx, &invite_count, &recovery_count, &last_total);
            })
            .await;
        }
    });
}

/// Shared threshold settings.
///
/// Tuple of (threshold_k, threshold_n) for recovery threshold configuration.
/// Used to populate the threshold modal with current values.
pub type SharedThreshold = Arc<RwLock<(u8, u8)>>;

/// Create a shared threshold holder and subscribe it to SETTINGS_SIGNAL.
///
/// Returns an Arc that closures can capture. The subscription updates the Arc's
/// contents whenever settings change, so readers always get current threshold.
pub fn use_threshold_subscription(hooks: &mut Hooks, app_ctx: &AppCoreContext) -> SharedThreshold {
    let shared_threshold_ref = hooks.use_ref(|| Arc::new(RwLock::new((2u8, 3u8))));
    let shared_threshold: SharedThreshold = shared_threshold_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let threshold = shared_threshold.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*SETTINGS_SIGNAL, move |settings_state| {
                if let Ok(mut guard) = threshold.write() {
                    *guard = (settings_state.threshold_k, settings_state.threshold_n);
                }
            })
            .await;
        }
    });

    shared_threshold
}

#[cfg(test)]
mod tests {
    use super::scoped_channel_snapshot;
    use aura_app::ui::types::{
        Channel as AppChannel, ChannelType, ChatState, Message, MessageDeliveryStatus,
    };
    use aura_core::crypto::hash::hash;
    use aura_core::types::identifiers::{AuthorityId, ChannelId};
    use std::path::Path;

    fn test_channel_id(seed: &str) -> ChannelId {
        ChannelId::from_bytes(hash(seed.as_bytes()))
    }

    fn test_channel(id: ChannelId, name: &str) -> AppChannel {
        AppChannel {
            id,
            context_id: None,
            name: name.to_string(),
            topic: None,
            channel_type: ChannelType::Home,
            unread_count: 0,
            is_dm: false,
            member_ids: Vec::new(),
            member_count: 1,
            last_message: None,
            last_message_time: None,
            last_activity: 0,
            last_finalized_epoch: 0,
        }
    }

    fn merge_dm_like_channels(incoming: &ChatState, previous: &ChatState) -> ChatState {
        let mut merged = incoming.clone();
        for channel in previous.all_channels() {
            if crate::tui::chat_scope::is_dm_like_channel(channel)
                && !merged.has_channel(&channel.id)
            {
                merged.add_channel(channel.clone());
                for message in previous.messages_for_channel(&channel.id) {
                    merged.apply_message(channel.id, message.clone());
                }
            }
        }
        merged
    }

    fn test_dm_channel(id: ChannelId, name: &str) -> AppChannel {
        AppChannel {
            id,
            context_id: None,
            name: name.to_string(),
            topic: None,
            channel_type: ChannelType::DirectMessage,
            unread_count: 0,
            is_dm: true,
            member_ids: Vec::new(),
            member_count: 2,
            last_message: None,
            last_message_time: None,
            last_activity: 0,
            last_finalized_epoch: 0,
        }
    }

    fn test_dm_like_channel(id: ChannelId, name: &str) -> AppChannel {
        AppChannel {
            id,
            context_id: None,
            name: name.to_string(),
            topic: Some("Direct messages with peer".to_string()),
            channel_type: ChannelType::Home,
            unread_count: 0,
            is_dm: false,
            member_ids: Vec::new(),
            member_count: 2,
            last_message: None,
            last_message_time: None,
            last_activity: 0,
            last_finalized_epoch: 0,
        }
    }

    fn test_message(channel_id: ChannelId, id: &str, timestamp: u64) -> Message {
        Message {
            id: id.to_string(),
            channel_id,
            sender_id: AuthorityId::new_from_entropy([3u8; 32]),
            sender_name: "tester".to_string(),
            content: "hello".to_string(),
            timestamp,
            reply_to: None,
            is_own: false,
            is_read: false,
            delivery_status: MessageDeliveryStatus::Sent,
            epoch_hint: None,
            is_finalized: false,
        }
    }

    #[test]
    fn message_subscription_requires_explicit_selected_channel_identity() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source_path =
            repo_root.join("crates/aura-terminal/src/tui/screens/app/subscriptions.rs");
        let source = std::fs::read_to_string(&source_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", source_path.display()));

        assert!(source.contains("selected_channel_id: Arc<RwLock<Option<String>>>"));
        assert!(!source.contains("all_channels()\n                        .next()"));
        assert!(!source.contains("guard.get(selected_idx)"));
    }

    #[test]
    fn scoped_snapshot_returns_all_channels_without_scope() {
        let home_a = test_channel_id("home-a");
        let home_b = test_channel_id("home-b");
        let mut state = ChatState::from_channels([
            test_channel(home_a, "Home A"),
            test_channel(home_b, "Home B"),
        ]);

        state.apply_message(home_a, test_message(home_a, "m1", 1));
        state.apply_message(home_b, test_message(home_b, "m2", 2));
        state.apply_message(home_b, test_message(home_b, "m3", 3));

        let (channels, message_count) = scoped_channel_snapshot(&state, None);
        assert_eq!(channels.len(), 2);
        assert_eq!(message_count, 3);
    }

    #[test]
    fn merge_transient_channels_does_not_preserve_selected_shared_channel() {
        let previous_channel = test_channel(test_channel_id("shared"), "Shared");
        let previous = ChatState::from_channels([previous_channel.clone()]);
        let incoming = ChatState::default();

        let merged = super::merge_transient_channels(
            &incoming,
            &previous,
            Some(previous_channel.id.to_string().as_str()),
        );

        assert_eq!(merged.channel_count(), 0);
    }

    #[test]
    fn merge_transient_channels_preserves_selected_dm_like_channel() {
        let previous_channel = test_dm_channel(test_channel_id("dm"), "dm:peer");
        let previous = ChatState::from_channels([previous_channel.clone()]);
        let incoming = ChatState::default();

        let merged = super::merge_transient_channels(
            &incoming,
            &previous,
            Some(previous_channel.id.to_string().as_str()),
        );

        assert_eq!(merged.channel_count(), 1);
        assert!(merged.has_channel(&previous_channel.id));
    }

    #[test]
    fn scoped_snapshot_filters_to_active_home_channel() {
        let home_a = test_channel_id("home-a");
        let home_b = test_channel_id("home-b");
        let mut state = ChatState::from_channels([
            test_channel(home_a, "Home A"),
            test_channel(home_b, "Home B"),
        ]);

        state.apply_message(home_a, test_message(home_a, "m1", 1));
        state.apply_message(home_b, test_message(home_b, "m2", 2));
        state.apply_message(home_b, test_message(home_b, "m3", 3));

        let scope = home_b.to_string();
        let (channels, message_count) = scoped_channel_snapshot(&state, Some(scope.as_str()));
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].id, home_b.to_string());
        assert_eq!(message_count, 2);
    }

    #[test]
    fn scoped_snapshot_keeps_dm_channels_visible_across_scopes() {
        let home_a = test_channel_id("home-a");
        let home_b = test_channel_id("home-b");
        let dm = test_channel_id("dm-contact");
        let mut state = ChatState::from_channels([
            test_channel(home_a, "Home A"),
            test_channel(home_b, "Home B"),
            test_dm_channel(dm, "DM"),
        ]);

        state.apply_message(home_a, test_message(home_a, "m1", 1));
        state.apply_message(home_b, test_message(home_b, "m2", 2));
        state.apply_message(dm, test_message(dm, "m3", 3));

        let scope = home_b.to_string();
        let (channels, message_count) = scoped_channel_snapshot(&state, Some(scope.as_str()));
        assert_eq!(channels.len(), 2);
        assert!(channels.iter().any(|c| c.id == home_b.to_string()));
        assert!(channels.iter().any(|c| c.id == dm.to_string()));
        assert_eq!(message_count, 2);
    }

    #[test]
    fn scoped_snapshot_keeps_dm_like_channels_visible_across_scopes() {
        let home_a = test_channel_id("home-a");
        let home_b = test_channel_id("home-b");
        let dm_like = test_channel_id("dm-like-contact");
        let mut state = ChatState::from_channels([
            test_channel(home_a, "Home A"),
            test_channel(home_b, "Home B"),
            test_dm_like_channel(dm_like, "DM: Contact"),
        ]);

        state.apply_message(home_b, test_message(home_b, "m1", 1));
        state.apply_message(dm_like, test_message(dm_like, "m2", 2));

        let scope = home_b.to_string();
        let (channels, message_count) = scoped_channel_snapshot(&state, Some(scope.as_str()));
        assert_eq!(channels.len(), 2);
        assert!(channels.iter().any(|c| c.id == home_b.to_string()));
        assert!(channels.iter().any(|c| c.id == dm_like.to_string()));
        assert_eq!(message_count, 2);
    }

    #[test]
    fn merge_preserves_dm_like_channels_from_previous_state() {
        let dm_like = test_channel_id("dm-like-contact");

        let mut previous = ChatState::from_channels([test_dm_like_channel(dm_like, "DM: Contact")]);
        previous.apply_message(dm_like, test_message(dm_like, "m1", 1));

        let incoming = ChatState::default();
        let merged = merge_dm_like_channels(&incoming, &previous);

        assert!(merged.has_channel(&dm_like));
        assert_eq!(merged.messages_for_channel(&dm_like).len(), 1);
    }
}
