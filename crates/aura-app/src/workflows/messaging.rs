//! Messaging Workflow - Portable Business Logic
//!
//! This module contains messaging operations that are portable across all frontends.
//! Uses typed reactive signals for state reads/writes.

use crate::workflows::channel_ref::ChannelRef;
use crate::workflows::context::current_home_context_or_fallback;
use crate::workflows::parse::parse_authority_id;
use crate::workflows::runtime::require_runtime;
use crate::workflows::signals::read_signal;
use crate::workflows::snapshot_policy::{chat_snapshot, contacts_snapshot};
use crate::workflows::state_helpers::with_chat_state;
use crate::{
    signal_defs::{HOMES_SIGNAL, HOMES_SIGNAL_NAME},
    thresholds::{default_channel_threshold, normalize_channel_threshold},
    views::chat::{Channel, ChannelType, ChatState, Message, MessageDeliveryStatus},
    AppCore,
};
use async_lock::RwLock;
use aura_chat::ChatFact;
use aura_core::{
    crypto::hash::hash,
    effects::amp::{
        ChannelCloseParams, ChannelCreateParams, ChannelJoinParams, ChannelLeaveParams,
        ChannelSendParams,
    },
    identifiers::{AuthorityId, ChannelId, ContextId},
    AuraError,
};
use aura_journal::fact::FactOptions;
use aura_journal::DomainFact;
use aura_protocol::amp::{serialize_amp_message, AmpMessage};
use std::sync::Arc;

/// Messaging backend policy (runtime-backed vs UI-local).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessagingBackend {
    /// Use runtime bridge for AMP + persisted facts.
    Runtime,
    /// UI-local state only (no runtime calls).
    LocalOnly,
}

async fn messaging_backend(app_core: &Arc<RwLock<AppCore>>) -> MessagingBackend {
    let core = app_core.read().await;
    if core.runtime().is_some() {
        MessagingBackend::Runtime
    } else {
        MessagingBackend::LocalOnly
    }
}

/// Create a deterministic ChannelId from a DM channel descriptor string
fn dm_channel_id(target: &str) -> ChannelId {
    let descriptor = format!("dm:{target}");
    ChannelId::from_bytes(hash(descriptor.as_bytes()))
}

/// Parse a channel string into a ChannelRef.
fn parse_channel_ref(channel: &str) -> ChannelRef {
    ChannelRef::parse(channel)
}

fn channel_id_from_input(channel: &str) -> ChannelId {
    parse_channel_ref(channel).to_channel_id()
}

/// Get current home channel id (e.g., "home:<id>") with fallback.
pub async fn current_home_channel_id(app_core: &Arc<RwLock<AppCore>>) -> Result<String, AuraError> {
    let homes = read_signal(app_core, &*HOMES_SIGNAL, HOMES_SIGNAL_NAME)
        .await
        .ok();
    let home_id = homes
        .and_then(|homes| homes.current_home_id().map(|id| id.to_string()))
        .unwrap_or_else(|| "home".to_string());

    Ok(format!("home:{home_id}"))
}

async fn context_id_for_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
) -> Result<ContextId, AuraError> {
    {
        let chat = chat_snapshot(app_core).await;
        if let Some(channel) = chat.channel(&channel_id) {
            if let Some(ctx_id) = channel.context_id {
                return Ok(ctx_id);
            }
        }
    }
    {
        let core = app_core.read().await;
        let homes = core.views().get_homes();
        if let Some(home_state) = homes.home_state(&channel_id) {
            if let Some(ctx_id) = home_state.context_id {
                return Ok(ctx_id);
            }
        }
    }

    // Not all channels correspond to a "home" entry in the homes view yet
    // (e.g. AMP-created channels in demos/tests). Fall back to the currently
    // selected home context (or per-authority demo context).
    current_home_context_or_fallback(app_core).await
}

