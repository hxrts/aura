//! Shared helpers for manager-owned state.

use tokio::sync::RwLock;

use super::invariant::InvariantViolation;

/// Mutate state and run a debug-only validation hook.
#[allow(unused_variables)] // validate only used in debug builds
pub async fn with_state_mut_validated<State, F, V, R>(
    state: &RwLock<State>,
    mutator: F,
    validate: V,
) -> R
where
    F: FnOnce(&mut State) -> R,
    V: FnOnce(&State) -> Result<(), InvariantViolation>,
{
    let mut guard = state.write().await;
    let result = mutator(&mut guard);
    #[cfg(debug_assertions)]
    {
        if let Err(violation) = validate(&guard) {
            tracing::error!(
                component = violation.component,
                description = %violation.description,
                "State invariant violated"
            );
            debug_assert!(false, "State invariant violated: {}", violation);
        }
    }
    result
}
