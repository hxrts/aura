//! iocraft hook helpers for long-lived reactive subscriptions.
//!
//! Keep `shell.rs` focused on wiring and rendering by extracting the
//! signal-subscription `use_future` blocks here.

use std::sync::{Arc, RwLock};

use iocraft::prelude::*;

use aura_app::signal_defs::{
    ConnectionStatus, SyncStatus, CONNECTION_STATUS_SIGNAL, CONTACTS_SIGNAL, SYNC_STATUS_SIGNAL,
};
use aura_core::effects::reactive::ReactiveEffects;

use crate::tui::hooks::AppCoreContext;
use crate::tui::types::{Contact, ContactStatus};

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
        async move {
            let mut stream = {
                let core = app_core.read().await;
                core.subscribe(&*SYNC_STATUS_SIGNAL)
            };
            while let Ok(status) = stream.recv().await {
                syncing.set(matches!(status, SyncStatus::Syncing { .. }));
            }
        }
    });

    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut peer_count = peer_count.clone();
        async move {
            let mut stream = {
                let core = app_core.read().await;
                core.subscribe(&*CONNECTION_STATUS_SIGNAL)
            };
            while let Ok(status) = stream.recv().await {
                let peers = match status {
                    ConnectionStatus::Online { peer_count } => peer_count,
                    _ => 0,
                };
                peer_count.set(peers);
            }
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
/// This uses `Arc<RwLock<Vec<Contact>>>` instead of `State<T>` because:
/// 1. Dispatch handler closures need to read current contacts at invocation time
/// 2. We don't want every contact change to trigger re-renders (screens handle that)
/// 3. The closure captures the Arc, not the data, so it always reads fresh data
pub type SharedContacts = Arc<RwLock<Vec<Contact>>>;

/// Create a shared contacts holder and subscribe it to CONTACTS_SIGNAL.
///
/// Returns an Arc that closures can capture. The subscription updates the Arc's
/// contents whenever contacts change, so readers always get current data.
///
/// Uses `std::sync::RwLock` so dispatch handlers can read synchronously.
pub fn use_contacts_subscription(hooks: &mut Hooks, app_ctx: &AppCoreContext) -> SharedContacts {
    // Create the shared contacts holder - use_ref ensures it persists across renders
    let shared_contacts_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_contacts: SharedContacts = shared_contacts_ref.read().clone();

    // Subscribe to CONTACTS_SIGNAL and update the shared contacts
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let contacts = shared_contacts.clone();
        async move {
            // Helper to convert app Contact to tui Contact
            fn convert_contact(c: &aura_app::views::Contact) -> Contact {
                Contact {
                    id: c.id.clone(),
                    nickname: c.nickname.clone(),
                    suggested_name: c.suggested_name.clone(),
                    // Map to Active status - the TUI ContactStatus doesn't have Online/Offline
                    status: ContactStatus::Active,
                    is_guardian: c.is_guardian,
                }
            }

            // Initial read
            {
                let core = app_core.read().await;
                if let Ok(contacts_state) = core.read(&*CONTACTS_SIGNAL).await {
                    let contact_list: Vec<Contact> = contacts_state
                        .contacts
                        .iter()
                        .map(convert_contact)
                        .collect();
                    if let Ok(mut guard) = contacts.write() {
                        *guard = contact_list;
                    }
                }
            }

            // Subscribe for updates
            let mut stream = {
                let core = app_core.read().await;
                core.subscribe(&*CONTACTS_SIGNAL)
            };

            while let Ok(contacts_state) = stream.recv().await {
                let contact_list: Vec<Contact> = contacts_state
                    .contacts
                    .iter()
                    .map(convert_contact)
                    .collect();
                if let Ok(mut guard) = contacts.write() {
                    *guard = contact_list;
                }
            }
        }
    });

    shared_contacts
}