/// Send a direct message to a contact
///
/// **What it does**: Sends a message in a DM channel with the contact
/// **Returns**: DM channel ID
/// **Signal pattern**: Updates ViewState; signal forwarding handles CHAT_SIGNAL
///
/// This operation:
/// 1. Creates DM channel if it doesn't exist
/// 2. Adds message to chat state
/// 3. ViewState update auto-forwards to CHAT_SIGNAL for UI updates
///
/// **Note**: Full implementation would use Intent::SendMessage for persistence.
/// Currently updates chat state locally for UI responsiveness.
///
/// # Arguments
/// * `app_core` - The application core
/// * `target` - Target contact ID
/// * `content` - Message content
/// * `timestamp_ms` - Current timestamp in milliseconds (caller provides via effect system)
pub async fn send_direct_message(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
    content: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let channel_id = dm_channel_id(target);
    let target_id = parse_authority_id(target).ok();

    let now = timestamp_ms;
    with_chat_state(app_core, |chat_state| {
        // Ensure the DM channel exists (create if needed)
        if !chat_state.has_channel(&channel_id) {
            let dm_channel = Channel {
                id: channel_id,
                context_id: None,
                name: format!("DM with {}", &target[..8.min(target.len())]),
                topic: Some(format!("Direct messages with {target}")),
                channel_type: ChannelType::DirectMessage,
                unread_count: 0,
                is_dm: true,
                member_ids: target_id.into_iter().collect(),
                member_count: target_id.map_or(1, |_| 2), // Self + target (if known)
                last_message: None,
                last_message_time: None,
                last_activity: now,
                last_finalized_epoch: 0,
            };
            chat_state.add_channel(dm_channel);
        }

        // Create the message with deterministic ID
        // Use AuthorityId::new_from_entropy([1u8; 32]) for self - in production this would be the actual user's ID
        let message = Message {
            id: format!("msg-{channel_id}-{now}"),
            channel_id,
            sender_id: AuthorityId::new_from_entropy([1u8; 32]),
            sender_name: "You".to_string(),
            content: content.to_string(),
            timestamp: now,
            reply_to: None,
            is_own: true,
            is_read: true,
            delivery_status: MessageDeliveryStatus::Sent,
            epoch_hint: None,
            is_finalized: false,
        };

        // Apply message to state
        chat_state.apply_message(channel_id, message);
    })
    .await?;

    Ok(channel_id.to_string())
}

/// Create a group channel (home channel) in chat state.
///
/// **What it does**: Creates a chat channel and selects it
/// **Returns**: Channel ID
/// **Signal pattern**: Updates `CHAT_SIGNAL` directly
///
/// **Note**: This is currently UI-local state only; persistence will be provided by
/// runtime-backed AMP/Chat facts when fully wired.
pub async fn create_channel(
    app_core: &Arc<RwLock<AppCore>>,
    name: &str,
    topic: Option<String>,
    members: &[String],
    threshold_k: u8,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let backend = messaging_backend(app_core).await;
    let member_ids: Vec<AuthorityId> = members
        .iter()
        .map(|member| parse_authority_id(member))
        .collect::<Result<Vec<_>, AuraError>>()?;
    let mut channel_id = ChannelId::from_bytes(hash(format!("local:{timestamp_ms}").as_bytes()));
    let mut channel_context: Option<ContextId> = None;

    if backend == MessagingBackend::Runtime {
        let runtime = require_runtime(app_core).await?;
        let context_id = current_home_context_or_fallback(app_core).await?;
        channel_context = Some(context_id);
        let channel_hint = (!name.trim().is_empty()).then(|| channel_id_from_input(name));
        let params = ChannelCreateParams {
            context: context_id,
            channel: channel_hint,
            skip_window: None,
            topic: topic.clone(),
        };

        channel_id = runtime
            .amp_create_channel(params)
            .await
            .map_err(|e| AuraError::agent(format!("Failed to create channel: {e}")))?;

        runtime
            .amp_join_channel(ChannelJoinParams {
                context: context_id,
                channel: channel_id,
                participant: runtime.authority_id(),
            })
            .await
            .map_err(|e| AuraError::agent(format!("Failed to join channel: {e}")))?;

        let fact = ChatFact::channel_created_ms(
            context_id,
            channel_id,
            name.to_string(),
            topic.clone(),
            false,
            timestamp_ms,
            runtime.authority_id(),
        )
        .to_generic();

        runtime
            .commit_relational_facts(&[fact])
            .await
            .map_err(|e| AuraError::agent(format!("Failed to persist channel: {e}")))?;
    } else if !name.trim().is_empty() {
        channel_id = channel_id_from_input(name);
    }

    // Update UI state for responsiveness; reactive reductions may also update this later.
    with_chat_state(app_core, |chat_state| {
        let channel = Channel {
            id: channel_id,
            context_id: channel_context,
            name: name.to_string(),
            topic,
            channel_type: ChannelType::Home,
            unread_count: 0,
            is_dm: false,
            member_ids: member_ids.clone(),
            member_count: (member_ids.len() as u32).saturating_add(1),
            last_message: None,
            last_message_time: None,
            last_activity: timestamp_ms,
            last_finalized_epoch: 0,
        };

        chat_state.add_channel(channel);
    })
    .await?;

    // Create channel invitations for selected members (if any).
    if backend == MessagingBackend::Runtime && !member_ids.is_empty() {
        let runtime = require_runtime(app_core).await?;
        let context_id = current_home_context_or_fallback(app_core).await?;

        let mut invitation_ids = Vec::new();
        let total_n = (member_ids.len() + 1) as u8;
        let threshold_k = if threshold_k == 0 {
            default_channel_threshold(total_n)
        } else {
            normalize_channel_threshold(threshold_k, total_n)
        };
        let invitation_message = Some(format!(
            "Group threshold: {threshold_k}-of-{total_n} (keys rotate after everyone accepts)"
        ));

        let bootstrap = runtime
            .amp_create_channel_bootstrap(context_id, channel_id, member_ids.clone())
            .await
            .map_err(|e| AuraError::agent(format!("Failed to bootstrap channel: {e}")))?;

        for receiver in &member_ids {
            let invitation = crate::workflows::invitation::create_channel_invitation(
                app_core,
                *receiver,
                channel_id.to_string(),
                Some(bootstrap.clone()),
                invitation_message.clone(),
                None,
            )
            .await?;
            invitation_ids.push(invitation.invitation_id);
        }

        if !invitation_ids.is_empty() {
            runtime
                .start_channel_invitation_monitor(invitation_ids, context_id, channel_id)
                .await
                .map_err(|e| AuraError::agent(format!("{e}")))?;
        }
    }

    Ok(channel_id.to_string())
}

