//! # Chat View State

use aura_core::identifiers::{AuthorityId, ChannelId};
use serde::{Deserialize, Serialize};

/// Type of channel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum ChannelType {
    /// Home-level messaging (group)
    #[default]
    Home,
    /// Direct message
    DirectMessage,
    /// Guardian chat
    Guardian,
    /// All channels (filter variant)
    All,
}

impl ChannelType {
    /// Alias for Home (group channels)
    pub const GROUP: ChannelType = ChannelType::Home;
}

/// A chat channel
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Channel {
    /// Channel identifier
    pub id: ChannelId,
    /// Channel name
    pub name: String,
    /// Channel topic/description
    pub topic: Option<String>,
    /// Channel type
    pub channel_type: ChannelType,
    /// Unread message count
    pub unread_count: u32,
    /// Whether this is a direct message channel
    pub is_dm: bool,
    /// Member count (for group channels)
    pub member_count: u32,
    /// Last message preview
    pub last_message: Option<String>,
    /// Last message timestamp (ms since epoch)
    pub last_message_time: Option<u64>,
    /// Last activity timestamp (ms since epoch)
    pub last_activity: u64,
}

/// A chat message
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Message {
    /// Message identifier (fact ID)
    pub id: String,
    /// Channel this message belongs to
    pub channel_id: ChannelId,
    /// Sender identifier
    pub sender_id: AuthorityId,
    /// Sender display name
    pub sender_name: String,
    /// Message content
    pub content: String,
    /// Timestamp (ms since epoch)
    pub timestamp: u64,
    /// ID of message being replied to
    pub reply_to: Option<String>,
    /// Whether the current user sent this message
    pub is_own: bool,
    /// Whether this message has been read
    pub is_read: bool,
}

/// Chat state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ChatState {
    /// All available channels
    pub channels: Vec<Channel>,
    /// Currently selected channel ID
    pub selected_channel_id: Option<ChannelId>,
    /// Messages in the selected channel
    pub messages: Vec<Message>,
    /// Total unread count across all channels
    pub total_unread: u32,
    /// Whether more messages are loading
    pub loading_more: bool,
    /// Whether there are more messages to load
    pub has_more: bool,
}

impl ChatState {
    /// Maximum number of messages retained in-memory for the active channel.
    const MAX_ACTIVE_MESSAGES: usize = 500;
    /// Get channel by ID
    pub fn channel(&self, id: &ChannelId) -> Option<&Channel> {
        self.channels.iter().find(|c| c.id == *id)
    }

    /// Get mutable channel by ID
    pub fn channel_mut(&mut self, id: &ChannelId) -> Option<&mut Channel> {
        self.channels.iter_mut().find(|c| c.id == *id)
    }

    /// Get unread count for a channel
    pub fn unread_count(&self, channel_id: &ChannelId) -> u32 {
        self.channel(channel_id)
            .map(|c| c.unread_count)
            .unwrap_or(0)
    }

    /// Ensure a valid selection when channels exist.
    pub fn ensure_selection(&mut self) {
        if self.channels.is_empty() {
            self.selected_channel_id = None;
            self.messages.clear();
            return;
        }

        let has_selected = self
            .selected_channel_id
            .as_ref()
            .map(|id| self.channel(id).is_some())
            .unwrap_or(false);

        if !has_selected {
            let first_id = self.channels[0].id;
            self.select_channel(Some(first_id));
        }
    }

    /// Add a new channel
    pub fn add_channel(&mut self, channel: Channel) {
        // Avoid duplicates
        if self.channel(&channel.id).is_none() {
            self.channels.push(channel);
        }
        self.ensure_selection();
    }

    /// Remove a channel by ID
    pub fn remove_channel(&mut self, channel_id: &ChannelId) {
        self.channels.retain(|c| c.id != *channel_id);
        // Clear messages if this was the selected channel
        if self.selected_channel_id.as_ref() == Some(channel_id) {
            self.selected_channel_id = None;
            self.messages.clear();
        }

        self.ensure_selection();
    }

