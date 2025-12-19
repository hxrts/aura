//! Query command handlers
//!
//! Handlers for ListParticipants, GetUserInfo.
//!
//! This module delegates to portable workflows in aura_app::workflows::query
//! and adds terminal-specific response formatting.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::AppCore;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

// Re-export workflows for convenience
pub use aura_app::workflows::query::{get_user_info, list_participants};

/// Handle query commands
pub async fn handle_query(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::ListParticipants { channel } => {
            // Delegate to workflow
            match list_participants(app_core, channel).await {
                Ok(participants) => Some(Ok(OpResponse::List(participants))),
                Err(e) => Some(Err(super::types::OpError::Failed(e.to_string()))),
            }
        }

        EffectCommand::GetUserInfo { target } => {
            // Delegate to workflow
            match get_user_info(app_core, target).await {
                Ok(contact) => {
                    // Format contact info for terminal display
                    let id_str = contact.id.to_string();
                    let display_name = if !contact.nickname.is_empty() {
                        contact.nickname.clone()
                    } else if let Some(ref suggested) = contact.suggested_name {
                        suggested.clone()
                    } else {
                        id_str.chars().take(8).collect::<String>() + "..."
                    };

                    let info = format!(
                        "User: {}\nID: {}\nOnline: {}\nGuardian: {}\nResident: {}",
                        display_name,
                        id_str,
                        if contact.is_online { "Yes" } else { "No" },
                        if contact.is_guardian { "Yes" } else { "No" },
                        if contact.is_resident { "Yes" } else { "No" }
                    );

                    Some(Ok(OpResponse::Data(info)))
                }
                Err(e) => {
                    // Workflow already returns user-friendly error messages
                    Some(Ok(OpResponse::Data(e.to_string())))
                }
            }
        }

        _ => None,
    }
}
