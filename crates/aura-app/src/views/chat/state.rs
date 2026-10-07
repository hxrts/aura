#![allow(missing_docs)]

use super::delivery::MessageDeliveryStatus;
use super::models::{Channel, ChannelType, Message};
use super::serde_support::channel_id_keyed_map;
use aura_chat::view::CanonicalChannelCreation;
use aura_chat::{
    MessageRevision, MessageRevisionKind, MessageRevisionOutcome, MessageRevisionRegister,
};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A metadata update that may be held until its channel creation fact arrives.
#[derive(Debug, Clone)]
pub struct ChannelProjectionUpdate {
    pub context_id: Option<ContextId>,
    pub name: Option<String>,
    pub topic: Option<String>,
    pub member_count: Option<u32>,
    pub member_ids: Option<Vec<AuthorityId>>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Default)]
struct ChannelUpdateClocks {
    name: u64,
    topic: u64,
    member_count: u64,
    member_ids: u64,
}

/// Chat state.
///
/// Observed callers cannot insert a channel from raw metadata. Production
/// materialization requires `CanonicalChannelCreation`.
///
/// ```compile_fail
/// use aura_app::views::chat::{Channel, ChatState};
/// let mut chat = ChatState::new();
/// chat.add_channel(Channel::default());
/// ```
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
    /// Internal provenance is reconstructed from journal replay after restart.
    #[serde(skip)]
    canonical_channels: HashSet<ChannelId>,
    #[serde(skip)]
    pending_channel_updates: HashMap<ChannelId, Vec<ChannelProjectionUpdate>>,
    #[serde(skip)]
    channel_update_clocks: HashMap<ChannelId, ChannelUpdateClocks>,
    /// Edits and deletes per message, resolved order-independently
    /// (`aura_chat::revisions`); a tombstone hides a message sent later.
    #[serde(skip)]
    message_revisions: HashMap<(ChannelId, String), MessageRevisionRegister>,
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

    /// Whether this channel was established from canonical creation evidence.
    #[must_use]
    pub fn has_canonical_channel(&self, id: &ChannelId, context_id: ContextId) -> bool {
        self.canonical_channels.contains(id)
            && self
                .channels
                .get(id)
                .is_some_and(|channel| channel.context_id == Some(context_id))
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

    #[cfg(test)]
    pub fn add_channel(&mut self, channel: Channel) {
        self.channels.entry(channel.id).or_insert(channel);
    }

    #[cfg(test)]
    pub fn upsert_channel(&mut self, channel: Channel) {
        self.channels.insert(channel.id, channel);
    }

    /// Materialize a channel only after a caller has consumed creation evidence.
    /// Duplicate creation replay does not reset later metadata updates.
    pub fn materialize_canonical_channel(
        &mut self,
        creation: CanonicalChannelCreation,
        local_authority: Option<AuthorityId>,
    ) -> bool {
        let channel_id = creation.channel_id();
        if !self.canonical_channels.insert(channel_id) {
            return false;
        }
        let is_dm = creation.is_dm();
        let creator = creation.creator_id();
        let mut member_ids = Vec::new();
        if let Some(local_authority) = local_authority {
            if is_dm {
                member_ids.push(local_authority);
            }
            if creator != local_authority {
                member_ids.push(creator);
            }
        }
        let member_count = if is_dm {
            member_ids.len().max(2) as u32
        } else if member_ids.is_empty() {
            creation.member_count()
        } else {
            member_ids.len().saturating_add(1) as u32
        };
        let mut channel = Channel {
            id: channel_id,
            context_id: Some(creation.context_id()),
            name: creation.name().to_string(),
            topic: creation.topic().map(str::to_string),
            channel_type: if is_dm {
                ChannelType::DirectMessage
            } else {
                ChannelType::Home
            },
            unread_count: 0,
            is_dm,
            member_ids,
            member_count,
            last_message: None,
            last_message_time: None,
            last_activity: creation.created_at(),
            last_finalized_epoch: 0,
        };
        if let Some(previous) = self.channels.get(&channel_id) {
            channel.unread_count = previous.unread_count;
            channel.last_message = previous.last_message.clone();
            channel.last_message_time = previous.last_message_time;
            channel.last_finalized_epoch = previous.last_finalized_epoch;
        }
        if let Some(last_message) = self
            .channel_messages
            .get(&channel_id)
            .and_then(|messages| messages.last())
        {
            if channel
                .last_message_time
                .is_none_or(|time| last_message.timestamp >= time)
            {
                channel.last_message = Some(last_message.content.clone());
                channel.last_message_time = Some(last_message.timestamp);
                channel.last_activity = channel.last_activity.max(last_message.timestamp);
            }
        }
        self.channels.insert(channel_id, channel);
        if let Some(mut pending) = self.pending_channel_updates.remove(&channel_id) {
            pending.sort_by_key(|update| update.updated_at);
            for update in pending {
                self.apply_or_stage_channel_update(channel_id, update);
            }
        }
        true
    }

    /// Enrich an established channel or stage a pre-creation update without
    /// exposing a phantom channel to observers.
    pub fn apply_or_stage_channel_update(
        &mut self,
        channel_id: ChannelId,
        update: ChannelProjectionUpdate,
    ) -> bool {
        if !self.canonical_channels.contains(&channel_id) {
            self.pending_channel_updates
                .entry(channel_id)
                .or_default()
                .push(update);
            return false;
        }
        let Some(channel) = self.channels.get_mut(&channel_id) else {
            return false;
        };
        if update
            .context_id
            .is_some_and(|context| channel.context_id != Some(context))
        {
            return false;
        }
        let clocks = self.channel_update_clocks.entry(channel_id).or_default();
        if let Some(name) = update.name {
            if update.updated_at >= clocks.name {
                channel.name = name;
                clocks.name = update.updated_at;
            }
        }
        if let Some(topic) = update.topic {
            if update.updated_at >= clocks.topic {
                channel.topic = Some(topic);
                clocks.topic = update.updated_at;
            }
        }
        if let Some(member_count) = update.member_count {
            if update.updated_at >= clocks.member_count {
                channel.member_count = member_count;
                clocks.member_count = update.updated_at;
            }
        }
        if let Some(member_ids) = update.member_ids {
            if update.updated_at >= clocks.member_ids {
                channel.member_ids = member_ids;
                clocks.member_ids = update.updated_at;
            }
        }
        channel.last_activity = channel.last_activity.max(update.updated_at);
        true
    }

    #[cfg(test)]
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
        self.canonical_channels.remove(channel_id);
        self.pending_channel_updates.remove(channel_id);
        self.channel_update_clocks.remove(channel_id);
        self.message_revisions
            .retain(|(channel, _), _| channel != channel_id);
        self.channels.remove(channel_id)
    }

    pub fn clear(&mut self) {
        self.channels.clear();
        self.channel_messages.clear();
        self.canonical_channels.clear();
        self.pending_channel_updates.clear();
        self.channel_update_clocks.clear();
        self.message_revisions.clear();
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

    pub fn apply_message(&mut self, channel_id: ChannelId, mut message: Message) {
        match self.revision_outcome(&channel_id, &message.id) {
            RevisionView::Deleted => return,
            RevisionView::Edited(content) => message.content = content,
            RevisionView::Unrevised => {}
        }
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

    /// Record an edit or delete of `message_id` and re-render the message from
    /// the resolved register: a delete removes it for good, otherwise the
    /// winning edit's content is shown. Returns whether the revision was new.
    pub fn apply_message_revision(
        &mut self,
        channel_id: ChannelId,
        message_id: &str,
        revision: MessageRevision,
    ) -> bool {
        let inserted = self
            .message_revisions
            .entry((channel_id, message_id.to_string()))
            .or_default()
            .insert(revision);
        match self.revision_outcome(&channel_id, message_id) {
            RevisionView::Deleted => {
                if let Some(msgs) = self.channel_messages.get_mut(&channel_id) {
                    msgs.retain(|m| m.id != message_id);
                }
            }
            RevisionView::Edited(content) => {
                if let Some(message) = self.message_mut(&channel_id, message_id) {
                    message.content = content;
                }
            }
            RevisionView::Unrevised => {}
        }
        inserted
    }

    fn revision_outcome(&self, channel_id: &ChannelId, message_id: &str) -> RevisionView {
        let Some(register) = self
            .message_revisions
            .get(&(*channel_id, message_id.to_string()))
        else {
            return RevisionView::Unrevised;
        };
        match register.resolve() {
            MessageRevisionOutcome::Deleted => RevisionView::Deleted,
            MessageRevisionOutcome::Edited(MessageRevision {
                kind: MessageRevisionKind::Edit { new_payload },
                ..
            }) => RevisionView::Edited(String::from_utf8_lossy(new_payload).into_owned()),
            MessageRevisionOutcome::Edited(_) | MessageRevisionOutcome::Unrevised => {
                RevisionView::Unrevised
            }
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

#[cfg(test)]
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
/// Rendered resolution of a message's revision register.
enum RevisionView {
    Unrevised,
    Edited(String),
    Deleted,
}

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

#[cfg(test)]
mod revision_tests {
    use super::*;
    use aura_chat::revisions::test_support::causal_after;
    use aura_chat::{ChatFact, MessageRevisionKey};
    use aura_journal::causal_reduction::assert_permutation_invariant;

    #[derive(Debug, Clone)]
    enum Arrival {
        Send(Message),
        Revise(MessageRevision),
    }

    fn channel() -> ChannelId {
        ChannelId::from_bytes([4u8; 32])
    }

    fn context() -> ContextId {
        ContextId::new_from_entropy([5u8; 32])
    }

    fn key() -> MessageRevisionKey {
        MessageRevisionKey::new(context(), channel(), "m1")
    }

    fn send() -> Arrival {
        Arrival::Send(Message {
            id: "m1".to_string(),
            channel_id: channel(),
            sender_id: AuthorityId::new_from_entropy([3u8; 32]),
            sender_name: String::new(),
            content: "original".to_string(),
            timestamp: 10,
            reply_to: None,
            is_own: false,
            is_read: false,
            delivery_status: MessageDeliveryStatus::default(),
            epoch_hint: None,
            is_finalized: false,
        })
    }

    fn revision(fact: &ChatFact) -> MessageRevision {
        MessageRevision::from_fact(fact).expect("edit or delete")
    }

    fn edit(device: u8, body: &str, observed: &[MessageRevision]) -> MessageRevision {
        revision(&ChatFact::message_edited_ms(
            context(),
            channel(),
            "m1".to_string(),
            AuthorityId::new_from_entropy([device; 32]),
            body.as_bytes().to_vec(),
            0,
            causal_after(device, key(), observed),
        ))
    }

    fn delete(device: u8, observed: &[MessageRevision]) -> MessageRevision {
        revision(&ChatFact::message_deleted_ms(
            context(),
            channel(),
            "m1".to_string(),
            AuthorityId::new_from_entropy([device; 32]),
            0,
            causal_after(device, key(), observed),
        ))
    }

    fn contents(arrivals: &[Arrival]) -> Vec<String> {
        let mut state = ChatState::default();
        for arrival in arrivals {
            match arrival.clone() {
                Arrival::Send(message) => state.apply_message(channel(), message),
                Arrival::Revise(revision) => {
                    state.apply_message_revision(channel(), "m1", revision);
                }
            }
        }
        state
            .messages_for_channel(&channel())
            .iter()
            .map(|message| message.content.clone())
            .collect()
    }

    #[test]
    fn concurrent_edits_converge_for_every_arrival_order() {
        let base = edit(1, "base", &[]);
        let left = edit(2, "left", std::slice::from_ref(&base));
        let right = edit(3, "right", std::slice::from_ref(&base));
        let shown = assert_permutation_invariant(
            &[
                send(),
                Arrival::Revise(base),
                Arrival::Revise(left),
                Arrival::Revise(right),
            ],
            contents,
        );
        assert!(shown == ["left"] || shown == ["right"], "{shown:?}");
    }

    #[test]
    fn edit_after_delete_stays_deleted() {
        let first = edit(1, "a", &[]);
        let removed = delete(2, std::slice::from_ref(&first));
        let after = edit(1, "after", &[first.clone(), removed.clone()]);
        let shown = assert_permutation_invariant(
            &[
                send(),
                Arrival::Revise(first),
                Arrival::Revise(removed),
                Arrival::Revise(after),
            ],
            contents,
        );
        assert!(shown.is_empty());
    }

    #[test]
    fn delete_before_send_arrival_hides_the_message() {
        let shown =
            assert_permutation_invariant(&[send(), Arrival::Revise(delete(2, &[]))], contents);
        assert!(shown.is_empty());
    }
}
