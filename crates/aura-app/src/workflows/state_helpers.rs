//! ViewState read-modify-write helpers for workflows.

use std::sync::Arc;

use async_lock::RwLock;

use crate::views::{chat::ChatState, neighborhood::NeighborhoodState, recovery::RecoveryState};
use crate::AppCore;
use aura_core::AuraError;

/// Read-modify-write helper for chat state.
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn with_chat_state<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut ChatState) -> T,
) -> Result<T, AuraError> {
    let mut core = app_core.write().await;
    let mut state = core.snapshot().chat;
    let output = update(&mut state);
    core.views_mut().set_chat(state);
    Ok(output)
}

/// Read-modify-write helper for recovery state.
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn with_recovery_state<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut RecoveryState) -> T,
) -> Result<T, AuraError> {
    let mut core = app_core.write().await;
    let mut state = core.snapshot().recovery;
    let output = update(&mut state);
    core.views_mut().set_recovery(state);
    Ok(output)
}

/// Read-modify-write helper for neighborhood state.
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn with_neighborhood_state<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut NeighborhoodState) -> T,
) -> Result<T, AuraError> {
    let mut core = app_core.write().await;
    let mut state = core.snapshot().neighborhood;
    let output = update(&mut state);
    core.views_mut().set_neighborhood(state);
    Ok(output)
}
