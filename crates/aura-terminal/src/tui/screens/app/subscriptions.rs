//! iocraft hook helpers for long-lived reactive subscriptions.
//!
//! Keep shell.rs focused on wiring and rendering by extracting the
//! signal-subscription use_future blocks here.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use iocraft::prelude::*;

use aura_app::signal_defs::{
    ConnectionStatus, SyncStatus, BLOCKS_SIGNAL, BLOCK_SIGNAL, CHAT_SIGNAL,
    CONNECTION_STATUS_SIGNAL, CONTACTS_SIGNAL, INVITATIONS_SIGNAL, NEIGHBORHOOD_SIGNAL,
    RECOVERY_SIGNAL, SYNC_STATUS_SIGNAL,
};
use aura_effects::time::PhysicalTimeHandler;

use crate::tui::hooks::{subscribe_signal_with_retry, AppCoreContext};
use crate::tui::types::{Channel, Contact, Invitation, Message, PendingRequest, Resident};

pub struct NavStatusSignals {
    pub syncing: State<bool>,
    pub peer_count: State<usize>,
    pub last_sync_time: State<Option<u64>>,
}

pub fn use_nav_status_signals(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    initial_syncing: bool,
    initial_peer_count: usize,
    initial_last_sync_time: Option<u64>,
) -> NavStatusSignals {
    let syncing = hooks.use_state(|| initial_syncing);
    let peer_count = hooks.use_state(|| initial_peer_count);
    let last_sync_time = hooks.use_state(|| initial_last_sync_time);

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut syncing = syncing.clone();
        let mut last_sync_time = last_sync_time.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*SYNC_STATUS_SIGNAL, move |status| {
                syncing.set(matches!(status, SyncStatus::Syncing { .. }));

                if matches!(status, SyncStatus::Synced) {
                    last_sync_time.set(Some(now_millis()));
                }
            })
            .await;
        }
    });

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut peer_count = peer_count.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CONNECTION_STATUS_SIGNAL, move |status| {
                let peers = match status {
                    ConnectionStatus::Online { peer_count } => peer_count,
                    _ => 0,
                };
                peer_count.set(peers);
            })
            .await;
        }
    });

    NavStatusSignals {
        syncing,
        peer_count,
        last_sync_time,
    }
}

/// Shared contacts state that can be read by closures without re-rendering.
///
/// This uses Arc<RwLock<Vec<Contact>>> instead of State<T> because:
/// 1. Dispatch handler closures need to read current contacts at invocation time.
/// 2. We do not want every contacts update to trigger shell re-renders.
/// 3. The closure captures the Arc, not the data, so it always reads fresh data.
pub type SharedContacts = Arc<RwLock<Vec<Contact>>>;

/// Create a shared contacts holder and subscribe it to CONTACTS_SIGNAL.
///
/// Returns an Arc that closures can capture. The subscription updates the Arc's
/// contents whenever contacts change, so readers always get current data.
///
/// Uses std::sync::RwLock so dispatch handlers can read synchronously.
pub fn use_contacts_subscription(hooks: &mut Hooks, app_ctx: &AppCoreContext) -> SharedContacts {
    // Create the shared contacts holder - use_ref ensures it persists across renders.
    let shared_contacts_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_contacts: SharedContacts = shared_contacts_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let contacts = shared_contacts.clone();
        async move {
            // CONNECTION_STATUS_SIGNAL depends on the current contacts list (peer count = online contacts).
            // Ensure the footer updates when CONTACTS_SIGNAL changes by refreshing the derived status.
            let refresh_in_flight = Arc::new(AtomicBool::new(false));
            let app_core_for_refresh = app_core.clone();

            subscribe_signal_with_retry(app_core, &*CONTACTS_SIGNAL, move |contacts_state| {
                let contact_list: Vec<Contact> =
                    contacts_state.contacts.iter().map(Contact::from).collect();
                if let Ok(mut guard) = contacts.write() {
                    *guard = contact_list;
                }

                // Avoid spawning an unbounded number of refresh tasks if contacts update rapidly.
                if refresh_in_flight.swap(true, Ordering::SeqCst) {
                    return;
                }

                let app_core_for_refresh = app_core_for_refresh.clone();
                let refresh_in_flight = refresh_in_flight.clone();
                tokio::spawn(async move {
                    let _ = aura_app::workflows::system::refresh_account(app_core_for_refresh.raw()).await;
                    refresh_in_flight.store(false, Ordering::SeqCst);
                });
            })
            .await;
        }
    });

    shared_contacts
}

