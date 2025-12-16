//! Query command handlers
//!
//! Handlers for ListParticipants, GetUserInfo.

use std::sync::Arc;

use aura_app::AppCore;
use tokio::sync::RwLock;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

/// Handle query commands
pub async fn handle_query(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::ListParticipants { channel } => {
            // Get contacts snapshot to find participants
            let app_core = app_core.read().await;
            let snapshot = app_core.snapshot();
            let contacts = &snapshot.contacts;

            // Helper to get display name from contact
            let get_name = |c: &aura_app::views::Contact| -> String {
                if !c.petname.is_empty() {
                    c.petname.clone()
                } else if let Some(ref suggested) = c.suggested_name {
                    suggested.clone()
                } else {
                    c.id.chars().take(8).collect::<String>() + "..."
                }
            };

            let mut participants = Vec::new();

            // Always include self (current user)
            participants.push("You".to_string());

            // For DM channels (format: "dm:<contact_id>"), include just that contact
            if channel.starts_with("dm:") {
                let contact_id = channel.strip_prefix("dm:").unwrap_or("");
                if let Some(contact) = contacts.contact(contact_id) {
                    participants.push(get_name(contact));
                } else {
                    participants.push(contact_id.to_string());
                }
            } else {
                // For group channels, include all contacts as potential participants
                // (In a real implementation, this would query actual channel membership)
                for contact in contacts.filtered_contacts() {
                    participants.push(get_name(contact));
                }
            }

            Some(Ok(OpResponse::List(participants)))
        }

        EffectCommand::GetUserInfo { target } => {
            // Get contacts snapshot to find user info
            let app_core = app_core.read().await;
            let snapshot = app_core.snapshot();
            let contacts = &snapshot.contacts;

            // Helper to get display name from contact
            let get_name = |c: &aura_app::views::Contact| -> String {
                if !c.petname.is_empty() {
                    c.petname.clone()
                } else if let Some(ref suggested) = c.suggested_name {
                    suggested.clone()
                } else {
                    c.id.chars().take(8).collect::<String>() + "..."
                }
            };

            // Helper to format contact info
            let format_info = |c: &aura_app::views::Contact| -> String {
                format!(
                    "User: {}\nID: {}\nOnline: {}\nGuardian: {}\nResident: {}",
                    get_name(c),
                    c.id,
                    if c.is_online { "Yes" } else { "No" },
                    if c.is_guardian { "Yes" } else { "No" },
                    if c.is_resident { "Yes" } else { "No" }
                )
            };

            // Look up contact by ID
            if let Some(contact) = contacts.contact(target) {
                Some(Ok(OpResponse::Data(format_info(contact))))
            } else {
                // Try partial match by name
                let matching: Vec<_> = contacts
                    .filtered_contacts()
                    .into_iter()
                    .filter(|c| get_name(c).to_lowercase().contains(&target.to_lowercase()))
                    .collect();

                if matching.len() == 1 {
                    Some(Ok(OpResponse::Data(format_info(matching[0]))))
                } else if matching.is_empty() {
                    Some(Ok(OpResponse::Data(format!("User '{}' not found", target))))
                } else {
                    let names: Vec<_> = matching.iter().map(|c| get_name(c)).collect();
                    Some(Ok(OpResponse::Data(format!(
                        "Multiple matches for '{}': {}",
                        target,
                        names.join(", ")
                    ))))
                }
            }
        }

        _ => None,
    }
}
