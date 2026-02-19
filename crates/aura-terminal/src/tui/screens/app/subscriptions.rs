//! iocraft hook helpers for long-lived reactive subscriptions.
//!
//! Keep shell.rs focused on wiring and rendering by extracting the
//! signal-subscription use_future homes here.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use iocraft::prelude::*;

use aura_app::ui::signals::{
    ConnectionStatus, DiscoveredPeer, DiscoveredPeerMethod, NetworkStatus, CHAT_SIGNAL,
    CONNECTION_STATUS_SIGNAL, CONTACTS_SIGNAL, DISCOVERED_PEERS_SIGNAL, INVITATIONS_SIGNAL,
    NEIGHBORHOOD_SIGNAL, NETWORK_STATUS_SIGNAL, RECOVERY_SIGNAL, SETTINGS_SIGNAL,
    TRANSPORT_PEERS_SIGNAL,
};
use aura_app::ui::types::ChatState;

use crate::tui::chat_scope::{active_home_scope_id, channel_matches_scope};
use crate::tui::hooks::{subscribe_signal_with_retry, AppCoreContext};
use crate::tui::types::{Channel, Contact, Device, Invitation, Message, PendingRequest};
use crate::tui::updates::{UiUpdate, UiUpdateSender};

/// Shared authority id state for UI dispatch handlers.
#[derive(Clone, Default)]
pub struct SharedAuthorityId(Arc<RwLock<String>>);

impl SharedAuthorityId {
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(String::new())))
    }

    pub fn read(&self) -> std::sync::LockResult<std::sync::RwLockReadGuard<'_, String>> {
        self.0.read()
    }

    pub fn write(&self) -> std::sync::LockResult<std::sync::RwLockWriteGuard<'_, String>> {
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
                    *guard = settings_state.authority_id;
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
                network_status.set(status);
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
                known_online.set(count);
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
                transport_peers.set(count);
            })
            .await;
        }
    });

    // Keep a best-effort physical clock for relative-time UI formatting.
    // This must come from the runtime/effects system (not OS clock).
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut now_ms = now_ms.clone();
        async move {
            loop {
                let runtime = app_core.raw().read().await.runtime().cloned();
                if let Some(runtime) = runtime {
                    if let Ok(ts) = runtime.current_time_ms().await {
                        now_ms.set(Some(ts));
                    }
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
                    let _ = tx.try_send(UiUpdate::LanPeersCountChanged(new_count));
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
                    let _ = tx.try_send(UiUpdate::ContactCountChanged(new_count));
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
pub fn use_devices_subscription(hooks: &mut Hooks, app_ctx: &AppCoreContext) -> SharedDevices {
    let shared_devices_ref = hooks.use_ref(SharedDevices::new);
    let shared_devices: SharedDevices = shared_devices_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let devices = shared_devices.clone();
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
    shared_channels: SharedChannels,
    tui_selected: Arc<RwLock<usize>>,
) -> SharedMessages {
    // Create the shared messages holder - use_ref ensures it persists across renders.
    let shared_messages_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_messages: SharedMessages = shared_messages_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let messages = shared_messages.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CHAT_SIGNAL, move |chat_state| {
                // Get the selected channel from TUI state
                let selected_idx = tui_selected.read().map(|g| *g).unwrap_or(0);

                // Look up the channel ID from shared channels
                let channel_id = shared_channels
                    .read()
                    .ok()
                    .and_then(|guard| guard.get(selected_idx).map(|c| c.id.clone()));

                // Get messages for that channel (or empty if none selected)
                let message_list: Vec<Message> = if let Some(channel_id) = channel_id {
                    if let Ok(cid) = channel_id.parse::<aura_core::identifiers::ChannelId>() {
                        chat_state
                            .messages_for_channel(&cid)
                            .iter()
                            .map(Message::from)
                            .collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    // Fallback: get messages for first channel if available
                    chat_state
                        .all_channels()
                        .next()
                        .map(|c| {
                            chat_state
                                .messages_for_channel(&c.id)
                                .iter()
                                .map(Message::from)
                                .collect()
                        })
                        .unwrap_or_default()
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

fn scoped_channel_snapshot(
    chat_state: &ChatState,
    active_scope: Option<&str>,
) -> (Vec<Channel>, usize) {
    let mut channels = Vec::new();
    let mut message_count = 0usize;

    for channel in chat_state.all_channels() {
        let channel_id = channel.id.to_string();
        if !channel_matches_scope(&channel_id, active_scope) {
            continue;
        }
        message_count += chat_state.messages_for_channel(&channel.id).len();
        channels.push(Channel::from(channel));
    }

    (channels, message_count)
}

fn publish_scoped_channels(
    channels: &SharedChannels,
    update_tx: &Option<UiUpdateSender>,
    chat_state: &ChatState,
    active_scope: Option<&str>,
) {
    let (channel_list, message_count) = scoped_channel_snapshot(chat_state, active_scope);
    let channel_count = channel_list.len();

    if let Ok(mut guard) = channels.write() {
        *guard = channel_list;
    }

    if let Some(tx) = update_tx {
        let _ = tx.try_send(UiUpdate::ChatStateUpdated {
            channel_count,
            message_count,
            selected_index: Some(0),
        });
    }
}

/// Create a shared channels holder and subscribe it to CHAT_SIGNAL.
pub fn use_channels_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    update_tx: Option<UiUpdateSender>,
) -> SharedChannels {
    let shared_channels_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_channels: SharedChannels = shared_channels_ref.read().clone();
    let active_scope_ref = hooks.use_ref(|| Arc::new(RwLock::new(None::<String>)));
    let active_scope: Arc<RwLock<Option<String>>> = active_scope_ref.read().clone();
    let latest_chat_state_ref = hooks.use_ref(|| Arc::new(RwLock::new(ChatState::default())));
    let latest_chat_state: Arc<RwLock<ChatState>> = latest_chat_state_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let channels = shared_channels.clone();
        let active_scope = active_scope.clone();
        let latest_chat_state = latest_chat_state.clone();
        let update_tx = update_tx.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CHAT_SIGNAL, move |chat_state| {
                if let Ok(mut guard) = latest_chat_state.write() {
                    *guard = chat_state.clone();
                }

                let scope = active_scope.read().ok().and_then(|g| g.clone());
                publish_scoped_channels(&channels, &update_tx, &chat_state, scope.as_deref());
            })
            .await;
        }
    });

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let channels = shared_channels.clone();
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
                publish_scoped_channels(&channels, &update_tx, &chat_state, Some(scope.as_str()));
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

    let send_total =
        |tx: &Option<UiUpdateSender>, invites: &Arc<AtomicUsize>, recovery: &Arc<AtomicUsize>| {
            if let Some(ref tx) = tx {
                let total = invites.load(Ordering::Relaxed) + recovery.load(Ordering::Relaxed);
                let _ = tx.try_send(UiUpdate::NotificationsCountChanged(total));
            }
        };

    // Invitations
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let invite_count = invite_count.clone();
        let recovery_count = recovery_count.clone();
        let update_tx = update_tx.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*INVITATIONS_SIGNAL, move |state| {
                invite_count.store(state.pending_received_count(), Ordering::Relaxed);
                send_total(&update_tx, &invite_count, &recovery_count);
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
                send_total(&update_tx, &invite_count, &recovery_count);
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
    use aura_core::identifiers::{AuthorityId, ChannelId};

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
}
