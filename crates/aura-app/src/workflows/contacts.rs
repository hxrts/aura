//! Contacts Workflow - Portable Business Logic
//!
//! This module contains contact-management operations that are portable across
//! all frontends via the RuntimeBridge abstraction.

use super::error::runtime_call;
use crate::views::contacts::ReadReceiptPolicy;
use crate::workflows::context::default_relational_context;
use crate::workflows::observed_snapshot::observed_contacts_snapshot;
use crate::workflows::parse::parse_authority_id;
use crate::workflows::runtime::{require_runtime, timeout_runtime_call};
use crate::AppCore;
use async_lock::RwLock;
use aura_chat::ChatFact;
use aura_core::hash::hash;
use aura_core::time::PhysicalTime;
use aura_core::types::identifiers::ChannelId;
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::AuraError;
use aura_journal::DomainFact;
use aura_relational::{ContactFact, FriendshipFact};
use std::sync::Arc;
use std::time::Duration;

const CONTACTS_RUNTIME_TIMEOUT: Duration = Duration::from_millis(5_000);

fn friendship_context(local: AuthorityId, peer: AuthorityId) -> ContextId {
    let mut left = local.to_bytes();
    let mut right = peer.to_bytes();
    if left > right {
        std::mem::swap(&mut left, &mut right);
    }

    let mut seed = b"friendship-context:v1".to_vec();
    seed.extend_from_slice(&left);
    seed.extend_from_slice(&right);
    ContextId::new_from_entropy(hash(&seed))
}

fn physical_time(timestamp_ms: u64) -> PhysicalTime {
    PhysicalTime {
        ts_ms: timestamp_ms,
        uncertainty: None,
    }
}

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
    let facts = vec![fact];

    timeout_runtime_call(
        &runtime,
        "add_contact",
        "commit_relational_facts",
        CONTACTS_RUNTIME_TIMEOUT,
        || runtime.commit_relational_facts(&facts),
    )
    .await?
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

    timeout_runtime_call(
        &runtime,
        "add_contacts_batch",
        "commit_relational_facts",
        CONTACTS_RUNTIME_TIMEOUT,
        || runtime.commit_relational_facts(&facts),
    )
    .await?
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
    let facts = vec![fact];

    timeout_runtime_call(
        &runtime,
        "update_contact_nickname",
        "commit_relational_facts",
        CONTACTS_RUNTIME_TIMEOUT,
        || runtime.commit_relational_facts(&facts),
    )
    .await?
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
    let facts = vec![fact];

    timeout_runtime_call(
        &runtime,
        "remove_contact",
        "commit_relational_facts",
        CONTACTS_RUNTIME_TIMEOUT,
        || runtime.commit_relational_facts(&facts),
    )
    .await?
    .map_err(|e| runtime_call("remove contact", e))?;

    Ok(())
}

/// Push committed friendship facts to the other party.
///
/// Journal anti-entropy only exchanges each authority's own journal, so the
/// relational friendship facts must be delivered to the peer directly (the
/// same verified relational-fact envelope used for direct-chat facts).
async fn deliver_friendship_facts(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    peer: AuthorityId,
    context: ContextId,
    facts: &[aura_journal::fact::RelationalFact],
) -> Result<(), AuraError> {
    for fact in facts {
        timeout_runtime_call(
            runtime,
            "deliver_friendship_facts",
            "send_chat_fact",
            CONTACTS_RUNTIME_TIMEOUT,
            || runtime.send_chat_fact(peer, context, fact),
        )
        .await?
        .map_err(|e| runtime_call("deliver friendship fact", e))?;
    }
    Ok(())
}

/// Send a bilateral friend request for an existing unilateral contact.
pub async fn send_friend_request(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    let target = parse_authority_id(contact_id)?;
    let owner_id = runtime.authority_id();
    let fact = FriendshipFact::Proposed {
        context_id: friendship_context(owner_id, target),
        requester: owner_id,
        accepter: target,
        proposed_at: physical_time(timestamp_ms),
    }
    .to_generic();
    let facts = vec![fact];

    timeout_runtime_call(
        &runtime,
        "send_friend_request",
        "commit_relational_facts",
        CONTACTS_RUNTIME_TIMEOUT,
        || runtime.commit_relational_facts(&facts),
    )
    .await?
    .map_err(|e| runtime_call("send friend request", e))?;
    deliver_friendship_facts(
        &runtime,
        target,
        friendship_context(owner_id, target),
        &facts,
    )
    .await?;

    Ok(())
}

/// Accept an inbound bilateral friend request.
pub async fn accept_friend_request(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    let target = parse_authority_id(contact_id)?;
    let owner_id = runtime.authority_id();
    let fact = FriendshipFact::Accepted {
        context_id: friendship_context(owner_id, target),
        requester: target,
        accepter: owner_id,
        accepted_at: physical_time(timestamp_ms),
    }
    .to_generic();
    let facts = vec![fact];

    timeout_runtime_call(
        &runtime,
        "accept_friend_request",
        "commit_relational_facts",
        CONTACTS_RUNTIME_TIMEOUT,
        || runtime.commit_relational_facts(&facts),
    )
    .await?
    .map_err(|e| runtime_call("accept friend request", e))?;
    deliver_friendship_facts(
        &runtime,
        target,
        friendship_context(owner_id, target),
        &facts,
    )
    .await?;

    Ok(())
}

