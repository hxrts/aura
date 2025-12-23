//! Chat View Delta and Reducer
//!
//! This module provides view-level reduction for chat facts, transforming
//! journal facts into UI-level deltas for chat views.
//!
//! # Architecture
//!
//! View reduction is separate from journal-level reduction:
//! - **Journal reduction** (`ChatFactReducer`): Facts → `RelationalBinding` for storage
//! - **View reduction** (this module): Facts → `ChatDelta` for UI updates
//!
//! # Usage
//!
//! Register the reducer with the runtime's `ViewDeltaRegistry`:
//!
//! ```ignore
//! use aura_chat::{ChatViewReducer, CHAT_FACT_TYPE_ID};
//! use aura_composition::ViewDeltaRegistry;
//!
//! let mut registry = ViewDeltaRegistry::new();
//! registry.register(CHAT_FACT_TYPE_ID, Box::new(ChatViewReducer));
//! ```

use aura_composition::{IntoViewDelta, ViewDelta, ViewDeltaReducer};
use aura_core::identifiers::AuthorityId;
use aura_journal::DomainFact;

use crate::{ChatFact, CHAT_FACT_TYPE_ID};

/// Delta type for chat view updates.
///
/// These deltas represent incremental changes to chat UI state,
/// derived from journal facts during view reduction.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatDelta {
    /// A new channel was created or discovered
    ChannelAdded {
        /// Identifier of the channel (debug-friendly string).
        channel_id: String,
        /// Human-friendly channel name.
        name: String,
        /// Optional description/topic for the channel.
        topic: Option<String>,
        /// Indicates whether this channel is a direct message.
        is_dm: bool,
        /// Number of members currently in the channel.
        member_count: u32,
        /// Unix epoch milliseconds when the channel was created.
        created_at: u64,
        /// AuthorityId string of the creator.
        creator_id: String,
    },
    /// A channel was removed
    ChannelRemoved {
        /// Identifier of the removed channel.
        channel_id: String,
    },
    /// A channel's metadata was updated
    ChannelUpdated {
        /// Identifier of the channel whose metadata changed.
        channel_id: String,
        /// Updated channel name, if provided.
        name: Option<String>,
        /// Updated topic, if provided.
        topic: Option<String>,
        /// Updated member count hint.
        member_count: Option<u32>,
    },
    /// A new message was sent
    MessageAdded {
        /// Channel that received the message.
        channel_id: String,
        /// Unique identifier for the message.
        message_id: String,
        /// AuthorityId string of the sender.
        sender_id: String,
        /// Human-readable sender display name.
        sender_name: String,
        /// Message text/payload.
        content: String,
        /// Unix epoch milliseconds when the message was sent.
        timestamp: u64,
        /// Optional message this one replies to.
        reply_to: Option<String>,
    },
    /// A message was removed/deleted
    MessageRemoved {
        /// Channel from which the message was removed.
        channel_id: String,
        /// Identifier of the removed message.
        message_id: String,
    },
    /// A message was delivered to a recipient's device
    ///
    /// This delta is emitted when we learn that a message has been
    /// successfully received by the recipient (before they read it).
    /// Used for showing "delivered" status indicators (double checkmark).
    MessageDelivered {
        /// Channel containing the message.
        channel_id: String,
        /// Identifier of the delivered message.
        message_id: String,
        /// AuthorityId string of the recipient who received the message.
        recipient_id: String,
        /// Optional device that received the message.
        device_id: Option<String>,
        /// Unix epoch milliseconds when the message was delivered.
        delivered_at: u64,
    },
    /// A message was read by a recipient
    ///
    /// This delta is emitted when a recipient has viewed the message.
    /// Used for showing "read" status indicators (blue checkmarks).
    MessageRead {
        /// Channel containing the message.
        channel_id: String,
        /// Identifier of the read message.
        message_id: String,
        /// AuthorityId string of the reader.
        reader_id: String,
        /// Unix epoch milliseconds when the message was read.
        read_at: u64,
    },
    /// Delivery receipt was acknowledged by sender
    ///
    /// This delta is emitted when the sender acknowledges a delivery receipt,
    /// closing the delivery receipt loop. Primarily used for internal state
    /// management and garbage collection.
    DeliveryAcknowledged {
        /// Channel containing the message.
        channel_id: String,
        /// Identifier of the acknowledged message.
        message_id: String,
        /// Unix epoch milliseconds when the acknowledgment was sent.
        acknowledged_at: u64,
    },
}

/// View reducer for chat facts.
///
/// Transforms `ChatFact` instances into `ChatDelta` view updates.
pub struct ChatViewReducer;

