//! Messaging Workflow - Portable Business Logic
//!
//! This module contains messaging operations that are portable across all frontends.
//! Uses typed reactive signals for state reads/writes.

use crate::{
    signal_defs::{CHAT_SIGNAL, CONTACTS_SIGNAL},
    views::chat::{Channel, ChannelType, ChatState, Message},
    AppCore,
};
use async_lock::RwLock;
use aura_core::{
    crypto::hash::hash,
    effects::reactive::ReactiveEffects,
    identifiers::{AuthorityId, ChannelId},
    AuraError,
};
use std::sync::Arc;

/// Create a deterministic ChannelId from a DM channel descriptor string
fn dm_channel_id(target: &str) -> ChannelId {
    let descriptor = format!("dm:{}", target);
    ChannelId::from_bytes(hash(descriptor.as_bytes()))
}

/// Parse a channel string into a ChannelId.
///
/// Accepts either:
/// - `channel:<hex>` (canonical `ChannelId` display format)
/// - a human-friendly name (hashed deterministically, case-insensitive)
fn parse_channel_id(channel: &str) -> ChannelId {
    channel
        .parse::<ChannelId>()
        .unwrap_or_else(|_| ChannelId::from_bytes(hash(channel.to_lowercase().as_bytes())))
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

    let core = app_core.read().await;
    let mut chat_state = core
        .read(&*CHAT_SIGNAL)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to read CHAT_SIGNAL: {}", e)))?;

    let now = timestamp_ms;

    // Ensure the DM channel exists (create if needed)
    if !chat_state.channels.iter().any(|c| c.id == channel_id) {
        let dm_channel = Channel {
            id: channel_id,
            name: format!("DM with {}", &target[..8.min(target.len())]),
            topic: Some(format!("Direct messages with {}", target)),
            channel_type: ChannelType::DirectMessage,
            unread_count: 0,
            is_dm: true,
            member_count: 2, // Self + target
            last_message: None,
            last_message_time: None,
            last_activity: now,
        };
        chat_state.add_channel(dm_channel);
    }

    // Select this channel so messages are visible
    chat_state.selected_channel_id = Some(channel_id);

    // Create the message with deterministic ID
    // Use AuthorityId::default() for self - in production this would be the actual user's ID
    let message = Message {
        id: format!("msg-{}-{}", channel_id, now),
        channel_id,
        sender_id: AuthorityId::default(),
        sender_name: "You".to_string(),
        content: content.to_string(),
        timestamp: now,
        reply_to: None,
        is_own: true,
        is_read: true,
    };

    // Apply message to state
    chat_state.apply_message(channel_id, message);

    core.emit(&*CHAT_SIGNAL, chat_state)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to emit CHAT_SIGNAL: {}", e)))?;

    Ok(channel_id.to_string())
}

/// Create a group channel (block channel) in chat state.
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
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let channel_id = parse_channel_id(name);
    let now = timestamp_ms;

    let core = app_core.read().await;
    let mut chat_state = core
        .read(&*CHAT_SIGNAL)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to read CHAT_SIGNAL: {}", e)))?;

    if !chat_state.channels.iter().any(|c| c.id == channel_id) {
        let channel = Channel {
            id: channel_id,
            name: name.to_string(),
            topic,
            channel_type: ChannelType::Block,
            unread_count: 0,
            is_dm: false,
            member_count: (members.len() as u32).saturating_add(1),
            last_message: None,
            last_message_time: None,
            last_activity: now,
        };
        chat_state.add_channel(channel);
    }

    chat_state.selected_channel_id = Some(channel_id);

    core.emit(&*CHAT_SIGNAL, chat_state)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to emit CHAT_SIGNAL: {}", e)))?;

    Ok(channel_id.to_string())
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
    let channel_id = parse_channel_id(channel);
    let now = timestamp_ms;

    let core = app_core.read().await;
    let mut chat_state = core
        .read(&*CHAT_SIGNAL)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to read CHAT_SIGNAL: {}", e)))?;

    if !chat_state.channels.iter().any(|c| c.id == channel_id) {
        let channel = Channel {
            id: channel_id,
            name: channel.to_string(),
            topic: None,
            channel_type: ChannelType::Block,
            unread_count: 0,
            is_dm: false,
            member_count: 1,
            last_message: None,
            last_message_time: None,
            last_activity: now,
        };
        chat_state.add_channel(channel);
    }

    // Select the channel so messages are visible in `chat_state.messages`.
    chat_state.selected_channel_id = Some(channel_id);

    let message_id = format!("msg-{}-{}", channel_id, now);
    let message = Message {
        id: message_id.clone(),
        channel_id,
        sender_id: AuthorityId::default(),
        sender_name: "You".to_string(),
        content: content.to_string(),
        timestamp: now,
        reply_to: None,
        is_own: true,
        is_read: true,
    };

    chat_state.apply_message(channel_id, message);

    core.emit(&*CHAT_SIGNAL, chat_state)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to emit CHAT_SIGNAL: {}", e)))?;

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
    let authority_id = contact_id
        .parse::<AuthorityId>()
        .unwrap_or_else(|_| AuthorityId::default());

    let core = app_core.read().await;
    let contacts = core
        .read(&*CONTACTS_SIGNAL)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to read CONTACTS_SIGNAL: {}", e)))?;

    // Get contact name from ViewState for the channel name
    let contact_name = contacts
        .contacts
        .iter()
        .find(|c| c.id == authority_id)
        .map(|c| c.nickname.clone())
        .unwrap_or_else(|| format!("DM with {}", &contact_id[..8.min(contact_id.len())]));

    let now = timestamp_ms;

    // Create the DM channel
    let dm_channel = Channel {
        id: channel_id,
        name: contact_name,
        topic: Some(format!("Direct messages with {}", contact_id)),
        channel_type: ChannelType::DirectMessage,
        unread_count: 0,
        is_dm: true,
        member_count: 2, // Self + contact
        last_message: None,
        last_message_time: None,
        last_activity: now,
    };

    let mut chat_state = core
        .read(&*CHAT_SIGNAL)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to read CHAT_SIGNAL: {}", e)))?;

    // Add the DM channel (add_channel avoids duplicates)
    chat_state.add_channel(dm_channel);

    // Select this channel (don't clear messages - retain history)
    chat_state.selected_channel_id = Some(channel_id);

    core.emit(&*CHAT_SIGNAL, chat_state)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to emit CHAT_SIGNAL: {}", e)))?;

    Ok(channel_id.to_string())
}

