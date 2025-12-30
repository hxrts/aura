//! Context command handlers
//!
//! Handlers for SetContext, MovePosition, AcceptPendingHomeInvitation.
//!
//! This module delegates to portable workflows in aura_app::workflows::context
//! and adds terminal-specific response formatting.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::AppCore;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

// Re-export workflows for convenience
pub use aura_app::workflows::context::{move_position, set_context};
pub use aura_app::workflows::invitation::accept_pending_home_invitation;

/// Handle context commands
pub async fn handle_context(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::SetContext { context_id } => {
            // Delegate to workflow
            let new_context = if context_id.is_empty() {
                None
            } else {
                Some(context_id.clone())
            };

            match set_context(app_core, new_context.clone()).await {
                Ok(context) => Some(Ok(OpResponse::ContextChanged {
                    context_id: context,
                })),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::MovePosition {
            neighborhood_id: _,
            home_id,
            depth,
        } => {
            // Delegate to workflow
            match move_position(app_core, home_id, depth).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::AcceptPendingHomeInvitation => {
            // Accept a pending home invitation via workflow
            match accept_pending_home_invitation(app_core).await {
                Ok(invitation_id) => Some(Ok(OpResponse::Data(format!(
                    "Accepted home invitation: {invitation_id}"
                )))),
                Err(e) => Some(Err(super::types::OpError::Failed(format!(
                    "Failed to accept home invitation: {e}"
                )))),
            }
        }

        _ => None,
    }
}
