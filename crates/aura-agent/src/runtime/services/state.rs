//! Shared helpers for manager-owned state.

use std::sync::Arc;
use tokio::sync::RwLock;

/// Mutate state behind an async RwLock.
pub async fn with_state_mut<State, F, R>(state: &Arc<RwLock<State>>, mutator: F) -> R
where
    F: FnOnce(&mut State) -> R,
{
    let mut guard = state.write().await;
    mutator(&mut guard)
}

/// Mutate state and run a debug-only validation hook.
pub async fn with_state_mut_validated<State, F, V, R>(
    state: &Arc<RwLock<State>>,
    mutator: F,
    validate: V,
) -> R
where
    F: FnOnce(&mut State) -> R,
    V: FnOnce(&State) -> Result<(), String>,
{
    let mut guard = state.write().await;
    let result = mutator(&mut guard);
    #[cfg(debug_assertions)]
    {
        if let Err(message) = validate(&guard) {
            tracing::error!(%message, "State invariant violated");
            debug_assert!(false, "State invariant violated: {}", message);
        }
    }
    result
}
