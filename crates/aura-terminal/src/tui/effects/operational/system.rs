//! System command handlers
//!
//! Handlers for Ping, Shutdown, RefreshAccount.
//!
//! This module delegates to portable workflows in aura_app::workflows::system
//! and adds terminal-specific response formatting.

use std::sync::Arc;

use aura_app::AppCore;
use tokio::sync::RwLock;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

// Re-export workflows for convenience
pub use aura_app::workflows::system::{is_available, ping, refresh_account};

/// Handle system commands
pub async fn handle_system(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::Ping => {
            // Delegate to workflow
            match ping(app_core).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::Shutdown => {
            // Shutdown is handled by the TUI event loop, not here
            Some(Ok(OpResponse::Ok))
        }

        EffectCommand::RefreshAccount => {
            // Delegate to workflow
            match refresh_account(app_core).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        _ => None,
    }
}
