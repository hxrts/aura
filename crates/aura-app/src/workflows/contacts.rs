//! Contacts Workflow - Portable Business Logic
//!
//! This module contains contact-management operations that are portable across
//! all frontends via the RuntimeBridge abstraction.

use crate::AppCore;
use async_lock::RwLock;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::AuraError;
use aura_journal::DomainFact;
use aura_relational::ContactFact;
use std::sync::Arc;

/// Update (or clear) a contact's nickname.
///
/// Nicknames are **user-assigned local labels**. Passing an empty nickname clears the label,
/// allowing the contact's `suggested_name` to be used for display again.
pub async fn update_contact_nickname(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: &str,
    nickname: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    let target = contact_id
        .parse::<AuthorityId>()
        .map_err(|_| AuraError::invalid(format!("Invalid authority ID: {contact_id}")))?;

    let trimmed = nickname.trim();
    if trimmed.len() > 100 {
        return Err(AuraError::invalid("Nickname too long"));
    }

    let owner_id = runtime.authority_id();

    // Contacts are currently modeled as generic relational facts; use a stable
    // default context so they don't depend on "current home/chat" context.
    let fact = ContactFact::renamed_with_timestamp_ms(
        ContextId::default(),
        owner_id,
        target,
        trimmed.to_string(),
        timestamp_ms,
    )
    .to_generic();

    runtime
        .commit_relational_facts(&[fact])
        .await
        .map_err(|e| AuraError::agent(format!("Failed to commit contact nickname: {e}")))?;

    Ok(())
}

/// Remove a contact from the local contact list.
pub async fn remove_contact(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    let target = contact_id
        .parse::<AuthorityId>()
        .map_err(|_| AuraError::invalid(format!("Invalid authority ID: {contact_id}")))?;

    let owner_id = runtime.authority_id();

    // Contacts are currently modeled as generic relational facts; use a stable
    // default context so they don't depend on "current home/chat" context.
    let fact = ContactFact::removed_with_timestamp_ms(
        ContextId::default(),
        owner_id,
        target,
        timestamp_ms,
    )
    .to_generic();

    runtime
        .commit_relational_facts(&[fact])
        .await
        .map_err(|e| AuraError::agent(format!("Failed to remove contact: {e}")))?;

    Ok(())
}