/// Decline an inbound bilateral friend request.
pub async fn decline_friend_request(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    let target = parse_authority_id(contact_id)?;
    let owner_id = runtime.authority_id();
    let fact = FriendshipFact::Revoked {
        context_id: friendship_context(owner_id, target),
        requester: target,
        accepter: owner_id,
        revoked_at: physical_time(timestamp_ms),
    }
    .to_generic();
    let facts = vec![fact];

    timeout_runtime_call(
        &runtime,
        "decline_friend_request",
        "commit_relational_facts",
        CONTACTS_RUNTIME_TIMEOUT,
        || runtime.commit_relational_facts(&facts),
    )
    .await?
    .map_err(|e| runtime_call("decline friend request", e))?;
    deliver_friendship_facts(
        &runtime,
        target,
        friendship_context(owner_id, target),
        &facts,
    )
    .await?;

    Ok(())
}

/// Revoke an existing bilateral friendship or cancel an outbound request.
pub async fn revoke_friendship(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    let target = parse_authority_id(contact_id)?;
    let owner_id = runtime.authority_id();
    let fact = FriendshipFact::Revoked {
        context_id: friendship_context(owner_id, target),
        requester: owner_id,
        accepter: target,
        revoked_at: physical_time(timestamp_ms),
    }
    .to_generic();
    let facts = vec![fact];

    timeout_runtime_call(
        &runtime,
        "revoke_friendship",
        "commit_relational_facts",
        CONTACTS_RUNTIME_TIMEOUT,
        || runtime.commit_relational_facts(&facts),
    )
    .await?
    .map_err(|e| runtime_call("revoke friendship", e))?;
    deliver_friendship_facts(
        &runtime,
        target,
        friendship_context(owner_id, target),
        &facts,
    )
    .await?;

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
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    let timestamp_ms = crate::workflows::time::current_time_ms(app_core)
        .await
        .map_err(|error| AuraError::internal(error.to_string()))?;

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
    let facts = vec![fact];

    timeout_runtime_call(
        &runtime,
        "set_read_receipt_policy",
        "commit_relational_facts",
        CONTACTS_RUNTIME_TIMEOUT,
        || runtime.commit_relational_facts(&facts),
    )
    .await?
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
///
/// OWNERSHIP: observed
pub async fn emit_read_receipts(
    app_core: &Arc<RwLock<AppCore>>,
    context_id: aura_core::types::identifiers::ContextId,
    channel_id: ChannelId,
    unread_messages: Vec<(String, aura_core::types::identifiers::AuthorityId)>,
    timestamp_ms: u64,
) -> Result<u32, AuraError> {
    let runtime = require_runtime(app_core).await?;
    let contacts = observed_contacts_snapshot(app_core).await;
    let reader_id = runtime.authority_id();

    let mut facts = Vec::new();
    let mut count = 0u32;
    let mut recipients = Vec::new();

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

        recipients.push((sender_id, fact.clone()));
        facts.push(fact);
        count += 1;
    }

    if !facts.is_empty() {
        timeout_runtime_call(
            &runtime,
            "emit_read_receipts",
            "commit_relational_facts",
            CONTACTS_RUNTIME_TIMEOUT,
            || runtime.commit_relational_facts(&facts),
        )
        .await?
        .map_err(|e| runtime_call("emit read receipts", e))?;
    }

    // The sender learns its message was read from the receipt itself; delivery
    // is best effort, like delivery receipts.
    for (sender_id, fact) in &recipients {
        let _ = timeout_runtime_call(
            &runtime,
            "emit_read_receipts",
            "send_chat_fact",
            CONTACTS_RUNTIME_TIMEOUT,
            || runtime.send_chat_fact(*sender_id, context_id, fact),
        )
        .await;
    }

    Ok(count)
}

/// Send read receipts for the received messages of a channel the user is
/// viewing that have not been marked read yet. Returns how many were sent.
///
/// OWNERSHIP: observed
pub async fn mark_channel_viewed(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
) -> Result<u32, AuraError> {
    let chat = crate::workflows::signals::read_signal(
        app_core,
        &*crate::signal_defs::CHAT_SIGNAL,
        crate::signal_defs::CHAT_SIGNAL_NAME,
    )
    .await?;
    let Some(context_id) = chat
        .all_channels()
        .find(|channel| channel.id == channel_id)
        .and_then(|channel| channel.context_id)
    else {
        return Ok(0);
    };
    let unread: Vec<_> = chat
        .messages_for_channel(&channel_id)
        .iter()
        .filter(|message| !message.is_own && !message.is_read)
        .map(|message| (message.id.clone(), message.sender_id))
        .collect();
    if unread.is_empty() {
        return Ok(0);
    }
    let timestamp_ms = crate::workflows::time::current_time_ms(app_core).await?;
    emit_read_receipts(app_core, context_id, channel_id, unread, timestamp_ms).await
}
