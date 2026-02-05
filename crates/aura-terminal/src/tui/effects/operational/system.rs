//! System command handlers
//!
//! Handlers for Ping, Shutdown, RefreshAccount, CreateAccount.
//!
//! This module delegates to portable workflows in aura_app::ui::workflows::system
//! and adds terminal-specific response formatting.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::ui::prelude::*;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

// Re-export workflows for convenience
pub use aura_app::ui::workflows::system::{ping, refresh_account};

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

        EffectCommand::CreateAccount { .. } => {
            // Account file and nickname are already persisted by the callback
            // before this command is dispatched. Refresh signals so the UI
            // reflects the newly created account.
            match refresh_account(app_core).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        _ => None,
    }
}
