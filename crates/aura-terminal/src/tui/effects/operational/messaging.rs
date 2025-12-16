//! Messaging command handlers
//!
//! Handlers for SendDirectMessage, StartDirectChat, SendAction, InviteUser.

use std::sync::Arc;

use aura_app::signal_defs::CHAT_SIGNAL;
use aura_app::views::chat::{Channel, ChannelType};
use aura_app::AppCore;
use aura_core::effects::reactive::ReactiveEffects;
use tokio::sync::RwLock;

use super::types::{OpResponse, OpResult};
use super::EffectCommand;

/// Handle messaging commands
pub async fn handle_messaging(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::SendDirectMessage { target, content } => {
            // Create the DM channel ID based on target
            let dm_channel_id = format!("dm:{}", target);
            tracing::info!(
                "Sending direct message to {} in channel {}",
                target,
                dm_channel_id
            );

            // Get current chat state and add a message
            // Note: Full implementation would use Intent::SendMessage
            // For now, emit signal update to refresh UI
            let core = app_core.read().await;
            if let Ok(mut chat_state) = core.read(&*CHAT_SIGNAL).await {
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
                    tracing::info!("Created DM channel: {}", dm_channel_id);
                }

                // Select this channel so messages are visible
                // (apply_message only adds to messages list when channel is selected)
                chat_state.selected_channel_id = Some(dm_channel_id.clone());

                // Create the message with deterministic ID based on channel and timestamp
                let message = aura_app::views::chat::Message {
                    id: format!("msg-{}-{}", dm_channel_id, now),
                    channel_id: dm_channel_id.clone(),
                    sender_id: "self".to_string(),
                    sender_name: "You".to_string(),
                    content: content.clone(),
                    timestamp: now,
                    reply_to: None,
                    is_own: true,
                    is_read: true,
                };

                // Apply message to state (updates channel metadata and adds to messages
                // list if this channel is currently selected)
                chat_state.apply_message(dm_channel_id.clone(), message);

                // Emit updated state
                let _ = core.emit(&*CHAT_SIGNAL, chat_state).await;
            }

            Some(Ok(OpResponse::Data(format!(
                "Message sent to DM channel: {}",
                dm_channel_id
            ))))
        }

        EffectCommand::StartDirectChat { contact_id } => {
            // Create a DM channel for this contact
            let dm_channel_id = format!("dm:{}", contact_id);

            tracing::info!(
                "Starting direct chat with contact {} (channel: {})",
                contact_id,
                dm_channel_id
            );

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
                    .unwrap_or_else(|| {
                        format!("DM with {}", &contact_id[..8.min(contact_id.len())])
                    })
            };

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
                last_activity: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            };

            // Add channel to ChatState and select it
            let core = app_core.read().await;
            if let Ok(mut chat_state) = core.read(&*CHAT_SIGNAL).await {
                // Add the DM channel (add_channel avoids duplicates)
                chat_state.add_channel(dm_channel);

                // Select this channel (don't clear messages - retain history)
                chat_state.selected_channel_id = Some(dm_channel_id.clone());

                // Emit updated state
                let _ = core.emit(&*CHAT_SIGNAL, chat_state).await;
                tracing::info!("DM channel created and selected: {}", dm_channel_id);
            }

            Some(Ok(OpResponse::Data(format!(
                "Started DM chat: {}",
                dm_channel_id
            ))))
        }

        EffectCommand::SendAction {
            channel: _,
            action: _,
        } => {
            // IRC-style /me action
            Some(Ok(OpResponse::Ok))
        }

        EffectCommand::InviteUser { target: _ } => Some(Ok(OpResponse::Ok)),

        _ => None,
    }
}
