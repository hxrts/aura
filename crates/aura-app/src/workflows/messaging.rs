//! Messaging Workflow - Portable Business Logic
//!
//! This module contains messaging operations that are portable across all frontends.
//! It follows the reactive signal pattern and emits CHAT_SIGNAL updates.

use crate::{
    signal_defs::CHAT_SIGNAL,
    views::chat::{Channel, ChannelType, ChatState, Message},
    AppCore,
};
use aura_core::{effects::reactive::ReactiveEffects, AuraError};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Send a direct message to a contact
///
/// **What it does**: Sends a message in a DM channel with the contact
/// **Returns**: DM channel ID
/// **Signal pattern**: Emits CHAT_SIGNAL after message is added
///
/// This operation:
/// 1. Creates DM channel if it doesn't exist
/// 2. Adds message to chat state
/// 3. Emits CHAT_SIGNAL for UI updates
///
/// **Note**: Full implementation would use Intent::SendMessage for persistence.
/// Currently updates chat state locally for UI responsiveness.
pub async fn send_direct_message(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
    content: &str,
) -> Result<String, AuraError> {
    let dm_channel_id = format!("dm:{}", target);

    let core = app_core.read().await;
    let mut chat_state = core.read(&*CHAT_SIGNAL).await.unwrap_or_default();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    // Ensure the DM channel exists (create if needed)
    if !chat_state.channels.iter().any(|c| c.id == dm_channel_id) {
        let dm_channel = Channel {
            id: dm_channel_id.clone(),
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
    chat_state.selected_channel_id = Some(dm_channel_id.clone());

    // Create the message with deterministic ID
    let message = Message {
        id: format!("msg-{}-{}", dm_channel_id, now),
        channel_id: dm_channel_id.clone(),
        sender_id: "self".to_string(),
        sender_name: "You".to_string(),
        content: content.to_string(),
        timestamp: now,
        reply_to: None,
        is_own: true,
        is_read: true,
    };

    // Apply message to state
    chat_state.apply_message(dm_channel_id.clone(), message);

    // Emit updated state
    core.emit(&*CHAT_SIGNAL, chat_state)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to emit chat signal: {}", e)))?;

    Ok(dm_channel_id)
}

/// Start a direct chat with a contact
///
/// **What it does**: Creates a DM channel and selects it
/// **Returns**: DM channel ID
/// **Signal pattern**: Emits CHAT_SIGNAL after channel is created
///
/// This operation:
/// 1. Gets contact name from ViewState
/// 2. Creates DM channel if it doesn't exist
/// 3. Selects the channel for active conversation
/// 4. Emits CHAT_SIGNAL for UI updates
pub async fn start_direct_chat(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: &str,
) -> Result<String, AuraError> {
    let dm_channel_id = format!("dm:{}", contact_id);

    // Get contact name from ViewState for the channel name
    let contact_name = {
        let core = app_core.read().await;
        let snapshot = core.snapshot();
        snapshot
            .contacts
            .contacts
            .iter()
            .find(|c| c.id == *contact_id)
            .map(|c| c.petname.clone())
            .unwrap_or_else(|| format!("DM with {}", &contact_id[..8.min(contact_id.len())]))
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    // Create the DM channel
    let dm_channel = Channel {
        id: dm_channel_id.clone(),
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

    // Add channel to ChatState and select it
    let core = app_core.read().await;
    let mut chat_state = core.read(&*CHAT_SIGNAL).await.unwrap_or_default();

    // Add the DM channel (add_channel avoids duplicates)
    chat_state.add_channel(dm_channel);

    // Select this channel (don't clear messages - retain history)
    chat_state.selected_channel_id = Some(dm_channel_id.clone());

    // Emit updated state
    core.emit(&*CHAT_SIGNAL, chat_state)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to emit chat signal: {}", e)))?;

    Ok(dm_channel_id)
}

/// Get current chat state
///
/// **What it does**: Reads chat state from CHAT_SIGNAL
/// **Returns**: Current chat state with channels and messages
/// **Signal pattern**: Read-only operation (no emission)
pub async fn get_chat_state(app_core: &Arc<RwLock<AppCore>>) -> ChatState {
    let core = app_core.read().await;

    match core.read(&*CHAT_SIGNAL).await {
        Ok(state) => state,
        Err(_) => ChatState::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[tokio::test]
    async fn test_get_chat_state_default() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let state = get_chat_state(&app_core).await;
        assert!(state.channels.is_empty());
        assert!(state.messages.is_empty());
    }
}
