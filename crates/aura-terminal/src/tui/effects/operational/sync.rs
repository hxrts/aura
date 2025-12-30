//! Sync command handlers - TUI Operational Layer
//!
//! This module provides TUI-specific sync operation handling.
//! Business logic has been moved to `aura_app::ui::workflows::sync`.
//!
//! ## Architecture
//!
//! - **Business Logic**: `aura_app::ui::workflows::sync` (portable)
//! - **TUI Integration**: This module (operational layer)
//!
//! Handlers for ForceSync, RequestState.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::ui::prelude::*;

use super::types::{OpError, OpResponse, OpResult};
use super::EffectCommand;

// Re-export workflow functions for convenience
pub use aura_app::ui::workflows::sync::{force_sync, request_state};

/// Handle sync commands
///
/// This is now a thin wrapper around workflow functions.
/// Business logic lives in aura_app::ui::workflows::sync.
pub async fn handle_sync(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::ForceSync => {
            // Use workflow for business logic
            let result = force_sync(app_core).await;

            match result {
                Ok(_) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(OpError::Failed(format!("Sync failed: {e}")))),
            }
        }

        EffectCommand::RequestState { peer_id } => {
            // Use workflow for business logic
            let result = request_state(app_core, peer_id).await;

            match result {
                Ok(_) => Some(Ok(OpResponse::Data(format!(
                    "Sync requested from peer: {peer_id}"
                )))),
                Err(e) => Some(Err(OpError::Failed(format!(
                    "Failed to sync from peer {peer_id}: {e}"
                )))),
            }
        }

        _ => None,
    }
}
