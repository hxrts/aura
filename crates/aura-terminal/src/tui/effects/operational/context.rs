//! Context command handlers
//!
//! Handlers for SetContext, MovePosition, AcceptPendingBlockInvitation.

use std::sync::Arc;

use aura_app::AppCore;
use tokio::sync::RwLock;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

/// Handle context commands
pub async fn handle_context(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::SetContext { context_id } => {
            // Set active context - used for navigation and command targeting
            // The actual state update is handled by IoContext when it receives
            // the ContextChanged response
            let new_context = if context_id.is_empty() {
                None
            } else {
                Some(context_id.clone())
            };
            tracing::debug!("SetContext: changing to {:?}", new_context);
            Some(Ok(OpResponse::ContextChanged {
                context_id: new_context,
            }))
        }

        EffectCommand::MovePosition {
            neighborhood_id: _,
            block_id,
            depth,
        } => {
            // Move position in neighborhood view
            // Parse the depth string to determine traversal depth (0=Street, 1=Frontage, 2=Interior)
            let depth_value = match depth.to_lowercase().as_str() {
                "street" => 0,
                "frontage" => 1,
                "interior" => 2,
                _ => 1, // Default to frontage
            };

            // Update neighborhood state with new position
            if let Ok(core) = app_core.try_read() {
                // Get current neighborhood state
                let mut neighborhood = core.views().get_neighborhood();

                // Determine if this is "home" navigation
                let target_block_id = if block_id == "home" {
                    neighborhood.home_block_id.clone()
                } else if block_id == "current" {
                    // Stay on current block, just change depth
                    neighborhood
                        .position
                        .as_ref()
                        .map(|p| p.current_block_id.clone())
                        .unwrap_or_else(|| neighborhood.home_block_id.clone())
                } else {
                    block_id.clone()
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
                            target_block_id.clone()
                        }
                    });

                // Create or update position
                let position = aura_app::views::neighborhood::TraversalPosition {
                    current_block_id: target_block_id.clone(),
                    current_block_name: block_name,
                    depth: depth_value,
                    path: vec![target_block_id],
                };
                neighborhood.position = Some(position);

                // Set the updated state
                core.views().set_neighborhood(neighborhood);
                tracing::debug!(
                    "MovePosition: updated to block {} at depth {}",
                    block_id,
                    depth
                );
            }
            Some(Ok(OpResponse::Ok))
        }

        EffectCommand::AcceptPendingBlockInvitation => {
            // Accept a pending block invitation
            Some(Ok(OpResponse::Ok))
        }

        _ => None,
    }
}
