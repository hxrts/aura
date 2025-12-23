//! Contacts command handlers
//!
//! Handlers for contact-management commands (nickname updates, etc.).
//!
//! Business logic lives in `aura_app::workflows::contacts`.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::AppCore;

use super::types::{OpError, OpResponse, OpResult};
use super::EffectCommand;

pub use aura_app::workflows::contacts::{remove_contact, update_contact_nickname};

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
            let now = super::time::current_time_ms(app_core).await;
            match update_contact_nickname(app_core, contact_id, nickname, now).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(OpError::Failed(format!(
                    "Failed to update contact nickname: {}",
                    e
                )))),
            }
        }
        EffectCommand::RemoveContact { contact_id } => {
            let now = super::time::current_time_ms(app_core).await;
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
