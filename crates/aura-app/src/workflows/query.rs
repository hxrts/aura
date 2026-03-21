//! Query Workflow - Portable Business Logic
//!
//! This module contains query operations that are portable across all frontends.
//! These are read-only operations that query contact and channel state.

use crate::workflows::channel_ref::ChannelRef;
use crate::workflows::observed_snapshot::{observed_chat_snapshot, observed_contacts_snapshot};
use crate::workflows::parse::parse_authority_id;
use crate::{views::Contact, AppCore};
use async_lock::RwLock;
use aura_core::types::identifiers::{AuthorityId, ChannelId};
use aura_core::AuraError;
use std::collections::BTreeSet;
use std::sync::Arc;

/// List participants in a channel
///
/// **What it does**: Queries participants for a specific channel
/// **Returns**: List of participant names
/// **Signal pattern**: Read-only operation (no emission)
///
/// For DM channels, returns self + known members from materialized channel
/// state.
/// For group channels, returns self + known members from channel state.
pub async fn list_participants(
    app_core: &Arc<RwLock<AppCore>>,
    channel: &str,
) -> Result<Vec<String>, AuraError> {
    // OWNERSHIP: observed
    let contacts = observed_contacts_snapshot(app_core).await;
    let chat = observed_chat_snapshot(app_core).await;

    let mut participants = vec!["You".to_string()];
    let mut seen = BTreeSet::new();
    seen.insert("You".to_string());

    let channel_ref = ChannelRef::parse(channel);
    let channel_entry = match channel_ref {
        ChannelRef::Id(id) => chat.channel(&id),
        ChannelRef::Name(name) => chat
            .all_channels()
            .find(|c| c.name.eq_ignore_ascii_case(&name)),
    };

    if let Some(channel_entry) = channel_entry {
        for member_id in &channel_entry.member_ids {
            let name = if let Some(contact) = contacts.contact(member_id) {
                effective_name(contact)
            } else {
                member_id.to_string()
            };
            if seen.insert(name.clone()) {
                participants.push(name);
            }
        }

        return Ok(participants);
    }

    Err(AuraError::not_found(channel.to_string()))
}

/// List participants in a channel by canonical channel ID.
pub async fn list_participants_by_channel_id(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
) -> Result<Vec<String>, AuraError> {
    list_participants(app_core, &channel_id.to_string()).await
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
    resolve_contact(app_core, target).await
}

/// Get user information by canonical authority ID.
pub async fn get_user_info_by_authority_id(
    app_core: &Arc<RwLock<AppCore>>,
    authority_id: AuthorityId,
) -> Result<Contact, AuraError> {
    // OWNERSHIP: observed
    let contacts = observed_contacts_snapshot(app_core).await;
    contacts
        .contact(&authority_id)
        .cloned()
        .ok_or_else(|| AuraError::not_found(authority_id.to_string()))
}

/// Resolve a user target string to a contact.
///
/// Resolution order:
/// 1. Exact authority ID match
/// 2. Exact nickname / nickname suggestion match (case-insensitive)
/// 3. Prefix ID or partial effective-name match (case-insensitive)
pub async fn resolve_contact(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
) -> Result<Contact, AuraError> {
    // OWNERSHIP: observed
    let contacts = observed_contacts_snapshot(app_core).await;
    let target = target.trim();
    if target.is_empty() {
        return Err(AuraError::invalid("User target cannot be empty"));
    }

    if let Ok(authority_id) = parse_authority_id(target) {
        if let Some(contact) = contacts.contact(&authority_id) {
            return Ok(contact.clone());
        }
    }

    let target_lower = target.to_lowercase();
    let mut exact = Vec::new();
    let mut fuzzy = Vec::new();
    for contact in contacts.all_contacts() {
        let id = contact.id.to_string();
        let nickname = contact.nickname.trim();
        let suggestion = contact.nickname_suggestion.as_deref().unwrap_or("").trim();
        let effective = effective_name(contact);

        if id.eq_ignore_ascii_case(target)
            || (!nickname.is_empty() && nickname.eq_ignore_ascii_case(target))
            || (!suggestion.is_empty() && suggestion.eq_ignore_ascii_case(target))
        {
            exact.push(contact.clone());
            continue;
        }

        if id.to_lowercase().starts_with(&target_lower)
            || effective.to_lowercase().contains(&target_lower)
        {
            fuzzy.push(contact.clone());
        }
    }

    let matching = if exact.is_empty() { fuzzy } else { exact };

    if matching.len() == 1 {
        Ok(matching[0].clone())
    } else if matching.is_empty() {
        Err(AuraError::not_found(target.to_string()))
    } else {
        let names: Vec<_> = matching.iter().map(effective_name).collect();
        Err(AuraError::invalid(format!(
            "Multiple matches for '{}': {}",
            target,
            names.join(", ")
        )))
    }
}

