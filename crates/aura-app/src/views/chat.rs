//! # Chat View State

mod delivery;
mod helpers;
mod models;
mod serde_support;
mod state;

pub use delivery::MessageDeliveryStatus;
pub use helpers::{
    is_note_to_self_channel_name, note_to_self_channel_id, note_to_self_context_id,
    NOTE_TO_SELF_CHANNEL_NAME, NOTE_TO_SELF_CHANNEL_TOPIC,
};
pub use models::{Channel, ChannelType, Message};
pub use state::ChatState;

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
    use serde_json::Value;

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
    fn test_chat_state_serializes_channels_as_canonical_map() {
        let channel_id = ChannelId::from_bytes([9u8; 32]);
        let state = ChatState::from_channels([make_test_channel(channel_id)]);

        let encoded = serde_json::to_value(&state).expect("chat state should serialize");
        let channels = encoded
            .get("channels")
            .expect("chat state should include channels");

        assert!(
            matches!(channels, Value::Object(_)),
            "channels should serialize as the canonical map form"
        );
        assert!(
            channels.get(channel_id.to_string()).is_some(),
            "serialized channels should be keyed by canonical channel id"
        );
    }

    #[test]
    fn test_mark_finalized_up_to_epoch_marks_messages() {
        let mut state = ChatState::default();
        let channel_id = ChannelId::from_bytes([1u8; 32]);
        state.add_channel(make_test_channel(channel_id));

        let mut msg1 = make_test_message("msg1", Some(1));
        msg1.channel_id = channel_id;
        let mut msg2 = make_test_message("msg2", Some(2));
        msg2.channel_id = channel_id;
        let mut msg3 = make_test_message("msg3", Some(3));
        msg3.channel_id = channel_id;
        let mut msg4 = make_test_message("msg4", None);
        msg4.channel_id = channel_id;

        state.apply_message(channel_id, msg1);
        state.apply_message(channel_id, msg2);
        state.apply_message(channel_id, msg3);
        state.apply_message(channel_id, msg4);

        let count = state.mark_finalized_up_to_epoch(&channel_id, 2);
        assert_eq!(count, Some(2));

        let messages = state.channel_messages.get(&channel_id).unwrap();
        assert!(
            messages
                .iter()
                .find(|m| m.id == "msg1")
                .unwrap()
                .is_finalized
        );
        assert!(
            messages
                .iter()
                .find(|m| m.id == "msg2")
                .unwrap()
                .is_finalized
        );
        assert!(
            !messages
                .iter()
                .find(|m| m.id == "msg3")
                .unwrap()
                .is_finalized
        );
        assert!(
            !messages
                .iter()
                .find(|m| m.id == "msg4")
                .unwrap()
                .is_finalized
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

        state.mark_finalized_up_to_epoch(&channel_id, 10);
        assert_eq!(state.channel(&channel_id).unwrap().last_finalized_epoch, 10);

        state.mark_finalized_up_to_epoch(&channel_id, 7);
        assert_eq!(state.channel(&channel_id).unwrap().last_finalized_epoch, 10);
    }

    #[test]
    fn test_mark_finalized_idempotent() {
        let mut state = ChatState::default();
        let channel_id = ChannelId::from_bytes([1u8; 32]);
        state.add_channel(make_test_channel(channel_id));

        let mut msg = make_test_message("msg1", Some(1));
        msg.channel_id = channel_id;
        state.apply_message(channel_id, msg);

        let count1 = state.mark_finalized_up_to_epoch(&channel_id, 5);
        assert_eq!(count1, Some(1));

        let count2 = state.mark_finalized_up_to_epoch(&channel_id, 5);
        assert_eq!(count2, Some(0));
    }

    #[test]
    fn test_mark_finalized_unknown_channel() {
        let mut state = ChatState::default();
        let unknown_channel = ChannelId::from_bytes([99u8; 32]);

        let count = state.mark_finalized_up_to_epoch(&unknown_channel, 10);
        assert_eq!(count, None);
    }

    #[test]
    fn test_rebind_channel_identity_moves_messages_to_canonical_channel() {
        let mut state = ChatState::default();
        let stale_id = ChannelId::from_bytes([7u8; 32]);
        let canonical_id = ChannelId::from_bytes([8u8; 32]);
        state.upsert_channel(Channel {
            id: stale_id,
            context_id: None,
            name: "shared-parity-lab".to_string(),
            topic: None,
            channel_type: ChannelType::Home,
            unread_count: 0,
            is_dm: false,
            member_ids: Vec::new(),
            member_count: 1,
            last_message: None,
            last_message_time: None,
            last_activity: 5,
            last_finalized_epoch: 0,
        });
        let mut message = make_test_message("msg-1", None);
        message.channel_id = stale_id;
        state.apply_message(stale_id, message);

        state.rebind_channel_identity(
            &stale_id,
            Channel {
                id: canonical_id,
                context_id: Some(ContextId::new_from_entropy([4u8; 32])),
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 2,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            },
        );

        assert!(state.channel(&stale_id).is_none());
        let canonical = state
            .channel(&canonical_id)
            .expect("canonical channel present");
        assert_eq!(canonical.member_count, 2);
        let messages = state.messages_for_channel(&canonical_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].channel_id, canonical_id);
        assert_eq!(messages[0].content, "Hello");
    }

    #[test]
    fn test_rebind_channel_identity_merges_existing_canonical_messages_atomically() {
        let mut state = ChatState::default();
        let stale_id = ChannelId::from_bytes([11u8; 32]);
        let canonical_id = ChannelId::from_bytes([12u8; 32]);

        state.upsert_channel(Channel {
            id: stale_id,
            context_id: Some(ContextId::new_from_entropy([1u8; 32])),
            name: "shared-parity-lab".to_string(),
            topic: Some("stale".to_string()),
            channel_type: ChannelType::Home,
            unread_count: 3,
            is_dm: false,
            member_ids: Vec::new(),
            member_count: 1,
            last_message: Some("stale".to_string()),
            last_message_time: Some(11),
            last_activity: 11,
            last_finalized_epoch: 2,
        });
        state.upsert_channel(Channel {
            id: canonical_id,
            context_id: None,
            name: "shared-parity-lab".to_string(),
            topic: None,
            channel_type: ChannelType::Home,
            unread_count: 1,
            is_dm: false,
            member_ids: Vec::new(),
            member_count: 2,
            last_message: Some("canonical".to_string()),
            last_message_time: Some(12),
            last_activity: 12,
            last_finalized_epoch: 3,
        });

        let mut stale_message = make_test_message("msg-stale", None);
        stale_message.channel_id = stale_id;
        state.apply_message(stale_id, stale_message.clone());

        let mut canonical_message = make_test_message("msg-canonical", None);
        canonical_message.channel_id = canonical_id;
        canonical_message.id = stale_message.id;
        canonical_message.content = "Canonical".to_string();
        state.apply_message(canonical_id, canonical_message);

        let original_total_unread = state.total_unread;

        state.rebind_channel_identity(
            &stale_id,
            Channel {
                id: canonical_id,
                context_id: None,
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 2,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            },
        );

        assert!(state.channel(&stale_id).is_none());
        let canonical = state
            .channel(&canonical_id)
            .expect("canonical channel present");
        assert_eq!(
            canonical.context_id,
            Some(ContextId::new_from_entropy([1u8; 32]))
        );
        assert_eq!(canonical.topic.as_deref(), Some("stale"));
        assert_eq!(canonical.unread_count, 3);
        assert_eq!(canonical.last_finalized_epoch, 3);
        assert_eq!(state.total_unread, original_total_unread);

        let messages = state.messages_for_channel(&canonical_id);
        assert_eq!(messages.len(), 1, "duplicate ids are deduplicated");
        assert_eq!(messages[0].channel_id, canonical_id);
    }
}