/// Join an existing channel.
pub async fn join_channel(app_core: &Arc<RwLock<AppCore>>, channel: &str) -> Result<(), AuraError> {
    let runtime = { require_runtime(app_core).await? };

    let channel_id = channel_id_from_input(channel);
    let context_id = current_home_context_or_fallback(app_core).await?;

    runtime
        .amp_join_channel(ChannelJoinParams {
            context: context_id,
            channel: channel_id,
            participant: runtime.authority_id(),
        })
        .await
        .map_err(|e| AuraError::agent(format!("Failed to join channel: {e}")))?;

    Ok(())
}

/// Leave a channel.
pub async fn leave_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel: &str,
) -> Result<(), AuraError> {
    let runtime = { require_runtime(app_core).await? };

    let channel_id = channel_id_from_input(channel);
    let context_id = current_home_context_or_fallback(app_core).await?;

    runtime
        .amp_leave_channel(ChannelLeaveParams {
            context: context_id,
            channel: channel_id,
            participant: runtime.authority_id(),
        })
        .await
        .map_err(|e| AuraError::agent(format!("Failed to leave channel: {e}")))?;

    Ok(())
}

/// Close/archive a channel.
///
/// Today this is a UI-local operation that removes the channel from `CHAT_SIGNAL`.
/// A fully persisted implementation will commit a `ChatFact::ChannelClosed` fact.
pub async fn close_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = { require_runtime(app_core).await? };

    let channel_id = channel_id_from_input(channel);
    let context_id = context_id_for_channel(app_core, channel_id).await?;

    runtime
        .amp_close_channel(ChannelCloseParams {
            context: context_id,
            channel: channel_id,
        })
        .await
        .map_err(|e| AuraError::agent(format!("Failed to close channel: {e}")))?;

    let fact =
        ChatFact::channel_closed_ms(context_id, channel_id, timestamp_ms, runtime.authority_id())
            .to_generic();

    runtime
        .commit_relational_facts(&[fact])
        .await
        .map_err(|e| AuraError::agent(format!("Failed to persist channel close: {e}")))?;

    Ok(())
}

