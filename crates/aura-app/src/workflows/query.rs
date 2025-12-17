//! Query Workflow - Portable Business Logic
//!
//! This module contains query operations that are portable across all frontends.
//! These are read-only operations that query contact and channel state.

use crate::{views::Contact, AppCore};
use async_lock::RwLock;
use aura_core::AuraError;
use std::sync::Arc;

/// List participants in a channel
///
/// **What it does**: Queries participants for a specific channel
/// **Returns**: List of participant names
/// **Signal pattern**: Read-only operation (no emission)
///
/// For DM channels (format: "dm:<contact_id>"), returns self + that contact.
/// For group channels, returns self + all contacts (as potential participants).
pub async fn list_participants(
    app_core: &Arc<RwLock<AppCore>>,
    channel: &str,
) -> Result<Vec<String>, AuraError> {
    let app_core_guard = app_core.read().await;
    let snapshot = app_core_guard.snapshot();
    let contacts = &snapshot.contacts;

    let mut participants = Vec::new();

    // Always include self (current user)
    participants.push("You".to_string());

    // For DM channels (format: "dm:<contact_id>"), include just that contact
    if channel.starts_with("dm:") {
        let contact_id = channel.strip_prefix("dm:").unwrap_or("");
        if let Some(contact) = contacts.contact(contact_id) {
            participants.push(get_display_name(contact));
        } else {
            participants.push(contact_id.to_string());
        }
    } else {
        // For group channels, include all contacts as potential participants
        // (In a real implementation, this would query actual channel membership)
        for contact in contacts.filtered_contacts() {
            participants.push(get_display_name(contact));
        }
    }

    Ok(participants)
}

/// Get user information by ID or name
///
/// **What it does**: Queries contact information
/// **Returns**: Contact information or error message
/// **Signal pattern**: Read-only operation (no emission)
///
/// Searches by:
/// 1. Exact ID match
/// 2. Partial name match (case-insensitive)
pub async fn get_user_info(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
) -> Result<Contact, AuraError> {
    let app_core_guard = app_core.read().await;
    let snapshot = app_core_guard.snapshot();
    let contacts = &snapshot.contacts;

    // Look up contact by ID
    if let Some(contact) = contacts.contact(target) {
        return Ok(contact.clone());
    }

    // Try partial match by name
    let matching: Vec<_> = contacts
        .filtered_contacts()
        .into_iter()
        .filter(|c| {
            get_display_name(c)
                .to_lowercase()
                .contains(&target.to_lowercase())
        })
        .collect();

    if matching.len() == 1 {
        Ok(matching[0].clone())
    } else if matching.is_empty() {
        Err(AuraError::not_found(format!("User '{}' not found", target)))
    } else {
        let names: Vec<_> = matching.iter().map(|c| get_display_name(c)).collect();
        Err(AuraError::invalid(format!(
            "Multiple matches for '{}': {}",
            target,
            names.join(", ")
        )))
    }
}

/// Get list of all contacts
///
/// **What it does**: Queries all contacts from snapshot
/// **Returns**: List of contacts
/// **Signal pattern**: Read-only operation (no emission)
pub async fn list_contacts(app_core: &Arc<RwLock<AppCore>>) -> Vec<Contact> {
    let app_core_guard = app_core.read().await;
    let snapshot = app_core_guard.snapshot();
    snapshot
        .contacts
        .filtered_contacts()
        .into_iter()
        .cloned()
        .collect()
}

/// Helper function to get display name from contact
///
/// Priority: petname > suggested_name > truncated ID
fn get_display_name(contact: &Contact) -> String {
    if !contact.petname.is_empty() {
        contact.petname.clone()
    } else if let Some(ref suggested) = contact.suggested_name {
        suggested.clone()
    } else {
        contact.id.chars().take(8).collect::<String>() + "..."
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[tokio::test]
    async fn test_list_contacts() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let contacts = list_contacts(&app_core).await;
        // Default should have empty contacts
        assert!(contacts.is_empty());
    }

    #[tokio::test]
    async fn test_get_user_info_not_found() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let result = get_user_info(&app_core, "nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_participants() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        // DM channel should include "You" + target
        let participants = list_participants(&app_core, "dm:user-123").await.unwrap();
        assert!(participants.contains(&"You".to_string()));
    }
}
