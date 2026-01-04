//! # Chat View State

use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::collection::DomainCollection;

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
    /// Last finalized channel epoch (for consensus finalization tracking)
    ///
    /// Messages with epoch_hint <= this value are considered finalized.
    #[serde(default)]
    pub last_finalized_epoch: u32,
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

/// Custom serde module for backward-compatible channel serialization.
///
/// Serializes DomainCollection as Vec for wire format compatibility.
mod channels_serde {
    use super::{Channel, ChannelId, DomainCollection};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(
        channels: &DomainCollection<ChannelId, Channel>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let vec: Vec<&Channel> = channels.all().collect();
        vec.serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<DomainCollection<ChannelId, Channel>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<Channel> = Vec::deserialize(deserializer)?;
        Ok(DomainCollection::from_iter(vec.into_iter().map(|c| (c.id.clone(), c))))
    }
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
    /// All available channels (private, use accessor methods)
    #[serde(with = "channels_serde")]
    channels: DomainCollection<ChannelId, Channel>,
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

    // ========================================================================
    // Factory Methods
    // ========================================================================

    /// Create a ChatState from a list of channels
    #[must_use]
    pub fn from_channels(channels: impl IntoIterator<Item = Channel>) -> Self {
        Self {
            channels: DomainCollection::from_iter(
                channels.into_iter().map(|c| (c.id.clone(), c)),
            ),
            channel_messages: HashMap::new(),
            total_unread: 0,
            loading_more: false,
            has_more: false,
        }
    }

    // ========================================================================
    // Channel Accessors
    // ========================================================================

    /// Get channel by ID
    #[must_use]
    pub fn channel(&self, id: &ChannelId) -> Option<&Channel> {
        self.channels.get(id)
    }

    /// Get mutable channel by ID
    pub fn channel_mut(&mut self, id: &ChannelId) -> Option<&mut Channel> {
        self.channels.get_mut(id)
    }

    /// Check if a channel exists
    #[must_use]
    pub fn has_channel(&self, id: &ChannelId) -> bool {
        self.channels.contains(id)
    }

    /// Get all channels as an iterator
    #[must_use]
    pub fn all_channels(&self) -> impl Iterator<Item = &Channel> {
        self.channels.all()
    }

    /// Get the number of channels
    #[must_use]
    pub fn channel_count(&self) -> usize {
        self.channels.count()
    }

    /// Check if there are no channels
    #[must_use]
    pub fn channels_is_empty(&self) -> bool {
        self.channels.is_empty()
    }