/// Shared residents state (current block) that can be read by closures without re-rendering.
///
/// This is used for block actions that operate on the currently selected resident.
pub type SharedResidents = Arc<RwLock<Vec<Resident>>>;

/// Create a shared residents holder and subscribe it to BLOCKS_SIGNAL (preferred) and BLOCK_SIGNAL (fallback).
///
/// Priority: if BLOCKS_SIGNAL has a current block, it wins; otherwise BLOCK_SIGNAL is used.
pub fn use_residents_subscription(hooks: &mut Hooks, app_ctx: &AppCoreContext) -> SharedResidents {
    let shared_residents_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_residents: SharedResidents = shared_residents_ref.read().clone();

    let has_blocks_current_ref = hooks.use_ref(|| Arc::new(AtomicBool::new(false)));
    let has_blocks_current = has_blocks_current_ref.read().clone();

    // Preferred: multi-block state.
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let residents = shared_residents.clone();
        let has_blocks_current = has_blocks_current.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*BLOCKS_SIGNAL, move |blocks_state| {
                if let Some(block) = blocks_state.current_block() {
                    has_blocks_current.store(true, Ordering::Relaxed);
                    let list: Vec<Resident> = block.residents.iter().map(Resident::from).collect();
                    if let Ok(mut guard) = residents.write() {
                        *guard = list;
                    }
                } else {
                    has_blocks_current.store(false, Ordering::Relaxed);
                    if let Ok(mut guard) = residents.write() {
                        guard.clear();
                    }
                }
            })
            .await;
        }
    });

    // Fallback: legacy singular block state.
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let residents = shared_residents.clone();
        let has_blocks_current = has_blocks_current.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*BLOCK_SIGNAL, move |block_state| {
                if has_blocks_current.load(Ordering::Relaxed) {
                    return;
                }

                let list: Vec<Resident> =
                    block_state.residents.iter().map(Resident::from).collect();
                if let Ok(mut guard) = residents.write() {
                    *guard = list;
                }
            })
            .await;
        }
    });

    shared_residents
}

fn now_millis() -> u64 {
    PhysicalTimeHandler::new().physical_time_now_ms()
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
pub fn use_messages_subscription(hooks: &mut Hooks, app_ctx: &AppCoreContext) -> SharedMessages {
    // Create the shared messages holder - use_ref ensures it persists across renders.
    let shared_messages_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_messages: SharedMessages = shared_messages_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let messages = shared_messages.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CHAT_SIGNAL, move |chat_state| {
                let message_list: Vec<Message> =
                    chat_state.messages.iter().map(Message::from).collect();
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

/// Create a shared channels holder and subscribe it to CHAT_SIGNAL.
pub fn use_channels_subscription(hooks: &mut Hooks, app_ctx: &AppCoreContext) -> SharedChannels {
    let shared_channels_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_channels: SharedChannels = shared_channels_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let channels = shared_channels.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CHAT_SIGNAL, move |chat_state| {
                let channel_list: Vec<Channel> =
                    chat_state.channels.iter().map(Channel::from).collect();
                if let Ok(mut guard) = channels.write() {
                    *guard = channel_list;
                }
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
                    .pending
                    .iter()
                    .chain(inv_state.sent.iter())
                    .chain(inv_state.history.iter())
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

/// Shared neighborhood block IDs (in display order).
///
/// Used to map neighborhood grid index -> block ID for EnterBlock.
pub type SharedNeighborhoodBlocks = Arc<RwLock<Vec<String>>>;

/// Create a shared neighborhood blocks holder and subscribe it to NEIGHBORHOOD_SIGNAL.
pub fn use_neighborhood_blocks_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
) -> SharedNeighborhoodBlocks {
    let shared_blocks_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_blocks: SharedNeighborhoodBlocks = shared_blocks_ref.read().clone();

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let blocks = shared_blocks.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*NEIGHBORHOOD_SIGNAL, move |n| {
                let ids: Vec<String> = n.neighbors.iter().map(|b| b.id.to_string()).collect();
                if let Ok(mut guard) = blocks.write() {
                    *guard = ids;
                }
            })
            .await;
        }
    });

    shared_blocks
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
                    .pending_requests
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
