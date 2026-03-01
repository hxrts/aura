//! Snapshot policy helpers to avoid accidental heavy reads.

use std::sync::Arc;

use crate::signal_defs::{CHAT_SIGNAL, CHAT_SIGNAL_NAME, CONTACTS_SIGNAL, CONTACTS_SIGNAL_NAME};
use crate::workflows::signals::read_signal;
use crate::{AppCore, ChatState, ContactsState, RecoveryState, StateSnapshot};
use async_lock::RwLock;

/// Read a full snapshot. Prefer narrow helpers when possible.
pub async fn full_snapshot(app_core: &Arc<RwLock<AppCore>>) -> StateSnapshot {
    let core = app_core.read().await;
    core.snapshot()
}

/// Read chat state from a snapshot (narrow scope).
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn chat_snapshot(app_core: &Arc<RwLock<AppCore>>) -> ChatState {
    if let Ok(chat) = read_signal(app_core, &*CHAT_SIGNAL, CHAT_SIGNAL_NAME).await {
        return chat;
    }
    app_core.read().await.snapshot().chat
}

/// Read contacts state from a snapshot (narrow scope).
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn contacts_snapshot(app_core: &Arc<RwLock<AppCore>>) -> ContactsState {
    if let Ok(contacts) = read_signal(app_core, &*CONTACTS_SIGNAL, CONTACTS_SIGNAL_NAME).await {
        return contacts;
    }
    app_core.read().await.snapshot().contacts
}

/// Read recovery state from a snapshot (narrow scope).
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn recovery_snapshot(app_core: &Arc<RwLock<AppCore>>) -> RecoveryState {
    let core = app_core.read().await;
    core.snapshot().recovery
}