    /// Mark a channel as joined (increment member count)
    pub fn mark_channel_joined(&mut self, channel_id: &ChannelId) {
        if let Some(channel) = self.channel_mut(channel_id) {
            channel.member_count = channel.member_count.saturating_add(1);
        }
    }

    /// Mark a channel as left (decrement member count)
    pub fn mark_channel_left(&mut self, channel_id: &ChannelId) {
        if let Some(channel) = self.channel_mut(channel_id) {
            channel.member_count = channel.member_count.saturating_sub(1);
        }
    }

    /// Update channel topic
    pub fn update_topic(&mut self, channel_id: &ChannelId, topic: String) {
        if let Some(channel) = self.channel_mut(channel_id) {
            channel.topic = Some(topic);
        }
    }

    /// Apply a new message to the state
    pub fn apply_message(&mut self, channel_id: ChannelId, message: Message) {
        // Check if this is the selected channel before mutable borrow
        let is_selected = self.selected_channel_id.as_ref() == Some(&channel_id);
        let should_increment_unread = !message.is_own && !is_selected;

        // Update channel metadata
        if let Some(channel) = self.channel_mut(&channel_id) {
            channel.last_message = Some(message.content.clone());
            channel.last_message_time = Some(message.timestamp);
            channel.last_activity = message.timestamp;

            // Increment unread if not own message and not selected channel
            if should_increment_unread {
                channel.unread_count = channel.unread_count.saturating_add(1);
            }
        }

        // Update total unread after channel borrow is released
        if should_increment_unread {
            self.total_unread = self.total_unread.saturating_add(1);
        }

        // Add message to list if this is the selected channel
        if is_selected {
            // Avoid duplicates
            if !self.messages.iter().any(|m| m.id == message.id) {
                self.messages.push(message);
                if self.messages.len() > Self::MAX_ACTIVE_MESSAGES {
                    let overflow = self.messages.len() - Self::MAX_ACTIVE_MESSAGES;
                    self.messages.drain(0..overflow);
                }
            }
        }
    }

    /// Select a channel and load its messages
    pub fn select_channel(&mut self, channel_id: Option<ChannelId>) {
        let mut next = channel_id;
        if next.is_none() && !self.channels.is_empty() {
            next = Some(self.channels[0].id);
        }

        if let Some(id) = &next {
            if self.channel(id).is_none() && !self.channels.is_empty() {
                next = Some(self.channels[0].id);
            }
        }

        if self.selected_channel_id != next {
            // Clear old messages
            self.messages.clear();
            self.selected_channel_id = next;

            // Mark as read - first get the unread count, then update both fields
            if let Some(id) = &next {
                // Get the unread count first (immutable borrow)
                let unread_to_subtract = self.channel(id).map(|c| c.unread_count).unwrap_or(0);

                // Update total_unread before mutable borrow
                self.total_unread = self.total_unread.saturating_sub(unread_to_subtract);

                // Now update the channel's unread count
                if let Some(channel) = self.channel_mut(id) {
                    channel.unread_count = 0;
                }
            }
        }
    }

    /// Mark a specific message as read by its ID
    ///
    /// Returns true if the message was found and marked as read,
    /// false if the message was not found in current messages.
    pub fn mark_message_read(&mut self, message_id: &str) -> bool {
        if let Some(message) = self.messages.iter_mut().find(|m| m.id == message_id) {
            if !message.is_read {
                message.is_read = true;
                return true;
            }
        }
        false
    }

    /// Decrement unread count for a channel (called when a message is read)
    pub fn decrement_unread(&mut self, channel_id: &ChannelId) {
        if let Some(channel) = self.channel_mut(channel_id) {
            if channel.unread_count > 0 {
                channel.unread_count = channel.unread_count.saturating_sub(1);
                self.total_unread = self.total_unread.saturating_sub(1);
            }
        }
    }

    /// Get mutable reference to a message by ID
    pub fn message_mut(&mut self, message_id: &str) -> Option<&mut Message> {
        self.messages.iter_mut().find(|m| m.id == message_id)
    }

    /// Remove a message by ID
    pub fn remove_message(&mut self, message_id: &str) {
        self.messages.retain(|m| m.id != message_id);
    }
}
