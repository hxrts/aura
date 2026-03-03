//! Moderator command handlers
//!
//! Handlers for GrantModerator, RevokeModerator.
//!
//! This module delegates to portable workflows in aura_app::ui::workflows::moderator
//! and adds terminal-specific response formatting.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::ui::prelude::*;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

// Re-export workflows for convenience
pub use aura_app::ui::workflows::moderator::{grant_moderator, revoke_moderator};

/// Handle moderator commands
pub async fn handle_moderator(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::GrantModerator { channel, target } => {
            // Delegate to workflow
            match grant_moderator(app_core, channel.as_deref(), target).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::RevokeModerator { channel, target } => {
            // Delegate to workflow
            match revoke_moderator(app_core, channel.as_deref(), target).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        _ => None,
    }
}
