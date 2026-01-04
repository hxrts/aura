//! # Chat View State

use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Message Delivery Status
// ============================================================================

/// Message delivery status for tracking message lifecycle
///
/// Tracks the progression of a message from sending to read receipt.
/// This is portable across all frontends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum MessageDeliveryStatus {
    /// Message is being sent (not yet acknowledged)
    Sending,
    /// Message was sent and acknowledged by the network
    #[default]
    Sent,
    /// Message was delivered to recipient's device (before read)
    Delivered,
    /// Message was read by the recipient
    Read,
    /// Message delivery failed (with retry available)
    Failed,
}

impl MessageDeliveryStatus {
    /// Get the status indicator character for display
    #[must_use]
    pub fn indicator(&self) -> &'static str {
        match self {
            Self::Sending => "◐",    // Half-filled circle (pending)
            Self::Sent => "✓",       // Single check (gray)
            Self::Delivered => "✓✓", // Double check (gray)
            Self::Read => "✓✓",      // Double check (blue) - color applied by frontend
            Self::Failed => "✗",     // X mark
        }
    }

    /// Get a short description for the status
    #[must_use]
    pub fn description(&self) -> &'static str {
        match self {
            Self::Sending => "Sending...",
            Self::Sent => "Sent",
            Self::Delivered => "Delivered",
            Self::Read => "Read",
            Self::Failed => "Failed",
        }
    }

    /// Get a lowercase label for logging/serialization
    #[must_use]
    pub fn label_lowercase(&self) -> &'static str {
        match self {
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Delivered => "delivered",
            Self::Read => "read",
            Self::Failed => "failed",
        }
    }

    /// Whether the message has reached the recipient's device
    #[must_use]
    pub fn is_delivered(&self) -> bool {
        matches!(self, Self::Delivered | Self::Read)
    }

    /// Whether the message has been read by the recipient
    #[must_use]
    pub fn is_read(&self) -> bool {
        matches!(self, Self::Read)
    }

    /// Whether the message is still pending (not yet confirmed delivered)
    #[must_use]
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Sending | Self::Sent)
    }

    /// Whether the message failed to send
    #[must_use]
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed)
    }

    /// Whether the message can be retried (only failed messages)
    #[must_use]
    pub fn can_retry(&self) -> bool {
        matches!(self, Self::Failed)
    }

    /// Whether the message has been successfully sent (any non-failed, non-sending state)
    #[must_use]
    pub fn is_sent(&self) -> bool {
        matches!(self, Self::Sent | Self::Delivered | Self::Read)
    }
}

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
    /// Relational context for this channel (when known).
    #[serde(default)]
    pub context_id: Option<ContextId>,
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
    /// Known channel members (excluding self).
    ///
    /// This is populated from UI flows or runtime-backed membership facts.
    #[serde(default)]
    pub member_ids: Vec<AuthorityId>,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Delivery status for own messages (Sending → Sent → Delivered → Read)
    #[serde(default)]
    pub delivery_status: MessageDeliveryStatus,
    /// Channel epoch when message was sent (for consensus finalization tracking)
    #[serde(default)]
    pub epoch_hint: Option<u32>,
    /// Whether this message has been finalized by consensus (A3)
    #[serde(default)]
    pub is_finalized: bool,
}

