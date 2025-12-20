//! Context Workflow - Portable Business Logic
//!
//! This module contains context/navigation operations that are portable across all frontends.
//! It follows the reactive signal pattern and manages neighborhood navigation state.

use crate::{
    views::neighborhood::{NeighborhoodState, TraversalPosition},
    AppCore,
};
use async_lock::RwLock;
use aura_core::{identifiers::ChannelId, AuraError};
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
/// 1. Determines target block (home, current, or specific ID)
/// 2. Resolves block name from neighbor list
/// 3. Creates/updates TraversalPosition
/// 4. Updates neighborhood view state
///
/// Depth values:
/// - 0: Street level
/// - 1: Frontage level (default)
/// - 2: Interior level
pub async fn move_position(
    app_core: &Arc<RwLock<AppCore>>,
    block_id: &str,
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
    let mut neighborhood = core.views().get_neighborhood().clone();

    // Determine target block ID
    let target_block_id = if block_id == "home" {
        neighborhood.home_block_id
    } else if block_id == "current" {
        // Stay on current block, just change depth
        neighborhood
            .position
            .as_ref()
            .map(|p| p.current_block_id)
            .unwrap_or(neighborhood.home_block_id)
    } else {
        // Parse block_id as ChannelId, fall back to home if invalid
        block_id
            .parse::<ChannelId>()
            .unwrap_or(neighborhood.home_block_id)
    };

    // Get block name from neighbors or use the ID
    let block_name = neighborhood
        .neighbor(&target_block_id)
        .map(|n| n.name.clone())
        .unwrap_or_else(|| {
            // Check if it's home
            if target_block_id == neighborhood.home_block_id {
                neighborhood.home_block_name.clone()
            } else {
                target_block_id.to_string()
            }
        });

    // Create or update position
    let position = TraversalPosition {
        current_block_id: target_block_id,
        current_block_name: block_name,
        depth: depth_value,
        path: vec![target_block_id],
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
    core.views().get_neighborhood().clone()
}

/// Get current traversal position
///
/// **What it does**: Reads current position from neighborhood state
/// **Returns**: Optional traversal position
/// **Signal pattern**: Read-only operation (no emission)
pub async fn get_current_position(app_core: &Arc<RwLock<AppCore>>) -> Option<TraversalPosition> {
    let core = app_core.read().await;
    let neighborhood = core.views().get_neighborhood();
    neighborhood.position.clone()
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
