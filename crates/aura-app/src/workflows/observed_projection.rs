//! Observed-only projection mutation helpers.
//!
//! These helpers update both the ViewState (for futures-signals) and the
//! ReactiveHandler signals (for app-level subscriptions) to ensure consistent
//! state across both signal systems without pretending to be authoritative
//! workflow primitives.

use std::sync::Arc;

use async_lock::RwLock;
use aura_chat::{ChatDelta, ChatFact, ChatViewReducer, CHAT_FACT_TYPE_ID};
use aura_composition::{downcast_delta, ViewDeltaReducer};
use aura_core::types::identifiers::{AuthorityId, ChannelId};
use aura_journal::{DomainFact, RelationalFact};

use crate::signal_defs::{
    CHAT_SIGNAL, CHAT_SIGNAL_NAME, CONTACTS_SIGNAL, CONTACTS_SIGNAL_NAME, NEIGHBORHOOD_SIGNAL,
    NEIGHBORHOOD_SIGNAL_NAME, RECOVERY_SIGNAL, RECOVERY_SIGNAL_NAME,
};
use crate::views::{
    chat::{Channel, ChannelType, ChatState, Message, MessageDeliveryStatus},
    contacts::ContactsState,
    neighborhood::NeighborhoodState,
    recovery::RecoveryState,
};
use crate::workflows::parse::{
    parse_authority_id as parse_workflow_authority_id, parse_context_id,
};
use crate::workflows::signals::emit_signal;
use crate::AppCore;
use aura_core::AuraError;

/// Observed-only projection update helper for chat state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. CHAT_SIGNAL (for ReactiveEffects subscribers)
///
/// OWNERSHIP: observed-display-update
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn update_chat_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut ChatState) -> T,
) -> Result<T, AuraError> {
    let (output, state) = {
        let mut core = app_core.write().await;
        let mut state = core.snapshot().chat;
        let output = update(&mut state);
        // OWNERSHIP: observed-display-update
        core.views_mut().set_chat(state.clone());
        (output, state)
    };

    // Also emit to CHAT_SIGNAL for ReactiveEffects subscribers
    emit_signal(app_core, &*CHAT_SIGNAL, state, CHAT_SIGNAL_NAME).await?;

    Ok(output)
}

/// Apply an authoritative chat fact to the local chat projection through the
/// sanctioned chat reducer, then mirror the reduced state into `CHAT_SIGNAL`.
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn reduce_chat_fact_observed(
    app_core: &Arc<RwLock<AppCore>>,
    fact: &ChatFact,
) -> Result<(), AuraError> {
    let RelationalFact::Generic { envelope, .. } = fact.to_generic() else {
        return Err(AuraError::internal(
            "chat fact reduction requires generic relational fact envelope",
        ));
    };

    let reducer = ChatViewReducer;
    let deltas = reducer.reduce_fact(CHAT_FACT_TYPE_ID, &envelope.payload, None);
    let state = {
        let current_state = {
            let core = app_core.read().await;
            core.snapshot().chat
        };
        let mut core = app_core.write().await;
        let mut state = current_state;
        for delta in deltas {
            let Some(chat_delta) = downcast_delta::<ChatDelta>(&delta) else {
                continue;
            };
            apply_chat_delta_reduced(&mut state, chat_delta.clone())?;
        }
        // OWNERSHIP: fact-backed
        core.views_mut().set_chat(state.clone());
        state
    };

    emit_signal(app_core, &*CHAT_SIGNAL, state, CHAT_SIGNAL_NAME).await?;
    Ok(())
}

#[cfg_attr(not(feature = "signals"), allow(dead_code))]
fn parse_channel_id(raw: &str) -> Result<ChannelId, AuraError> {
    raw.parse::<ChannelId>()
        .map_err(|_| AuraError::invalid(format!("Invalid channel ID in chat delta: {raw}")))
}

#[cfg_attr(not(feature = "signals"), allow(dead_code))]
fn parse_authority_id(raw: &str) -> Result<AuthorityId, AuraError> {
    parse_workflow_authority_id(raw)
        .map_err(|_| AuraError::invalid(format!("Invalid authority ID in chat delta: {raw}")))
}