/// Chat state
///
/// Note: This type does NOT track channel selection. Selection is UI state
/// that belongs in the frontend (TUI, mobile app, etc.). All message operations
/// require an explicit channel_id to avoid race conditions between UI navigation
/// and async operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ChatState {
    /// All available channels
    pub channels: Vec<Channel>,
    /// Per-channel message storage
    #[serde(default)]
    channel_messages: HashMap<ChannelId, Vec<Message>>,
    /// Total unread count across all channels
    pub total_unread: u32,
    /// Whether more messages are loading (per-channel state managed by caller)
    pub loading_more: bool,
    /// Whether there are more messages to load (per-channel state managed by caller)
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

    /// Get messages for a specific channel
    ///
    /// Returns an empty slice if the channel has no messages or doesn't exist.
    pub fn messages_for_channel(&self, channel_id: &ChannelId) -> &[Message] {
        self.channel_messages
            .get(channel_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all messages across all channels (primarily for test backwards compatibility).
    /// Returns messages in arbitrary order - use `messages_for_channel()` for production code.
    #[must_use]
    pub fn all_messages(&self) -> Vec<&Message> {
        self.channel_messages.values().flatten().collect()
    }

    /// Get total message count across all channels
    #[must_use]
    pub fn message_count(&self) -> usize {
        self.channel_messages.values().map(|v| v.len()).sum()
    }

    /// Add a new channel
    pub fn add_channel(&mut self, channel: Channel) {
        // Avoid duplicates
        if self.channel(&channel.id).is_none() {
            self.channels.push(channel);
        }
    }

    /// Remove a channel by ID
    pub fn remove_channel(&mut self, channel_id: &ChannelId) {
        self.channels.retain(|c| c.id != *channel_id);
        // Remove messages from per-channel cache
        self.channel_messages.remove(channel_id);
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
    ///
    /// Note: Unread counting is caller's responsibility since ChatState doesn't
    /// track selection. The caller should call `increment_unread()` if the channel
    /// is not currently selected in the UI.
    pub fn apply_message(&mut self, channel_id: ChannelId, message: Message) {
        // Update channel metadata
        if let Some(channel) = self.channel_mut(&channel_id) {
            channel.last_message = Some(message.content.clone());
            channel.last_message_time = Some(message.timestamp);
            channel.last_activity = message.timestamp;
        }

        // Store message in per-channel cache
        let channel_msgs = self.channel_messages.entry(channel_id).or_default();
        if !channel_msgs.iter().any(|m| m.id == message.id) {
            channel_msgs.push(message);
            if channel_msgs.len() > Self::MAX_ACTIVE_MESSAGES {
                let overflow = channel_msgs.len() - Self::MAX_ACTIVE_MESSAGES;
                channel_msgs.drain(0..overflow);
            }
        }
    }

    /// Increment unread count for a channel (call when message arrives for non-selected channel)
    pub fn increment_unread(&mut self, channel_id: &ChannelId) {
        if let Some(channel) = self.channel_mut(channel_id) {
            channel.unread_count = channel.unread_count.saturating_add(1);
        }
        self.total_unread = self.total_unread.saturating_add(1);
    }

    /// Clear unread count for a channel (call when channel is selected/viewed)
    pub fn clear_unread(&mut self, channel_id: &ChannelId) {
        if let Some(channel) = self.channel_mut(channel_id) {
            let count = channel.unread_count;
            channel.unread_count = 0;
            self.total_unread = self.total_unread.saturating_sub(count);
        }
    }

    /// Mark a specific message as read by its ID in a specific channel
    ///
    /// Returns true if the message was found and marked as read,
    /// false if the message was not found.
    pub fn mark_message_read(&mut self, channel_id: &ChannelId, message_id: &str) -> bool {
        if let Some(msgs) = self.channel_messages.get_mut(channel_id) {
            if let Some(message) = msgs.iter_mut().find(|m| m.id == message_id) {
                if !message.is_read {
                    message.is_read = true;
                    return true;
                }
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

    /// Get mutable reference to a message by ID in a specific channel
    pub fn message_mut(&mut self, channel_id: &ChannelId, message_id: &str) -> Option<&mut Message> {
        self.channel_messages
            .get_mut(channel_id)
            .and_then(|msgs| msgs.iter_mut().find(|m| m.id == message_id))
    }

    /// Remove a message by ID from a specific channel
    pub fn remove_message(&mut self, channel_id: &ChannelId, message_id: &str) {
        if let Some(msgs) = self.channel_messages.get_mut(channel_id) {
            msgs.retain(|m| m.id != message_id);
        }
    }

    /// Mark a message as delivered (update delivery_status from Sent to Delivered)
    ///
    /// This is called when a MessageDelivered fact is received, indicating that
    /// the recipient's device has received the message. Only updates own messages
    /// that are currently in Sent status.
    ///
    /// Returns true if the message was found and updated.
    pub fn mark_delivered(&mut self, message_id: &str) -> bool {
        for msgs in self.channel_messages.values_mut() {
            if let Some(msg) = msgs.iter_mut().find(|m| m.id == message_id && m.is_own) {
                if msg.delivery_status == MessageDeliveryStatus::Sent {
                    msg.delivery_status = MessageDeliveryStatus::Delivered;
                    return true;
                }
            }
        }
        false
    }

    /// Mark a message as read by recipient (update delivery_status to Read)
    ///
    /// This is called when a MessageRead fact is received, indicating that
    /// the recipient has viewed the message. Only updates own messages.
    ///
    /// Returns true if the message was found and updated.
    pub fn mark_read_by_recipient(&mut self, message_id: &str) -> bool {
        for msgs in self.channel_messages.values_mut() {
            if let Some(msg) = msgs.iter_mut().find(|m| m.id == message_id && m.is_own) {
                if msg.delivery_status != MessageDeliveryStatus::Read {
                    msg.delivery_status = MessageDeliveryStatus::Read;
                    return true;
                }
            }
        }
        false
    }

    /// Mark a message as finalized by consensus (A3 status)
    ///
    /// This is called when consensus confirms the message has been durably
    /// committed with 2f+1 witnesses.
    ///
    /// Returns true if the message was found and updated.
    pub fn mark_finalized(&mut self, message_id: &str) -> bool {
        for msgs in self.channel_messages.values_mut() {
            if let Some(msg) = msgs.iter_mut().find(|m| m.id == message_id) {
                if !msg.is_finalized {
                    msg.is_finalized = true;
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delivery_status_indicators() {
        assert_eq!(MessageDeliveryStatus::Sending.indicator(), "◐");
        assert_eq!(MessageDeliveryStatus::Sent.indicator(), "✓");
        assert_eq!(MessageDeliveryStatus::Delivered.indicator(), "✓✓");
        assert_eq!(MessageDeliveryStatus::Read.indicator(), "✓✓");
        assert_eq!(MessageDeliveryStatus::Failed.indicator(), "✗");
    }

    #[test]
    fn test_delivery_status_descriptions() {
        assert_eq!(MessageDeliveryStatus::Sending.description(), "Sending...");
        assert_eq!(MessageDeliveryStatus::Sent.description(), "Sent");
        assert_eq!(MessageDeliveryStatus::Delivered.description(), "Delivered");
        assert_eq!(MessageDeliveryStatus::Read.description(), "Read");
        assert_eq!(MessageDeliveryStatus::Failed.description(), "Failed");
    }

    #[test]
    fn test_is_delivered() {
        assert!(!MessageDeliveryStatus::Sending.is_delivered());
        assert!(!MessageDeliveryStatus::Sent.is_delivered());
        assert!(MessageDeliveryStatus::Delivered.is_delivered());
        assert!(MessageDeliveryStatus::Read.is_delivered());
        assert!(!MessageDeliveryStatus::Failed.is_delivered());
    }

    #[test]
    fn test_is_read() {
        assert!(!MessageDeliveryStatus::Sending.is_read());
        assert!(!MessageDeliveryStatus::Sent.is_read());
        assert!(!MessageDeliveryStatus::Delivered.is_read());
        assert!(MessageDeliveryStatus::Read.is_read());
        assert!(!MessageDeliveryStatus::Failed.is_read());
    }

    #[test]
    fn test_is_pending() {
        assert!(MessageDeliveryStatus::Sending.is_pending());
        assert!(MessageDeliveryStatus::Sent.is_pending());
        assert!(!MessageDeliveryStatus::Delivered.is_pending());
        assert!(!MessageDeliveryStatus::Read.is_pending());
        assert!(!MessageDeliveryStatus::Failed.is_pending());
    }

    #[test]
    fn test_is_failed() {
        assert!(!MessageDeliveryStatus::Sending.is_failed());
        assert!(!MessageDeliveryStatus::Sent.is_failed());
        assert!(!MessageDeliveryStatus::Delivered.is_failed());
        assert!(!MessageDeliveryStatus::Read.is_failed());
        assert!(MessageDeliveryStatus::Failed.is_failed());
    }

    #[test]
    fn test_can_retry() {
        assert!(!MessageDeliveryStatus::Sending.can_retry());
        assert!(!MessageDeliveryStatus::Sent.can_retry());
        assert!(!MessageDeliveryStatus::Delivered.can_retry());
        assert!(!MessageDeliveryStatus::Read.can_retry());
        assert!(MessageDeliveryStatus::Failed.can_retry());
    }

    #[test]
    fn test_is_sent() {
        assert!(!MessageDeliveryStatus::Sending.is_sent());
        assert!(MessageDeliveryStatus::Sent.is_sent());
        assert!(MessageDeliveryStatus::Delivered.is_sent());
        assert!(MessageDeliveryStatus::Read.is_sent());
        assert!(!MessageDeliveryStatus::Failed.is_sent());
    }

    #[test]
    fn test_delivery_status_labels() {
        assert_eq!(MessageDeliveryStatus::Sending.label_lowercase(), "sending");
        assert_eq!(MessageDeliveryStatus::Sent.label_lowercase(), "sent");
        assert_eq!(
            MessageDeliveryStatus::Delivered.label_lowercase(),
            "delivered"
        );
        assert_eq!(MessageDeliveryStatus::Read.label_lowercase(), "read");
        assert_eq!(MessageDeliveryStatus::Failed.label_lowercase(), "failed");
    }
}
