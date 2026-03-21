//! Observed-only snapshot helpers to avoid accidental heavy reads in workflow
//! code.

use std::sync::Arc;

use crate::{AppCore, ChatState, ContactsState, RecoveryState};
use async_lock::RwLock;

/// Read chat state from a snapshot (narrow scope).
/// OWNERSHIP: observed
#[aura_macros::observed_only]
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn observed_chat_snapshot(app_core: &Arc<RwLock<AppCore>>) -> ChatState {
    app_core.read().await.snapshot().chat
}

/// Read contacts state from a snapshot (narrow scope).
/// OWNERSHIP: observed
#[aura_macros::observed_only]
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn observed_contacts_snapshot(app_core: &Arc<RwLock<AppCore>>) -> ContactsState {
    app_core.read().await.snapshot().contacts
}

/// Read recovery state from a snapshot (narrow scope).
/// OWNERSHIP: observed
#[aura_macros::observed_only]
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn observed_recovery_snapshot(app_core: &Arc<RwLock<AppCore>>) -> RecoveryState {
    let core = app_core.read().await;
    core.snapshot().recovery
}