impl ViewDeltaReducer for ChatViewReducer {
    fn handles_type(&self) -> &'static str {
        CHAT_FACT_TYPE_ID
    }

    fn reduce_fact(
        &self,
        binding_type: &str,
        binding_data: &[u8],
        _own_authority: Option<AuthorityId>,
    ) -> Vec<ViewDelta> {
        if binding_type != CHAT_FACT_TYPE_ID {
            return vec![];
        }

        let Some(chat_fact) = ChatFact::from_bytes(binding_data) else {
            return vec![];
        };

        let delta = match chat_fact {
            ChatFact::ChannelCreated {
                channel_id,
                name,
                topic,
                is_dm,
                created_at,
                creator_id,
                ..
            } => Some(ChatDelta::ChannelAdded {
                channel_id: channel_id.to_string(),
                name,
                topic,
                is_dm,
                member_count: 0, // Would need additional fact tracking
                created_at: created_at.ts_ms,
                creator_id: creator_id.to_string(),
            }),
            ChatFact::ChannelClosed { channel_id, .. } => Some(ChatDelta::ChannelRemoved {
                channel_id: channel_id.to_string(),
            }),
            ChatFact::ChannelUpdated {
                channel_id,
                name,
                topic,
                ..
            } => Some(ChatDelta::ChannelUpdated {
                channel_id: channel_id.to_string(),
                name,
                topic,
                member_count: None,
            }),
            ChatFact::MessageSentSealed {
                channel_id,
                message_id,
                sender_id,
                sender_name,
                payload: _,
                sent_at,
                reply_to,
                ..
            } => Some(ChatDelta::MessageAdded {
                channel_id: channel_id.to_string(),
                message_id,
                sender_id: sender_id.to_string(),
                sender_name,
                content: "<sealed message>".to_string(),
                timestamp: sent_at.ts_ms,
                reply_to,
            }),
            ChatFact::MessageRead {
                channel_id,
                message_id,
                reader_id,
                read_at,
                ..
            } => Some(ChatDelta::MessageRead {
                channel_id: channel_id.to_string(),
                message_id,
                reader_id: reader_id.to_string(),
                read_at: read_at.ts_ms,
            }),
            ChatFact::MessageDelivered {
                channel_id,
                message_id,
                recipient_id,
                device_id,
                delivered_at,
                ..
            } => Some(ChatDelta::MessageDelivered {
                channel_id: channel_id.to_string(),
                message_id,
                recipient_id: recipient_id.to_string(),
                device_id,
                delivered_at: delivered_at.ts_ms,
            }),
            ChatFact::DeliveryAcknowledged {
                channel_id,
                message_id,
                acknowledged_at,
                ..
            } => Some(ChatDelta::DeliveryAcknowledged {
                channel_id: channel_id.to_string(),
                message_id,
                acknowledged_at: acknowledged_at.ts_ms,
            }),
        };

        delta.map(|d| vec![d.into_view_delta()]).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_composition::downcast_delta;
    use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};

    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([42u8; 32])
    }

    #[test]
    fn test_channel_created_reduction() {
        let reducer = ChatViewReducer;

        let fact = ChatFact::channel_created_ms(
            test_context_id(),
            ChannelId::default(),
            "test-channel".to_string(),
            Some("A test topic".to_string()),
            false,
            1234567890,
            AuthorityId::default(),
        );

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(CHAT_FACT_TYPE_ID, &bytes, None);

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<ChatDelta>(&deltas[0]).unwrap();
        match delta {
            ChatDelta::ChannelAdded {
                name,
                topic,
                is_dm,
                created_at,
                ..
            } => {
                assert_eq!(name, "test-channel");
                assert_eq!(topic, &Some("A test topic".to_string()));
                assert!(!is_dm);
                assert_eq!(*created_at, 1234567890);
            }
            _ => panic!("Expected ChannelAdded delta"),
        }
    }

    #[test]
    fn test_ids_use_display() {
        let reducer = ChatViewReducer;

        let channel_id = ChannelId::from_bytes([1u8; 32]);
        let creator = AuthorityId::new_from_entropy([2u8; 32]);

        let fact = ChatFact::channel_created_ms(
            test_context_id(),
            channel_id,
            "test-channel".to_string(),
            None,
            false,
            123,
            creator,
        );

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(CHAT_FACT_TYPE_ID, &bytes, None);

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<ChatDelta>(&deltas[0]).unwrap();
        match delta {
            ChatDelta::ChannelAdded {
                channel_id: id,
                creator_id: creator_id_str,
                ..
            } => {
                assert_eq!(id, &channel_id.to_string());
                assert_eq!(creator_id_str, &creator.to_string());
            }
            _ => panic!("Expected ChannelAdded delta"),
        }
    }

    #[test]
    fn test_message_sent_reduction() {
        let reducer = ChatViewReducer;

        let fact = ChatFact::message_sent_sealed_ms(
            test_context_id(),
            ChannelId::default(),
            "msg-123".to_string(),
            AuthorityId::default(),
            "Alice".to_string(),
            b"Hello, world!".to_vec(),
            1234567890,
            None,
        );

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(CHAT_FACT_TYPE_ID, &bytes, None);

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<ChatDelta>(&deltas[0]).unwrap();
        match delta {
            ChatDelta::MessageAdded {
                message_id,
                sender_name,
                content,
                ..
            } => {
                assert_eq!(message_id, "msg-123");
                assert_eq!(sender_name, "Alice");
                assert_eq!(content, "<sealed message>");
            }
            _ => panic!("Expected MessageAdded delta"),
        }
    }

    #[test]
    fn test_wrong_type_returns_empty() {
        let reducer = ChatViewReducer;
        let deltas = reducer.reduce_fact("wrong_type", b"some data", None);
        assert!(deltas.is_empty());
    }

    #[test]
    fn test_invalid_data_returns_empty() {
        let reducer = ChatViewReducer;
        let deltas = reducer.reduce_fact(CHAT_FACT_TYPE_ID, b"invalid json data", None);
        assert!(deltas.is_empty());
    }
}
