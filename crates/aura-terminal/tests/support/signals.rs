//! Signal waiting and polling helpers for async tests.
//!
//! This module provides utilities for waiting on signal state changes
//! with configurable timeouts. All functions use a polling approach
//! suitable for async tests.
//!
//! # Example
//!
//! ```ignore
//! use crate::support::signals::{wait_for_chat, wait_for_contacts};
//!
//! // Wait for a specific chat state
//! let chat = wait_for_chat(&app_core, |s| !s.messages.is_empty()).await;
//!
//! // Wait for a contact to appear
//! wait_for_contact(&app_core, contact_id).await;
//! ```

use async_lock::RwLock;
use std::sync::Arc;
use std::time::Duration;

use aura_app::signal_defs::{
    SettingsState, CHAT_SIGNAL, CONTACTS_SIGNAL, ERROR_SIGNAL, INVITATIONS_SIGNAL,
    NEIGHBORHOOD_SIGNAL, RECOVERY_SIGNAL, SETTINGS_SIGNAL,
};
use aura_app::views::{
    ChatState, ContactsState, InvitationsState, NeighborhoodState, RecoveryState,
};
use aura_app::{AppCore, AppError};
use aura_core::effects::reactive::{ReactiveEffects, Signal};
use aura_core::identifiers::{AuthorityId, DeviceId};
use aura_terminal::ids;

/// Default timeout for signal waits.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(500);

/// Extended timeout for slower operations.
pub const EXTENDED_TIMEOUT: Duration = Duration::from_secs(2);

/// Polling interval between signal checks.
pub const POLL_INTERVAL: Duration = Duration::from_millis(10);

// ============================================================================
// Generic Wait Helpers
// ============================================================================