/// Set a channel topic.
///
/// Today this is a UI-local operation that updates the channel entry in `CHAT_SIGNAL`.
/// A fully persisted implementation will commit a topic fact (see work/007.md).
pub async fn set_topic(
    app_core: &Arc<RwLock<AppCore>>,
    channel: &str,
    text: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = { require_runtime(app_core).await? };

    let channel_id = channel_id_from_input(channel);
    let context_id = context_id_for_channel(app_core, channel_id).await?;

    runtime
        .channel_set_topic(context_id, channel_id, text.to_string(), timestamp_ms)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to set channel topic: {e}")))?;

    Ok(())
}

/// Send a message to a group/channel.
///
/// **What it does**: Appends a message to the selected channel's message list
/// **Returns**: Message ID
/// **Signal pattern**: Updates `CHAT_SIGNAL` directly
///
/// **Note**: This is currently UI-local state only; persistence will be provided by
/// runtime-backed AMP/Chat facts when fully wired.
pub async fn send_message(
    app_core: &Arc<RwLock<AppCore>>,
    channel: &str,
    content: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let channel_ref = parse_channel_ref(channel);
    send_message_ref(app_core, channel_ref, content, timestamp_ms).await
}

/// Send a message to a channel by reference.
pub async fn send_message_ref(
    app_core: &Arc<RwLock<AppCore>>,
    channel: ChannelRef,
    content: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let channel_id = channel.to_channel_id();
    let channel_label = match &channel {
        ChannelRef::Id(id) => id.to_string(),
        ChannelRef::Name(name) => name.clone(),
    };

    let message_id = format!("msg-{channel_id}-{timestamp_ms}");
    let backend = messaging_backend(app_core).await;
    let mut channel_context: Option<ContextId> = None;
    let mut epoch_hint: Option<u32> = None;
    let sender_id = if backend == MessagingBackend::Runtime {
        let runtime = require_runtime(app_core).await?;
        let context_id = context_id_for_channel(app_core, channel_id).await?;
        channel_context = Some(context_id);

        let cipher = runtime
            .amp_send_message(ChannelSendParams {
                context: context_id,
                channel: channel_id,
                sender: runtime.authority_id(),
                plaintext: content.as_bytes().to_vec(),
                reply_to: None,
            })
            .await
            .map_err(|e| AuraError::agent(format!("Failed to send message: {e}")))?;

        let wire = AmpMessage::new(cipher.header.clone(), cipher.ciphertext.clone());
        let sealed = serialize_amp_message(&wire)
            .map_err(|e| AuraError::agent(format!("Failed to encode AMP message: {e}")))?;

        // Extract epoch from the AMP header (used for consensus finalization tracking)
        epoch_hint = Some(cipher.header.chan_epoch as u32);

        let fact = ChatFact::message_sent_sealed_ms(
            context_id,
            channel_id,
            message_id.clone(),
            runtime.authority_id(),
            "You".to_string(),
            sealed,
            timestamp_ms,
            None,
            epoch_hint,
        )
        .to_generic();

        // Enable ack tracking for message facts to support delivery confirmation
        runtime
            .commit_relational_facts_with_options(
                &[fact],
                FactOptions::default().with_ack_tracking(),
            )
            .await
            .map_err(|e| AuraError::agent(format!("Failed to persist message: {e}")))?;

        runtime.authority_id()
    } else {
        AuthorityId::new_from_entropy([1u8; 32])
    };

    // Update UI state for responsiveness.
    with_chat_state(app_core, |chat_state| {
        if !chat_state.has_channel(&channel_id) {
            chat_state.add_channel(Channel {
                id: channel_id,
                context_id: channel_context,
                name: channel_label,
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 1,
                last_message: None,
                last_message_time: None,
                last_activity: timestamp_ms,
                last_finalized_epoch: 0,
            });
        }

        chat_state.apply_message(
            channel_id,
            Message {
                id: message_id.clone(),
                channel_id,
                sender_id,
                sender_name: "You".to_string(),
                content: content.to_string(),
                timestamp: timestamp_ms,
                reply_to: None,
                is_own: true,
                is_read: true,
                delivery_status: MessageDeliveryStatus::Sent,
                epoch_hint,
                is_finalized: false,
            },
        );
    })
    .await?;

    Ok(message_id)
}

