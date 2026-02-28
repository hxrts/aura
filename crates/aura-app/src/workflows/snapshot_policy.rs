//! Snapshot policy helpers to avoid accidental heavy reads.

use std::sync::Arc;

use async_lock::RwLock;
use aura_core::effects::reactive::ReactiveEffects;

use crate::signal_defs::{CHAT_SIGNAL, CONTACTS_SIGNAL};
use crate::{AppCore, ChatState, ContactsState, RecoveryState, StateSnapshot};

/// Read a full snapshot. Prefer narrow helpers when possible.
pub async fn full_snapshot(app_core: &Arc<RwLock<AppCore>>) -> StateSnapshot {
    let core = app_core.read().await;
    core.snapshot()
}

/// Read chat state from a snapshot (narrow scope).
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn chat_snapshot(app_core: &Arc<RwLock<AppCore>>) -> ChatState {
    let core = app_core.read().await;
    core.read(&*CHAT_SIGNAL)
        .await
        .unwrap_or_else(|_| core.snapshot().chat)
}

/// Read contacts state from a snapshot (narrow scope).
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn contacts_snapshot(app_core: &Arc<RwLock<AppCore>>) -> ContactsState {
    let core = app_core.read().await;
    core.read(&*CONTACTS_SIGNAL)
        .await
        .unwrap_or_else(|_| core.snapshot().contacts)
}

/// Read recovery state from a snapshot (narrow scope).
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn recovery_snapshot(app_core: &Arc<RwLock<AppCore>>) -> RecoveryState {
    let core = app_core.read().await;
    core.snapshot().recovery
}