#[cfg_attr(not(feature = "signals"), allow(dead_code))]
#[allow(clippy::manual_unwrap_or_default)]
fn apply_chat_delta_reduced(state: &mut ChatState, delta: ChatDelta) -> Result<(), AuraError> {
    match delta {
        ChatDelta::ChannelAdded {
            channel_id,
            context_id,
            name,
            topic,
            is_dm,
            member_count,
            created_at,
            ..
        } => {
            let channel_id = parse_channel_id(&channel_id)?;
            let context_id = context_id.as_deref().map(parse_context_id).transpose()?;
            let channel_type = if is_dm {
                ChannelType::DirectMessage
            } else {
                ChannelType::Home
            };

            if let Some(channel) = state.channel_mut(&channel_id) {
                channel.context_id = context_id.or(channel.context_id);
                channel.name = name;
                channel.topic = topic;
                channel.is_dm = is_dm;
                channel.channel_type = channel_type;
                channel.member_count = channel.member_count.max(member_count);
                channel.last_activity = channel.last_activity.max(created_at);
            } else {
                let canonical = Channel {
                    id: channel_id,
                    context_id,
                    name,
                    topic,
                    channel_type,
                    unread_count: 0,
                    is_dm,
                    member_ids: Vec::new(),
                    member_count,
                    last_message: None,
                    last_message_time: None,
                    last_activity: created_at,
                    last_finalized_epoch: 0,
                };
                let stale_id = state
                    .all_channels()
                    .find(|channel| {
                        channel.id != channel_id
                            && channel.name.eq_ignore_ascii_case(&canonical.name)
                            && channel.is_dm == canonical.is_dm
                    })
                    .map(|channel| channel.id);
                if let Some(stale_id) = stale_id {
                    state.rebind_channel_identity(&stale_id, canonical);
                } else {
                    state.upsert_channel(canonical);
                }
            }
        }
        ChatDelta::ChannelRemoved { channel_id } => {
            let channel_id = parse_channel_id(&channel_id)?;
            let _ = state.remove_channel(&channel_id);
        }
        ChatDelta::ChannelUpdated {
            channel_id,
            context_id,
            name,
            topic,
            member_count,
            member_ids,
        } => {
            let channel_id = parse_channel_id(&channel_id)?;
            let context_id = context_id.as_deref().map(parse_context_id).transpose()?;
            let member_ids = member_ids
                .map(|ids| {
                    ids.into_iter()
                        .map(|raw| parse_authority_id(&raw))
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?;
            let canonical_name = name.clone().unwrap_or_else(|| channel_id.to_string());

            if let Some(channel) = state.channel_mut(&channel_id) {
                if let Some(context_id) = context_id {
                    channel.context_id = Some(context_id);
                }
                if let Some(name) = name {
                    channel.name = name;
                }
                if topic.is_some() {
                    channel.topic = topic;
                }
                if let Some(member_count) = member_count {
                    channel.member_count = if member_count == 0 {
                        0
                    } else {
                        channel.member_count.max(member_count)
                    };
                }
                if let Some(member_ids) = member_ids {
                    channel.member_ids = member_ids;
                }
            } else {
                let initial_member_ids: Vec<AuthorityId> = match member_ids {
                    Some(member_ids) => member_ids,
                    None => Vec::new(),
                };
                let initial_member_count = member_count.unwrap_or(1);
                let canonical = Channel {
                    id: channel_id,
                    context_id,
                    name: canonical_name.clone(),
                    topic,
                    channel_type: ChannelType::Home,
                    unread_count: 0,
                    is_dm: false,
                    member_ids: initial_member_ids,
                    member_count: initial_member_count,
                    last_message: None,
                    last_message_time: None,
                    last_activity: 0,
                    last_finalized_epoch: 0,
                };
                let stale_id = state
                    .all_channels()
                    .find(|channel| {
                        channel.id != channel_id
                            && !channel.is_dm
                            && channel.name.eq_ignore_ascii_case(&canonical_name)
                    })
                    .map(|channel| channel.id);
                if let Some(stale_id) = stale_id {
                    state.rebind_channel_identity(&stale_id, canonical);
                } else {
                    state.upsert_channel(canonical);
                }
            }
        }
        ChatDelta::MessageAdded {
            channel_id,
            message_id,
            sender_id,
            sender_name,
            content,
            timestamp,
            reply_to,
            epoch_hint,
        } => {
            let channel_id = parse_channel_id(&channel_id)?;
            let sender_id = parse_authority_id(&sender_id)?;
            if !state.has_channel(&channel_id) {
                state.upsert_channel(Channel {
                    id: channel_id,
                    context_id: None,
                    name: channel_id.to_string(),
                    topic: None,
                    channel_type: ChannelType::Home,
                    unread_count: 0,
                    is_dm: false,
                    member_ids: Vec::new(),
                    member_count: 1,
                    last_message: None,
                    last_message_time: None,
                    last_activity: timestamp,
                    last_finalized_epoch: 0,
                });
            }

            state.apply_message(
                channel_id,
                Message {
                    id: message_id,
                    channel_id,
                    sender_id,
                    sender_name,
                    content,
                    timestamp,
                    reply_to,
                    is_own: false,
                    is_read: false,
                    delivery_status: MessageDeliveryStatus::Sent,
                    epoch_hint,
                    is_finalized: false,
                },
            );
        }
        ChatDelta::MessageRemoved {
            channel_id,
            message_id,
        } => {
            let channel_id = parse_channel_id(&channel_id)?;
            state.remove_message(&channel_id, &message_id);
        }
        ChatDelta::MessageUpdated {
            channel_id,
            message_id,
            new_content,
            ..
        } => {
            let channel_id = parse_channel_id(&channel_id)?;
            if let Some(message) = state.message_mut(&channel_id, &message_id) {
                message.content = new_content;
            }
        }
        ChatDelta::MessageDeliveryUpdated {
            channel_id,
            message_id,
            delivery_status,
        } => {
            let channel_id = parse_channel_id(&channel_id)?;
            if let Some(message) = state.message_mut(&channel_id, &message_id) {
                message.delivery_status = match delivery_status {
                    aura_chat::ChatMessageDeliveryStatus::Sent => MessageDeliveryStatus::Sent,
                    aura_chat::ChatMessageDeliveryStatus::Delivered => {
                        MessageDeliveryStatus::Delivered
                    }
                    aura_chat::ChatMessageDeliveryStatus::Read => MessageDeliveryStatus::Read,
                    aura_chat::ChatMessageDeliveryStatus::Failed => MessageDeliveryStatus::Failed,
                };
            }
        }
        ChatDelta::MessageRead {
            channel_id,
            message_id,
            ..
        } => {
            let channel_id = parse_channel_id(&channel_id)?;
            state.mark_message_read(&channel_id, &message_id);
        }
    }

    Ok(())
}

/// Observed-only projection update helper for recovery state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. RECOVERY_SIGNAL (for ReactiveEffects subscribers)
///
/// OWNERSHIP: observed-display-update
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn update_recovery_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut RecoveryState) -> T,
) -> Result<T, AuraError> {
    let (output, state) = {
        let mut core = app_core.write().await;
        let mut state = core.snapshot().recovery;
        let output = update(&mut state);
        // OWNERSHIP: observed-display-update
        core.views_mut().set_recovery(state.clone());
        (output, state)
    };

    // Also emit to RECOVERY_SIGNAL for ReactiveEffects subscribers
    emit_signal(app_core, &*RECOVERY_SIGNAL, state, RECOVERY_SIGNAL_NAME).await?;

    Ok(output)
}