/// Wait for a signal to satisfy a predicate with custom timeout.
///
/// Panics if the predicate is not satisfied within the timeout.
pub async fn wait_for_signal<T, F>(
    app_core: &Arc<RwLock<AppCore>>,
    signal: &Signal<T>,
    mut predicate: F,
    timeout: Duration,
    description: &str,
) -> T
where
    T: Clone + Send + Sync + 'static,
    F: FnMut(&T) -> bool,
{
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        let state = {
            let core = app_core.read().await;
            core.read(signal).await.expect("Failed to read signal")
        };

        if predicate(&state) {
            return state;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("Timed out waiting for {description}");
        }

        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

// ============================================================================
// Error Signal Helpers
// ============================================================================

/// Read the current error signal (if any).
pub async fn read_error_signal(app_core: &Arc<RwLock<AppCore>>) -> Option<AppError> {
    let core = app_core.read().await;
    core.read(&*ERROR_SIGNAL).await.ok().flatten()
}

/// Wait for an error signal to become Some(AppError) within timeout.
pub async fn wait_for_error_signal(app_core: &Arc<RwLock<AppCore>>, timeout: Duration) -> AppError {
    wait_for_signal(
        app_core,
        &*ERROR_SIGNAL,
        |state| state.is_some(),
        timeout,
        "error signal",
    )
    .await
    .expect("Error signal should be Some(AppError)")
}

// ============================================================================
// Chat Signal Helpers
// ============================================================================

/// Wait for chat state to satisfy a predicate.
pub async fn wait_for_chat(
    app_core: &Arc<RwLock<AppCore>>,
    predicate: impl FnMut(&ChatState) -> bool,
) -> ChatState {
    wait_for_signal(
        app_core,
        &*CHAT_SIGNAL,
        predicate,
        DEFAULT_TIMEOUT,
        "chat state",
    )
    .await
}

/// Wait for chat state with extended timeout.
pub async fn wait_for_chat_extended(
    app_core: &Arc<RwLock<AppCore>>,
    predicate: impl FnMut(&ChatState) -> bool,
) -> ChatState {
    wait_for_signal(
        app_core,
        &*CHAT_SIGNAL,
        predicate,
        EXTENDED_TIMEOUT,
        "chat state",
    )
    .await
}

/// Wait for a message to appear in chat.
pub async fn wait_for_message(
    app_core: &Arc<RwLock<AppCore>>,
    predicate: impl Fn(&aura_app::views::Message) -> bool,
) -> ChatState {
    wait_for_chat(app_core, |state| state.messages.iter().any(&predicate)).await
}

// ============================================================================
// Contacts Signal Helpers
// ============================================================================

/// Wait for contacts state to satisfy a predicate.
pub async fn wait_for_contacts(
    app_core: &Arc<RwLock<AppCore>>,
    predicate: impl FnMut(&ContactsState) -> bool,
) -> ContactsState {
    wait_for_signal(
        app_core,
        &*CONTACTS_SIGNAL,
        predicate,
        DEFAULT_TIMEOUT,
        "contacts state",
    )
    .await
}

/// Wait for contacts state with extended timeout.
pub async fn wait_for_contacts_extended(
    app_core: &Arc<RwLock<AppCore>>,
    predicate: impl FnMut(&ContactsState) -> bool,
) -> ContactsState {
    wait_for_signal(
        app_core,
        &*CONTACTS_SIGNAL,
        predicate,
        EXTENDED_TIMEOUT,
        "contacts state",
    )
    .await
}

/// Wait for a specific contact to appear.
pub async fn wait_for_contact(app_core: &Arc<RwLock<AppCore>>, contact_id: AuthorityId) {
    wait_for_contacts_extended(app_core, |state| {
        state.contacts.iter().any(|c| c.id == contact_id)
    })
    .await;
}

/// Wait for specific contacts to appear (by authority IDs).
pub async fn wait_for_contacts_by_ids(app_core: &Arc<RwLock<AppCore>>, expected: &[AuthorityId]) {
    wait_for_contacts_extended(app_core, |state| {
        expected
            .iter()
            .all(|id| state.contacts.iter().any(|c| &c.id == id))
    })
    .await;
}

// ============================================================================
// Recovery Signal Helpers
// ============================================================================

/// Wait for recovery state to satisfy a predicate.
pub async fn wait_for_recovery(
    app_core: &Arc<RwLock<AppCore>>,
    predicate: impl FnMut(&RecoveryState) -> bool,
) -> RecoveryState {
    wait_for_signal(
        app_core,
        &*RECOVERY_SIGNAL,
        predicate,
        DEFAULT_TIMEOUT,
        "recovery state",
    )
    .await
}

/// Wait for recovery state with extended timeout.
pub async fn wait_for_recovery_extended(
    app_core: &Arc<RwLock<AppCore>>,
    predicate: impl FnMut(&RecoveryState) -> bool,
) -> RecoveryState {
    wait_for_signal(
        app_core,
        &*RECOVERY_SIGNAL,
        predicate,
        EXTENDED_TIMEOUT,
        "recovery state",
    )
    .await
}

// ============================================================================
// Devices Signal Helpers
// ============================================================================

/// Wait for devices state to satisfy a predicate.
/// Uses SETTINGS_SIGNAL since devices are part of SettingsState.
pub async fn wait_for_devices(
    app_core: &Arc<RwLock<AppCore>>,
    predicate: impl FnMut(&SettingsState) -> bool,
) -> SettingsState {
    wait_for_signal(
        app_core,
        &*SETTINGS_SIGNAL,
        predicate,
        DEFAULT_TIMEOUT,
        "devices state",
    )
    .await
}

/// Wait for a specific device to appear by device ID string.
pub async fn wait_for_device(app_core: &Arc<RwLock<AppCore>>, device_id: &str) {
    let device_id = ids::device_id(device_id);
    wait_for_signal(
        app_core,
        &*SETTINGS_SIGNAL,
        |state: &SettingsState| state.devices.iter().any(|d| d.id == device_id),
        EXTENDED_TIMEOUT,
        &format!("device '{device_id}'"),
    )
    .await;
}

/// Wait for a device to be absent by device ID string.
pub async fn wait_for_device_absent(app_core: &Arc<RwLock<AppCore>>, device_id: &str) {
    let device_id = ids::device_id(device_id);
    wait_for_signal(
        app_core,
        &*SETTINGS_SIGNAL,
        |state: &SettingsState| !state.devices.iter().any(|d| d.id == device_id),
        EXTENDED_TIMEOUT,
        &format!("device '{device_id}' to be removed"),
    )
    .await;
}

// ============================================================================
// Invitations Signal Helpers
// ============================================================================

/// Wait for invitations state to satisfy a predicate.
pub async fn wait_for_invitations(
    app_core: &Arc<RwLock<AppCore>>,
    predicate: impl FnMut(&InvitationsState) -> bool,
) -> InvitationsState {
    wait_for_signal(
        app_core,
        &*INVITATIONS_SIGNAL,
        predicate,
        DEFAULT_TIMEOUT,
        "invitations state",
    )
    .await
}

// ============================================================================
// Neighborhood Signal Helpers
// ============================================================================

/// Wait for neighborhood state to satisfy a predicate.
pub async fn wait_for_neighborhood(
    app_core: &Arc<RwLock<AppCore>>,
    predicate: impl FnMut(&NeighborhoodState) -> bool,
) -> NeighborhoodState {
    wait_for_signal(
        app_core,
        &*NEIGHBORHOOD_SIGNAL,
        predicate,
        DEFAULT_TIMEOUT,
        "neighborhood state",
    )
    .await
}

// ============================================================================
// Settings Signal Helpers
// ============================================================================

/// Wait for settings state to satisfy a predicate.
pub async fn wait_for_settings(
    app_core: &Arc<RwLock<AppCore>>,
    predicate: impl FnMut(&SettingsState) -> bool,
) -> SettingsState {
    wait_for_signal(
        app_core,
        &*SETTINGS_SIGNAL,
        predicate,
        DEFAULT_TIMEOUT,
        "settings state",
    )
    .await
}
