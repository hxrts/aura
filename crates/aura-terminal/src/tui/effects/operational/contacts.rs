//! Contacts command handlers
//!
//! Handlers for contact-management commands (nickname updates, etc.).
//!
//! Business logic lives in `aura_app::workflows::contacts`.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::AppCore;
use aura_effects::time::PhysicalTimeHandler;

use super::types::{OpError, OpResponse, OpResult};
use super::EffectCommand;

pub use aura_app::workflows::contacts::{remove_contact, update_contact_nickname};

/// Get current time in milliseconds since Unix epoch
fn current_time_ms() -> u64 {
    PhysicalTimeHandler::new().physical_time_now_ms()
}

/// Handle contact commands
pub async fn handle_contacts(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::UpdateContactNickname {
            contact_id,
            nickname,
        } => {
            let now = current_time_ms();
            match update_contact_nickname(app_core, contact_id, nickname, now).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(OpError::Failed(format!(
                    "Failed to update contact nickname: {}",
                    e
                )))),
            }
        }
        EffectCommand::RemoveContact { contact_id } => {
            let now = current_time_ms();
            match remove_contact(app_core, contact_id, now).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(OpError::Failed(format!(
                    "Failed to remove contact: {}",
                    e
                )))),
            }
        }
        _ => None,
    }
}