/// Get current chat state
///
/// **What it does**: Reads chat state from ViewState
/// **Returns**: Current chat state with channels and messages
/// **Signal pattern**: Read-only operation (no emission)
pub async fn get_chat_state(app_core: &Arc<RwLock<AppCore>>) -> Result<ChatState, AuraError> {
    let core = app_core.read().await;

    core.read(&*CHAT_SIGNAL)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to read CHAT_SIGNAL: {}", e)))
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
    // Parse channel_id as ChannelId
    let channel_id = channel_id_str
        .parse::<ChannelId>()
        .map_err(|_| AuraError::agent(format!("Invalid channel ID: {}", channel_id_str)))?;

    let core = app_core.read().await;
    let mut chat_state = core
        .read(&*CHAT_SIGNAL)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to read CHAT_SIGNAL: {}", e)))?;

    // Verify channel exists
    if chat_state.channel(&channel_id).is_none() {
        return Err(AuraError::agent(format!(
            "Channel not found: {}",
            channel_id
        )));
    }

    let now = timestamp_ms;

    // Create the action message with emote formatting
    // Content is prefixed with ACTION marker for UI rendering
    let message_id = format!("msg-{}-{}", channel_id, now);
    let message = Message {
        id: message_id.clone(),
        channel_id,
        sender_id: AuthorityId::default(),
        sender_name: "You".to_string(),
        // Format as emote: "* You action text"
        content: format!("* You {}", action),
        timestamp: now,
        reply_to: None,
        is_own: true,
        is_read: true,
    };

    // Apply message to state
    chat_state.apply_message(channel_id, message);

    core.emit(&*CHAT_SIGNAL, chat_state)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to emit CHAT_SIGNAL: {}", e)))?;

    Ok(message_id)
}

/// Invite a user to join the current channel
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
/// * `channel_id` - Channel to invite user to (use current selected if None)
/// * `message` - Optional invitation message
/// * `ttl_ms` - Optional time-to-live for the invitation
pub async fn invite_user_to_channel(
    app_core: &Arc<RwLock<AppCore>>,
    target_user_id: &str,
    channel_id: Option<&str>,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<String, AuraError> {
    // Determine channel ID - use provided or get current selected
    let channel = match channel_id {
        Some(id) => id.to_string(),
        None => {
            let chat_state = get_chat_state(app_core).await?;
            chat_state
                .selected_channel_id
                .map(|id| id.to_string())
                .ok_or_else(|| {
                    AuraError::agent("No channel selected. Please select a channel first.")
                })?
        }
    };

    // Parse target user ID as AuthorityId
    let receiver = target_user_id
        .parse::<AuthorityId>()
        .map_err(|e| AuraError::agent(format!("Invalid user ID: {}", e)))?;

    // Delegate to invitation workflow
    let invitation = crate::workflows::invitation::create_channel_invitation(
        app_core,
        receiver,
        channel.clone(),
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
        let mut core = AppCore::new(config).unwrap();
        core.init_signals().await.unwrap();
        let app_core = Arc::new(RwLock::new(core));

        let state = get_chat_state(&app_core).await.unwrap();
        assert!(state.channels.is_empty());
        assert!(state.messages.is_empty());
    }
}