/// Get list of all contacts
///
/// **What it does**: Queries all contacts from CONTACTS_SIGNAL (preferred) or ViewState snapshot
/// **Returns**: List of contacts
/// **Signal pattern**: Read-only operation (no emission)
///
/// This function reads from CONTACTS_SIGNAL first, which is populated by the agent's
/// reactive pipeline. Falls back to ViewState snapshot if the signal is not available.
pub async fn list_contacts(app_core: &Arc<RwLock<AppCore>>) -> Vec<Contact> {
    // OWNERSHIP: observed
    observed_contacts_snapshot(app_core)
        .await
        .all_contacts()
        .cloned()
        .collect()
}

/// Helper function to get effective name from contact
///
/// Priority: nickname > nickname_suggestion > truncated ID
fn effective_name(contact: &Contact) -> String {
    if !contact.nickname.is_empty() {
        contact.nickname.clone()
    } else if let Some(ref suggested) = contact.nickname_suggestion {
        suggested.clone()
    } else {
        let id_str = contact.id.to_string();
        id_str.chars().take(8).collect::<String>() + "..."
    }
}

#[cfg(test)]
#[allow(clippy::default_trait_access)]
mod tests {
    use super::*;
    use crate::views::{Channel, ChannelType, ChatState, Contact, ContactsState};
    use crate::AppConfig;
    use aura_core::types::identifiers::{AuthorityId, ChannelId};

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
    async fn test_get_user_info_reads_materialized_contacts() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let bob_id = AuthorityId::new_from_entropy([7u8; 32]);
        let bob = Contact {
            id: bob_id,
            nickname: "Bob".to_string(),
            nickname_suggestion: Some("Bobby".to_string()),
            is_guardian: false,
            is_member: false,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: Default::default(),
        };
        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_contacts(ContactsState::from_contacts(vec![bob.clone()]));
        }

        let by_name = get_user_info(&app_core, "bob").await.unwrap();
        assert_eq!(by_name.id, bob_id);

        let by_id = get_user_info(&app_core, &bob_id.to_string()).await.unwrap();
        assert_eq!(by_id.id, bob_id);
    }

    #[tokio::test]
    async fn test_list_participants_requires_materialized_channel() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let error = list_participants(&app_core, "dm:user-123")
            .await
            .expect_err("legacy dm descriptors should not be upgraded into channel truth");
        assert!(matches!(error, AuraError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_list_participants_does_not_fallback_to_all_contacts() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let bob_id = AuthorityId::new_from_entropy([8u8; 32]);
        let bob = Contact {
            id: bob_id,
            nickname: "Bob".to_string(),
            nickname_suggestion: None,
            is_guardian: false,
            is_member: false,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: Default::default(),
        };
        let channel_id = ChannelId::from_bytes([9u8; 32]);
        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_contacts(ContactsState::from_contacts(vec![bob]));
            core.views_mut()
                .set_chat(ChatState::from_channels(vec![Channel {
                    id: channel_id,
                    context_id: None,
                    name: "empty-room".to_string(),
                    topic: None,
                    channel_type: ChannelType::Home,
                    unread_count: 0,
                    is_dm: false,
                    member_ids: Vec::new(),
                    member_count: 1,
                    last_message: None,
                    last_message_time: None,
                    last_activity: 0,
                    last_finalized_epoch: 0,
                }]));
        }

        let participants = list_participants(&app_core, "empty-room")
            .await
            .expect("materialized channel should still resolve");
        assert_eq!(participants, vec!["You".to_string()]);
    }
}
