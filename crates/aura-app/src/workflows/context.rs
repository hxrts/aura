//! Context Workflow - Portable Business Logic
//!
//! This module contains context/navigation operations that are portable across all frontends.
//! It follows the reactive signal pattern and manages neighborhood navigation state.

use crate::{
    views::neighborhood::{NeighborhoodState, TraversalPosition},
    AppCore,
};
use async_lock::RwLock;
use aura_core::{identifiers::ChannelId, identifiers::ContextId, AuraError, EffectContext};
use std::sync::Arc;

/// Set active context for navigation and command targeting
///
/// **What it does**: Sets the active context ID
/// **Returns**: Optional context ID
/// **Signal pattern**: Read-only operation (no signal emission)
///
/// The actual state update is handled by the UI layer when it receives
/// the context change notification.
pub async fn set_context(
    _app_core: &Arc<RwLock<AppCore>>,
    context_id: Option<String>,
) -> Result<Option<String>, AuraError> {
    // Context switching is handled by UI layer
    // This workflow just validates and returns the new context
    Ok(context_id)
}

/// Move position in neighborhood view
///
/// **What it does**: Updates neighborhood traversal position
/// **Returns**: Unit result
/// **Signal pattern**: Updates neighborhood view state directly
///
/// This operation:
/// 1. Determines target home (home, current, or specific ID)
/// 2. Resolves home name from neighbor list
/// 3. Creates/updates TraversalPosition
/// 4. Updates neighborhood view state
///
/// Depth values:
/// - 0: Street level
/// - 1: Frontage level (default)
/// - 2: Interior level
pub async fn move_position(
    app_core: &Arc<RwLock<AppCore>>,
    home_id: &str,
    depth: &str,
) -> Result<(), AuraError> {
    // Parse the depth string to determine traversal depth
    let depth_value = match depth.to_lowercase().as_str() {
        "street" => 0,
        "frontage" => 1,
        "interior" => 2,
        _ => 1, // Default to frontage
    };

    let mut core = app_core.write().await;

    // Get current neighborhood state
    let mut neighborhood = core.views().get_neighborhood();

    // Determine target home ID
    let target_home_id = if home_id == "home" {
        neighborhood.home_home_id
    } else if home_id == "current" {
        // Stay on current home, just change depth
        neighborhood
            .position
            .as_ref()
            .map(|p| p.current_home_id)
            .unwrap_or(neighborhood.home_home_id)
    } else {
        // Parse home_id as ChannelId, fall back to home if invalid
        home_id
            .parse::<ChannelId>()
            .unwrap_or(neighborhood.home_home_id)
    };

    // Get home name from neighbors or use the ID
    let home_name = neighborhood
        .neighbor(&target_home_id)
        .map(|n| n.name.clone())
        .unwrap_or_else(|| {
            // Check if it's home
            if target_home_id == neighborhood.home_home_id {
                neighborhood.home_name.clone()
            } else {
                target_home_id.to_string()
            }
        });

    // Create or update position
    let position = TraversalPosition {
        current_home_id: target_home_id,
        current_home_name: home_name,
        depth: depth_value,
        path: vec![target_home_id],
    };
    neighborhood.position = Some(position);

    // Set the updated state
    core.views_mut().set_neighborhood(neighborhood);

    Ok(())
}

/// Get current neighborhood state
///
/// **What it does**: Reads neighborhood state from views
/// **Returns**: Current neighborhood state
/// **Signal pattern**: Read-only operation (no emission)
pub async fn get_neighborhood_state(app_core: &Arc<RwLock<AppCore>>) -> NeighborhoodState {
    let core = app_core.read().await;
    core.views().get_neighborhood()
}

/// Get current home context id with a deterministic fallback.
pub async fn current_home_context_or_fallback(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<ContextId, AuraError> {
    let core = app_core.read().await;
    let homes = core.views().get_homes();
    if let Some(home_state) = homes.current_home() {
        if let Some(ctx_id) = home_state.context_id {
            return Ok(ctx_id);
        }
    }

    // Fallback: when no home is selected yet (common in demos/tests), use a
    // deterministic per-authority context id so messaging can still function.
    if let Some(runtime) = core.runtime() {
        return Ok(EffectContext::with_authority(runtime.authority_id()).context_id());
    }

    Err(AuraError::not_found("No current home selected"))
}

/// Stable fallback context for relational facts that should not depend on UI selection.
pub fn default_relational_context() -> ContextId {
    ContextId::new_from_entropy([2u8; 32])
}

/// Get current traversal position
///
/// **What it does**: Reads current position from neighborhood state
/// **Returns**: Optional traversal position
/// **Signal pattern**: Read-only operation (no emission)
pub async fn get_current_position(app_core: &Arc<RwLock<AppCore>>) -> Option<TraversalPosition> {
    let core = app_core.read().await;
    let neighborhood = core.views().get_neighborhood();
    neighborhood.position
}

/// Initialize HOMES_SIGNAL with a default test home.
///
/// This is a test helper that creates a home and populates HOMES_SIGNAL
/// so that tests have a valid current home available.
///
/// **Signal pattern**: Emits HOMES_SIGNAL
pub async fn initialize_test_home(
    app_core: &Arc<RwLock<AppCore>>,
    name: &str,
    authority_id: aura_core::identifiers::AuthorityId,
    timestamp_ms: u64,
) -> Result<ChannelId, AuraError> {
    use crate::signal_defs::{HOMES_SIGNAL, HOMES_SIGNAL_NAME};
    use crate::views::home::HomeState;
    use crate::workflows::signals::emit_signal;
    use aura_core::crypto::hash::hash;

    // Create a deterministic home ID from the name
    let home_id = ChannelId::from_bytes(hash(format!("test-home:{name}").as_bytes()));

    // Create a context ID for the home
    let context_id = ContextId::new_from_entropy(hash(format!("test-context:{name}").as_bytes()));

    // Create the home state
    let home_state = HomeState::new(home_id, Some(name.to_string()), authority_id, timestamp_ms, context_id);

    // Add to HOMES_SIGNAL
    let homes = {
        let mut core = app_core.write().await;
        let mut homes = core.views().get_homes();
        homes.add_home(home_state);
        core.views_mut().set_homes(homes.clone());
        homes
    };

    // Emit to ReactiveEffects subscribers
    emit_signal(app_core, &*HOMES_SIGNAL, homes, HOMES_SIGNAL_NAME).await?;

    Ok(home_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[tokio::test]
    async fn test_set_context() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let result = set_context(&app_core, Some("context-123".to_string())).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("context-123".to_string()));

        let result = set_context(&app_core, None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    #[tokio::test]
    async fn test_get_neighborhood_state() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let state = get_neighborhood_state(&app_core).await;
        assert!(state.neighbors.is_empty());
    }
}
