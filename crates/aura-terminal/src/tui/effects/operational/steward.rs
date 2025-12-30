//! Steward command handlers
//!
//! Handlers for GrantSteward, RevokeSteward.
//!
//! This module delegates to portable workflows in aura_app::ui::workflows::steward
//! and adds terminal-specific response formatting.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::ui::prelude::*;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

// Re-export workflows for convenience
pub use aura_app::ui::workflows::steward::{grant_steward, revoke_steward};

/// Handle steward commands
pub async fn handle_steward(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::GrantSteward { target } => {
            // Delegate to workflow
            match grant_steward(app_core, target).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::RevokeSteward { target } => {
            // Delegate to workflow
            match revoke_steward(app_core, target).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        _ => None,
    }
}
