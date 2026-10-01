#![allow(missing_docs)]

use super::delivery::MessageDeliveryStatus;
use super::models::{Channel, Message};
use super::serde_support::channel_id_keyed_map;
use aura_core::types::identifiers::ChannelId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Chat state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ChatState {
    /// All available channels (keyed by ChannelId for O(1) lookup).
    #[serde(with = "channel_id_keyed_map", default)]
    pub(crate) channels: HashMap<ChannelId, Channel>,
    /// Per-channel message storage.
    #[serde(default)]
    pub(crate) channel_messages: HashMap<ChannelId, Vec<Message>>,
    /// Total unread count across all channels.
    pub total_unread: u32,
    /// Whether more messages are loading (per-channel state managed by caller).
    pub loading_more: bool,
    /// Whether there are more messages to load (per-channel state managed by caller).
    pub has_more: bool,
}

impl ChatState {
    const MAX_ACTIVE_MESSAGES: usize = 500;

    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn from_channels(channels: impl IntoIterator<Item = Channel>) -> Self {
        Self {
            channels: channels.into_iter().map(|c| (c.id, c)).collect(),
            ..Default::default()
        }
    }

    #[must_use]
    pub fn channel(&self, id: &ChannelId) -> Option<&Channel> {
        self.channels.get(id)
    }

    pub fn channel_mut(&mut self, id: &ChannelId) -> Option<&mut Channel> {
        self.channels.get_mut(id)
    }

    #[must_use]
    pub fn has_channel(&self, id: &ChannelId) -> bool {
        self.channels.contains_key(id)
    }

    pub fn all_channels(&self) -> impl Iterator<Item = &Channel> {
        self.channels.values()
    }

    pub fn all_channels_mut(&mut self) -> impl Iterator<Item = &mut Channel> {
        self.channels.values_mut()
    }

