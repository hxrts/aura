//! ViewState read-modify-write helpers for workflows.
//!
//! These helpers update both the ViewState (for futures-signals) and the
//! ReactiveHandler signals (for app-level subscriptions) to ensure consistent
//! state across both signal systems.

use std::sync::Arc;

use async_lock::RwLock;

use crate::signal_defs::{
    CHAT_SIGNAL, CHAT_SIGNAL_NAME, NEIGHBORHOOD_SIGNAL, NEIGHBORHOOD_SIGNAL_NAME, RECOVERY_SIGNAL,
    RECOVERY_SIGNAL_NAME,
};
use crate::views::{chat::ChatState, neighborhood::NeighborhoodState, recovery::RecoveryState};
use crate::workflows::signals::emit_signal;
use crate::AppCore;
use aura_core::AuraError;

/// Read-modify-write helper for chat state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. CHAT_SIGNAL (for ReactiveEffects subscribers)
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn with_chat_state<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut ChatState) -> T,
) -> Result<T, AuraError> {
    let (output, state) = {
        let mut core = app_core.write().await;
        let mut state = core.snapshot().chat;
        let output = update(&mut state);
        core.views_mut().set_chat(state.clone());
        (output, state)
    };

    // Also emit to CHAT_SIGNAL for ReactiveEffects subscribers
    emit_signal(app_core, &*CHAT_SIGNAL, state, CHAT_SIGNAL_NAME).await?;

    Ok(output)
}

/// Read-modify-write helper for recovery state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. RECOVERY_SIGNAL (for ReactiveEffects subscribers)
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn with_recovery_state<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut RecoveryState) -> T,
) -> Result<T, AuraError> {
    let (output, state) = {
        let mut core = app_core.write().await;
        let mut state = core.snapshot().recovery;
        let output = update(&mut state);
        core.views_mut().set_recovery(state.clone());
        (output, state)
    };

    // Also emit to RECOVERY_SIGNAL for ReactiveEffects subscribers
    emit_signal(app_core, &*RECOVERY_SIGNAL, state, RECOVERY_SIGNAL_NAME).await?;

    Ok(output)
}

/// Read-modify-write helper for neighborhood state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. NEIGHBORHOOD_SIGNAL (for ReactiveEffects subscribers)
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn with_neighborhood_state<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut NeighborhoodState) -> T,
) -> Result<T, AuraError> {
    let (output, state) = {
        let mut core = app_core.write().await;
        let mut state = core.snapshot().neighborhood;
        let output = update(&mut state);
        core.views_mut().set_neighborhood(state.clone());
        (output, state)
    };

    // Also emit to NEIGHBORHOOD_SIGNAL for ReactiveEffects subscribers
    emit_signal(
        app_core,
        &*NEIGHBORHOOD_SIGNAL,
        state,
        NEIGHBORHOOD_SIGNAL_NAME,
    )
    .await?;

    Ok(output)
}