    /// Get the first channel (for default selection)
    #[must_use]
    pub fn first_channel(&self) -> Option<&Channel> {
        self.channels.all().next()
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

    /// Add or update a channel
    pub fn add_channel(&mut self, channel: Channel) {
        self.channels.apply(channel.id.clone(), channel);
    }

    /// Remove a channel by ID
    pub fn remove_channel(&mut self, channel_id: &ChannelId) {
        self.channels.remove(channel_id);
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

    /// Mark all messages in a channel up to a given epoch as finalized.
    ///
    /// This is called when a `CommittedChannelEpochBump` fact is observed.
    /// Messages with epoch_hint <= parent_epoch are considered finalized.
    ///
    /// Updates the channel's last_finalized_epoch and marks individual messages.
    /// Returns the number of messages that were updated.
    pub fn mark_finalized_up_to_epoch(&mut self, channel_id: &ChannelId, epoch: u32) -> u32 {
        // Update the channel's finalized epoch
        if let Some(channel) = self.channel_mut(channel_id) {
            if epoch > channel.last_finalized_epoch {
                channel.last_finalized_epoch = epoch;
            }
        }

        // Mark all messages with epoch_hint <= epoch as finalized
        let mut count = 0u32;
        if let Some(msgs) = self.channel_messages.get_mut(channel_id) {
            for msg in msgs.iter_mut() {
                if let Some(hint) = msg.epoch_hint {
                    if hint <= epoch && !msg.is_finalized {
                        msg.is_finalized = true;
                        count += 1;
                    }
                }
            }
        }
        count
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

    fn make_test_message(id: &str, epoch_hint: Option<u32>) -> Message {
        Message {
            id: id.to_string(),
            channel_id: ChannelId::from_bytes([1u8; 32]),
            sender_id: AuthorityId::new_from_entropy([2u8; 32]),
            sender_name: "Test".to_string(),
            content: "Hello".to_string(),
            timestamp: 1000,
            reply_to: None,
            is_own: true,
            is_read: false,
            delivery_status: MessageDeliveryStatus::Sent,
            epoch_hint,
            is_finalized: false,
        }
    }

    fn make_test_channel(id: ChannelId) -> Channel {
        Channel {
            id,
            context_id: None,
            name: "Test Channel".to_string(),
            topic: None,
            channel_type: ChannelType::Home,
            unread_count: 0,
            is_dm: false,
            member_ids: Vec::new(),
            member_count: 1,
            last_message: None,
            last_message_time: None,
            last_activity: 0,
            last_finalized_epoch: 0,
        }
    }

    #[test]
    fn test_mark_finalized_up_to_epoch_marks_messages() {
        let mut state = ChatState::default();
        let channel_id = ChannelId::from_bytes([1u8; 32]);
        state.add_channel(make_test_channel(channel_id));

        // Add messages with various epoch hints
        let mut msg1 = make_test_message("msg1", Some(1));
        msg1.channel_id = channel_id;
        let mut msg2 = make_test_message("msg2", Some(2));
        msg2.channel_id = channel_id;
        let mut msg3 = make_test_message("msg3", Some(3));
        msg3.channel_id = channel_id;
        let mut msg4 = make_test_message("msg4", None); // No epoch hint
        msg4.channel_id = channel_id;

        state.apply_message(channel_id, msg1);
        state.apply_message(channel_id, msg2);
        state.apply_message(channel_id, msg3);
        state.apply_message(channel_id, msg4);

        // Finalize up to epoch 2 (should mark msg1 and msg2)
        let count = state.mark_finalized_up_to_epoch(&channel_id, 2);
        assert_eq!(count, 2);

        // Check message finalization states
        let messages = state.channel_messages.get(&channel_id).unwrap();
        assert!(
            messages.iter().find(|m| m.id == "msg1").unwrap().is_finalized,
            "msg1 should be finalized"
        );
        assert!(
            messages.iter().find(|m| m.id == "msg2").unwrap().is_finalized,
            "msg2 should be finalized"
        );
        assert!(
            !messages.iter().find(|m| m.id == "msg3").unwrap().is_finalized,
            "msg3 should NOT be finalized (epoch 3 > 2)"
        );
        assert!(
            !messages.iter().find(|m| m.id == "msg4").unwrap().is_finalized,
            "msg4 should NOT be finalized (no epoch hint)"
        );
    }

    #[test]
    fn test_mark_finalized_updates_channel_epoch() {
        let mut state = ChatState::default();
        let channel_id = ChannelId::from_bytes([1u8; 32]);
        state.add_channel(make_test_channel(channel_id));

        assert_eq!(state.channel(&channel_id).unwrap().last_finalized_epoch, 0);

        state.mark_finalized_up_to_epoch(&channel_id, 5);
        assert_eq!(state.channel(&channel_id).unwrap().last_finalized_epoch, 5);

        // Should update to higher epoch
        state.mark_finalized_up_to_epoch(&channel_id, 10);
        assert_eq!(state.channel(&channel_id).unwrap().last_finalized_epoch, 10);

        // Should NOT update to lower epoch
        state.mark_finalized_up_to_epoch(&channel_id, 7);
        assert_eq!(
            state.channel(&channel_id).unwrap().last_finalized_epoch,
            10,
            "Should not regress to lower epoch"
        );
    }

    #[test]
    fn test_mark_finalized_idempotent() {
        let mut state = ChatState::default();
        let channel_id = ChannelId::from_bytes([1u8; 32]);
        state.add_channel(make_test_channel(channel_id));

        let mut msg = make_test_message("msg1", Some(1));
        msg.channel_id = channel_id;
        state.apply_message(channel_id, msg);

        // First finalization
        let count1 = state.mark_finalized_up_to_epoch(&channel_id, 5);
        assert_eq!(count1, 1);

        // Second finalization of same messages should return 0
        let count2 = state.mark_finalized_up_to_epoch(&channel_id, 5);
        assert_eq!(count2, 0, "Already-finalized messages should not be counted again");
    }

    #[test]
    fn test_mark_finalized_unknown_channel() {
        let mut state = ChatState::default();
        let unknown_channel = ChannelId::from_bytes([99u8; 32]);

        // Should not panic on unknown channel
        let count = state.mark_finalized_up_to_epoch(&unknown_channel, 10);
        assert_eq!(count, 0);
    }
}