    #[must_use]
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }

    #[must_use]
    pub fn unread_count(&self, channel_id: &ChannelId) -> u32 {
        self.channel(channel_id)
            .map(|c| c.unread_count)
            .unwrap_or(0)
    }

    #[must_use]
    pub fn messages_for_channel(&self, channel_id: &ChannelId) -> &[Message] {
        self.channel_messages
            .get(channel_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    #[must_use]
    pub fn message_count(&self) -> usize {
        self.channel_messages.values().map(|v| v.len()).sum()
    }

    pub fn add_channel(&mut self, channel: Channel) {
        self.channels.entry(channel.id).or_insert(channel);
    }

    pub fn upsert_channel(&mut self, channel: Channel) {
        self.channels.insert(channel.id, channel);
    }

    pub fn rebind_channel_identity(&mut self, from: &ChannelId, mut canonical: Channel) {
        let canonical_id = canonical.id;
        if *from == canonical.id {
            self.upsert_channel(canonical);
            return;
        }

        let mut next_channels = self.channels.clone();
        let mut next_channel_messages = self.channel_messages.clone();

        if let Some(existing_canonical) = next_channels.remove(&canonical.id) {
            merge_channel_projection(&mut canonical, existing_canonical);
        }

        if let Some(previous) = next_channels.remove(from) {
            merge_channel_projection(&mut canonical, previous);
        }

        let mut merged_messages = next_channel_messages
            .remove(&canonical.id)
            .unwrap_or_default();
        if let Some(mut previous_messages) = next_channel_messages.remove(from) {
            for message in &mut previous_messages {
                message.channel_id = canonical.id;
            }
            for message in previous_messages {
                if !merged_messages
                    .iter()
                    .any(|existing| existing.id == message.id)
                {
                    merged_messages.push(message);
                }
            }
        }

        next_channels.insert(canonical_id, canonical);
        if !merged_messages.is_empty() {
            next_channel_messages.insert(canonical_id, merged_messages);
        }

        self.channels = next_channels;
        self.channel_messages = next_channel_messages;
    }

    pub fn remove_channel(&mut self, channel_id: &ChannelId) -> Option<Channel> {
        self.channel_messages.remove(channel_id);
        self.channels.remove(channel_id)
    }

    pub fn clear(&mut self) {
        self.channels.clear();
        self.channel_messages.clear();
        self.total_unread = 0;
    }

    pub fn mark_channel_joined(&mut self, channel_id: &ChannelId) {
        if let Some(channel) = self.channel_mut(channel_id) {
            channel.member_count = channel.member_count.saturating_add(1);
        }
    }

    pub fn mark_channel_left(&mut self, channel_id: &ChannelId) {
        if let Some(channel) = self.channel_mut(channel_id) {
            channel.member_count = channel.member_count.saturating_sub(1);
        }
    }

    pub fn update_topic(&mut self, channel_id: &ChannelId, topic: String) {
        if let Some(channel) = self.channel_mut(channel_id) {
            channel.topic = Some(topic);
        }
    }

    pub fn apply_message(&mut self, channel_id: ChannelId, message: Message) {
        let is_latest = self
            .channel_messages
            .get(&channel_id)
            .and_then(|messages| messages.last())
            .is_none_or(|last| message.timestamp >= last.timestamp);
        if let Some(channel) = self.channel_mut(&channel_id) {
            if is_latest {
                channel.last_message = Some(message.content.clone());
                channel.last_message_time = Some(message.timestamp);
                channel.last_activity = message.timestamp;
            }
        }

        let channel_msgs = self.channel_messages.entry(channel_id).or_default();
        if let Some(existing) = channel_msgs.iter_mut().find(|m| m.id == message.id) {
            // A message seen before its channel key arrived (e.g. on a newly
            // enrolled device) is replaced once it can be opened.
            if is_sealed_placeholder(&existing.content) && !is_sealed_placeholder(&message.content)
            {
                existing.content = message.content;
            }
        } else {
            // Messages can arrive out of order; keep them ordered by send time
            // (stable for equal timestamps).
            let position = channel_msgs.partition_point(|m| m.timestamp <= message.timestamp);
            channel_msgs.insert(position, message);
            if channel_msgs.len() > Self::MAX_ACTIVE_MESSAGES {
                let overflow = channel_msgs.len() - Self::MAX_ACTIVE_MESSAGES;
                channel_msgs.drain(0..overflow);
            }
        }
    }

    pub fn increment_unread(&mut self, channel_id: &ChannelId) {
        if let Some(channel) = self.channel_mut(channel_id) {
            channel.unread_count = channel.unread_count.saturating_add(1);
        }
        self.total_unread = self.total_unread.saturating_add(1);
    }

    pub fn clear_unread(&mut self, channel_id: &ChannelId) {
        if let Some(channel) = self.channel_mut(channel_id) {
            let count = channel.unread_count;
            channel.unread_count = 0;
            self.total_unread = self.total_unread.saturating_sub(count);
        }
    }

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

    pub fn decrement_unread(&mut self, channel_id: &ChannelId) {
        if let Some(channel) = self.channel_mut(channel_id) {
            if channel.unread_count > 0 {
                channel.unread_count = channel.unread_count.saturating_sub(1);
                self.total_unread = self.total_unread.saturating_sub(1);
            }
        }
    }

    pub fn message_mut(
        &mut self,
        channel_id: &ChannelId,
        message_id: &str,
    ) -> Option<&mut Message> {
        self.channel_messages
            .get_mut(channel_id)
            .and_then(|msgs| msgs.iter_mut().find(|m| m.id == message_id))
    }

    pub fn remove_message(&mut self, channel_id: &ChannelId, message_id: &str) {
        if let Some(msgs) = self.channel_messages.get_mut(channel_id) {
            msgs.retain(|m| m.id != message_id);
        }
    }

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

    pub fn mark_failed(&mut self, message_id: &str) -> bool {
        for msgs in self.channel_messages.values_mut() {
            if let Some(msg) = msgs.iter_mut().find(|m| m.id == message_id && m.is_own) {
                if msg.delivery_status != MessageDeliveryStatus::Failed {
                    msg.delivery_status = MessageDeliveryStatus::Failed;
                    return true;
                }
            }
        }
        false
    }

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

    pub fn mark_finalized_up_to_epoch(
        &mut self,
        channel_id: &ChannelId,
        epoch: u32,
    ) -> Option<u32> {
        let channel_exists = if let Some(channel) = self.channel_mut(channel_id) {
            if epoch > channel.last_finalized_epoch {
                channel.last_finalized_epoch = epoch;
            }
            true
        } else {
            false
        };

        if !channel_exists {
            return None;
        }

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
        Some(count)
    }
}

fn merge_channel_projection(canonical: &mut Channel, previous: Channel) {
    if canonical.context_id.is_none() {
        canonical.context_id = previous.context_id;
    }
    if canonical.topic.is_none() {
        canonical.topic = previous.topic;
    }
    if canonical.member_ids.is_empty() {
        canonical.member_ids = previous.member_ids;
    }
    canonical.member_count = canonical.member_count.max(previous.member_count);
    canonical.unread_count = canonical.unread_count.max(previous.unread_count);
    if canonical.last_message.is_none() {
        canonical.last_message = previous.last_message;
    }
    if canonical.last_message_time.is_none() {
        canonical.last_message_time = previous.last_message_time;
    }
    canonical.last_activity = canonical.last_activity.max(previous.last_activity);
    canonical.last_finalized_epoch = canonical
        .last_finalized_epoch
        .max(previous.last_finalized_epoch);
}

/// Placeholder content the runtime renders for a message it could not open.
fn is_sealed_placeholder(content: &str) -> bool {
    content.starts_with("[sealed: ") && content.ends_with(" bytes]")
}

#[cfg(test)]
mod ordering_tests {
    use super::*;

    fn message(channel_id: ChannelId, id: &str, timestamp: u64) -> Message {
        Message {
            id: id.to_string(),
            channel_id,
            sender_id: aura_core::types::identifiers::AuthorityId::new_from_entropy([3u8; 32]),
            sender_name: String::new(),
            content: id.to_string(),
            timestamp,
            reply_to: None,
            is_own: false,
            is_read: false,
            delivery_status: MessageDeliveryStatus::default(),
            epoch_hint: None,
            is_finalized: false,
        }
    }

    #[test]
    fn out_of_order_arrivals_are_kept_in_send_order() {
        let channel_id = ChannelId::from_bytes([9u8; 32]);
        let mut state = ChatState::default();
        for (id, timestamp) in [("m1", 10), ("m3", 30), ("m2", 20), ("m4", 40)] {
            state.apply_message(channel_id, message(channel_id, id, timestamp));
        }
        let ids: Vec<_> = state
            .messages_for_channel(&channel_id)
            .iter()
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(ids, vec!["m1", "m2", "m3", "m4"]);
    }

    // Regression (work/8.md task 32): a message replicated to a new device
    // before its channel key arrives is opened once the key is there.
    #[test]
    fn sealed_placeholder_is_replaced_once_the_message_opens() {
        let channel_id = ChannelId::from_bytes([8u8; 32]);
        let mut state = ChatState::default();
        let mut sealed = message(channel_id, "m1", 10);
        sealed.content = "[sealed: 217 bytes]".to_string();
        state.apply_message(channel_id, sealed.clone());

        let mut opened = message(channel_id, "m1", 10);
        opened.content = "before enrollment".to_string();
        state.apply_message(channel_id, opened);
        assert_eq!(
            state.messages_for_channel(&channel_id)[0].content,
            "before enrollment"
        );

        // A readable message is never replaced by a sealed copy.
        state.apply_message(channel_id, sealed);
        assert_eq!(
            state.messages_for_channel(&channel_id)[0].content,
            "before enrollment"
        );
        assert_eq!(state.messages_for_channel(&channel_id).len(), 1);
    }
}