/// Observed-only projection update helper for contacts state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. CONTACTS_SIGNAL (for ReactiveEffects subscribers)
///
/// OWNERSHIP: observed-display-update
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn update_contacts_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut ContactsState) -> T,
) -> Result<T, AuraError> {
    let (output, state) = {
        let mut core = app_core.write().await;
        let mut state = core.snapshot().contacts;
        let output = update(&mut state);
        // OWNERSHIP: observed-display-update
        core.views_mut().set_contacts(state.clone());
        (output, state)
    };

    emit_signal(app_core, &*CONTACTS_SIGNAL, state, CONTACTS_SIGNAL_NAME).await?;

    Ok(output)
}

/// Observed-only projection update helper for neighborhood state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. NEIGHBORHOOD_SIGNAL (for ReactiveEffects subscribers)
///
/// OWNERSHIP: observed-display-update
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn update_neighborhood_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut NeighborhoodState) -> T,
) -> Result<T, AuraError> {
    let (output, state) = {
        let mut core = app_core.write().await;
        let mut state = core.snapshot().neighborhood;
        let output = update(&mut state);
        // OWNERSHIP: observed-display-update
        core.views_mut().set_neighborhood(state.clone());
        (output, state)
    };

    // Also emit to NEIGHBORHOOD_SIGNAL for ReactiveEffects subscribers
    emit_signal(
        app_core,
        &*NEIGHBORHOOD_SIGNAL,
        state,
        NEIGHBORHOOD_SIGNAL_NAME,
    )
    .await?;

    Ok(output)
}