/// Start a direct chat with a contact
///
/// **What it does**: Creates a DM channel and selects it
/// **Returns**: DM channel ID
/// **Signal pattern**: Updates ViewState; signal forwarding handles CHAT_SIGNAL
///
/// This operation:
/// 1. Gets contact name from ViewState
/// 2. Creates DM channel if it doesn't exist
/// 3. Selects the channel for active conversation
/// 4. ViewState update auto-forwards to CHAT_SIGNAL for UI updates
///
/// # Arguments
/// * `app_core` - The application core
/// * `contact_id` - Contact ID to start chat with
/// * `timestamp_ms` - Current timestamp in milliseconds (caller provides via effect system)
pub async fn start_direct_chat(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let channel_id = dm_channel_id(contact_id);

    // Parse contact_id as AuthorityId for lookup
    let authority_id =
        parse_authority_id(contact_id).unwrap_or_else(|_| AuthorityId::new_from_entropy([1u8; 32]));

    let contacts = contacts_snapshot(app_core).await;

    // Get contact name from ViewState for the channel name
    let contact_name = contacts
        .contact(&authority_id)
        .map(|c| c.nickname.clone())
        .unwrap_or_else(|| format!("DM with {}", &contact_id[..8.min(contact_id.len())]));

    let now = timestamp_ms;

    // Create the DM channel
    let dm_channel = Channel {
        id: channel_id,
        context_id: None,
        name: contact_name,
        topic: Some(format!("Direct messages with {contact_id}")),
        channel_type: ChannelType::DirectMessage,
        unread_count: 0,
        is_dm: true,
        member_ids: vec![authority_id],
        member_count: 2, // Self + contact
        last_message: None,
        last_message_time: None,
        last_activity: now,
        last_finalized_epoch: 0,
    };

    with_chat_state(app_core, |chat_state| {
        // Add the DM channel (add_channel avoids duplicates)
        chat_state.add_channel(dm_channel);
    })
    .await?;

    Ok(channel_id.to_string())
}

/// Get current chat state
///
/// **What it does**: Reads chat state from ViewState
/// **Returns**: Current chat state with channels and messages
/// **Signal pattern**: Read-only operation (no emission)
pub async fn get_chat_state(app_core: &Arc<RwLock<AppCore>>) -> Result<ChatState, AuraError> {
    Ok(chat_snapshot(app_core).await)
}

/// Send an action/emote message to a channel
///
/// **What it does**: Sends an IRC-style /me action to a channel
/// **Returns**: Message ID
/// **Signal pattern**: Updates ViewState; signal forwarding handles CHAT_SIGNAL
///
/// Action messages are formatted as "* Sender action text" and displayed
/// differently from regular messages in the UI.
///
/// # Arguments
/// * `app_core` - The application core
/// * `channel_id` - Target channel ID
/// * `action` - Action text (e.g., "waves hello")
/// * `timestamp_ms` - Current timestamp in milliseconds (caller provides via effect system)
pub async fn send_action(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id_str: &str,
    action: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let content = format!("* You {action}");
    send_message(app_core, channel_id_str, &content, timestamp_ms).await
}

/// Invite a user to join a channel
///
/// **What it does**: Creates a channel invitation for the target user
/// **Returns**: Invitation ID
/// **Signal pattern**: RuntimeBridge handles signal emission
///
/// This delegates to the invitation workflow to create a channel invitation.
/// The target user receives the invitation and can accept to join the channel.
///
/// # Arguments
/// * `app_core` - The application core
/// * `target_user_id` - Target user's authority ID
/// * `channel_id` - Channel to invite user to (required - UI manages selection)
/// * `message` - Optional invitation message
/// * `ttl_ms` - Optional time-to-live for the invitation
pub async fn invite_user_to_channel(
    app_core: &Arc<RwLock<AppCore>>,
    target_user_id: &str,
    channel_id: &str,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<String, AuraError> {
    // Parse target user ID as AuthorityId
    let receiver = parse_authority_id(target_user_id)?;

    // Delegate to invitation workflow
    let invitation = crate::workflows::invitation::create_channel_invitation(
        app_core,
        receiver,
        channel_id.to_string(),
        None,
        message,
        ttl_ms,
    )
    .await?;

    Ok(invitation.invitation_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[tokio::test]
    async fn test_get_chat_state_default() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let state = get_chat_state(&app_core).await.unwrap();
        assert!(state.is_empty());
    }
}
