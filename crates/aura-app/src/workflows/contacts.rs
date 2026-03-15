//! Contacts Workflow - Portable Business Logic
//!
//! This module contains contact-management operations that are portable across
//! all frontends via the RuntimeBridge abstraction.

use super::error::runtime_call;
use crate::views::contacts::ReadReceiptPolicy;
use crate::workflows::context::default_relational_context;
use crate::workflows::parse::parse_authority_id;
use crate::workflows::runtime::require_runtime;
use crate::workflows::snapshot_policy::contacts_snapshot;
use crate::AppCore;
use async_lock::RwLock;
use aura_chat::ChatFact;
use aura_core::identifiers::ChannelId;
use aura_core::AuraError;
use aura_journal::DomainFact;
use aura_relational::ContactFact;
use std::sync::Arc;

/// Add a contact to the local contact list.
///
/// This creates a "contact added" fact that establishes the contact relationship.
/// The nickname is the display name for the contact.
pub async fn add_contact(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: &str,
    nickname: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    let target = parse_authority_id(contact_id)?;
    let trimmed = nickname.trim();
    if trimmed.len() > 100 {
        return Err(AuraError::invalid("Nickname too long"));
    }

    let owner_id = runtime.authority_id();

    let fact = ContactFact::added_with_timestamp_ms(
        default_relational_context(),
        owner_id,
        target,
        trimmed.to_string(),
        timestamp_ms,
    )
    .to_generic();

    runtime
        .commit_relational_facts(&[fact])
        .await
        .map_err(|e| runtime_call("add contact", e))?;

    Ok(())
}

/// Add multiple contacts to the local contact list in a single batch.
///
/// This is more efficient than calling `add_contact` multiple times as it
/// commits all facts in a single operation.
///
/// Each contact is specified as (authority_id_string, nickname, timestamp_ms).
pub async fn add_contacts_batch(
    app_core: &Arc<RwLock<AppCore>>,
    contacts: &[(&str, &str, u64)],
) -> Result<(), AuraError> {
    if contacts.is_empty() {
        return Ok(());
    }

    let runtime = require_runtime(app_core).await?;
    let owner_id = runtime.authority_id();

    let mut facts = Vec::with_capacity(contacts.len());
    for (contact_id, nickname, timestamp_ms) in contacts {
        let target = parse_authority_id(contact_id)?;
        let trimmed = nickname.trim();
        if trimmed.len() > 100 {
            return Err(AuraError::invalid(format!(
                "Nickname too long for contact {contact_id}"
            )));
        }

        let fact = ContactFact::added_with_timestamp_ms(
            default_relational_context(),
            owner_id,
            target,
            trimmed.to_string(),
            *timestamp_ms,
        )
        .to_generic();
        facts.push(fact);
    }

    runtime
        .commit_relational_facts(&facts)
        .await
        .map_err(|e| runtime_call("add contacts", e))?;

    Ok(())
}

/// Update (or clear) a contact's nickname.
///
/// Nicknames are **user-assigned local labels**. Passing an empty nickname clears the label,
/// allowing the contact's `nickname_suggestion` to be used for display again.
pub async fn update_contact_nickname(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: &str,
    nickname: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    let target = parse_authority_id(contact_id)?;

    let trimmed = nickname.trim();
    if trimmed.len() > 100 {
        return Err(AuraError::invalid("Nickname too long"));
    }

    let owner_id = runtime.authority_id();

    // Contacts are currently modeled as generic relational facts; use a stable
    // default context so they don't depend on "current home/chat" context.
    let fact = ContactFact::renamed_with_timestamp_ms(
        default_relational_context(),
        owner_id,
        target,
        trimmed.to_string(),
        timestamp_ms,
    )
    .to_generic();

    runtime
        .commit_relational_facts(&[fact])
        .await
        .map_err(|e| runtime_call("commit contact nickname", e))?;

    Ok(())
}

/// Remove a contact from the local contact list.
pub async fn remove_contact(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    let target = parse_authority_id(contact_id)?;

    let owner_id = runtime.authority_id();

    // Contacts are currently modeled as generic relational facts; use a stable
    // default context so they don't depend on "current home/chat" context.
    let fact = ContactFact::removed_with_timestamp_ms(
        default_relational_context(),
        owner_id,
        target,
        timestamp_ms,
    )
    .to_generic();

    runtime
        .commit_relational_facts(&[fact])
        .await
        .map_err(|e| runtime_call("remove contact", e))?;

    Ok(())
}

/// Update read receipt policy for a contact.
///
/// This controls whether we send read receipts when viewing messages from this contact.
/// Privacy-first default is Disabled.
pub async fn set_read_receipt_policy(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: &str,
    policy: ReadReceiptPolicy,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    let target = parse_authority_id(contact_id)?;
    let owner_id = runtime.authority_id();

    // ReadReceiptPolicy is re-exported from aura_relational, so we can use it directly
    let fact = ContactFact::read_receipt_policy_updated_ms(
        default_relational_context(),
        owner_id,
        target,
        policy,
        timestamp_ms,
    )
    .to_generic();

    runtime
        .commit_relational_facts(&[fact])
        .await
        .map_err(|e| runtime_call("update read receipt policy", e))?;

    Ok(())
}

/// Emit read receipts for messages in a channel.
///
/// This should be called when the user views a channel. It emits MessageRead facts
/// for each unread message from contacts who have read receipts enabled.
///
/// # Arguments
/// * `app_core` - The application core
/// * `context_id` - The context ID for the channel
/// * `channel_id` - The channel being viewed
/// * `unread_messages` - List of (message_id, sender_id) tuples for unread messages
/// * `timestamp_ms` - Current timestamp
pub async fn emit_read_receipts(
    app_core: &Arc<RwLock<AppCore>>,
    context_id: aura_core::identifiers::ContextId,
    channel_id: ChannelId,
    unread_messages: Vec<(String, aura_core::identifiers::AuthorityId)>,
    timestamp_ms: u64,
) -> Result<u32, AuraError> {
    let runtime = require_runtime(app_core).await?;
    let contacts = contacts_snapshot(app_core).await;
    let reader_id = runtime.authority_id();

    let mut facts = Vec::new();
    let mut count = 0u32;

    for (message_id, sender_id) in unread_messages {
        // Skip own messages
        if sender_id == reader_id {
            continue;
        }

        // Check if read receipts are enabled for this sender
        let policy = contacts.get_read_receipt_policy(&sender_id);
        if policy != ReadReceiptPolicy::Enabled {
            continue;
        }

        // Create MessageRead fact
        let fact =
            ChatFact::message_read_ms(context_id, channel_id, message_id, reader_id, timestamp_ms)
                .to_generic();

        facts.push(fact);
        count += 1;
    }

    if !facts.is_empty() {
        runtime
            .commit_relational_facts(&facts)
            .await
            .map_err(|e| runtime_call("emit read receipts", e))?;
    }

    Ok(count)
}
