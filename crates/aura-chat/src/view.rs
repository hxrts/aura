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

use aura_composition::{ComposableDelta, IntoViewDelta, ViewDelta, ViewDeltaReducer};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_journal::DomainFact;

use crate::{ChatFact, ChatMessageDeliveryStatus, CHAT_FACT_TYPE_ID};

/// Delta type for chat view updates.
///
/// These deltas represent incremental changes to chat UI state,
/// derived from journal facts during view reduction.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatDelta {
    /// A channel backed by a canonical `ChannelCreated` fact.
    ChannelAdded(CanonicalChannelCreation),
    /// A channel was removed
    ChannelRemoved {
        /// Identifier of the removed channel.
        channel_id: String,
    },
    /// A channel's metadata was updated
    ChannelUpdated {
        /// Identifier of the channel whose metadata changed.
        channel_id: String,
        /// Updated authoritative context identifier, if provided.
        context_id: Option<String>,
        /// Updated channel name, if provided.
        name: Option<String>,
        /// Updated topic, if provided.
        topic: Option<String>,
        /// Updated member count hint.
        member_count: Option<u32>,
        /// Updated known non-self participant identifiers.
        member_ids: Option<Vec<String>>,
        /// Fact timestamp used to order updates arriving before creation.
        updated_at: u64,
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
        /// Channel epoch when message was sent (for consensus finalization tracking).
        epoch_hint: Option<u32>,
    },
    /// A message was removed/deleted
    MessageRemoved {
        /// Channel from which the message was removed.
        channel_id: String,
        /// Identifier of the removed message.
        message_id: String,
    },
    /// A message was edited (Category A operation)
    MessageUpdated {
        /// Channel containing the message.
        channel_id: String,
        /// Identifier of the edited message.
        message_id: String,
        /// AuthorityId string of the editor (must be original sender).
        editor_id: String,
        /// New content after edit.
        new_content: String,
        /// Unix epoch milliseconds when the edit occurred.
        edited_at: u64,
    },
    /// A message delivery lifecycle status changed.
    MessageDeliveryUpdated {
        /// Channel containing the message.
        channel_id: String,
        /// Identifier of the message.
        message_id: String,
        /// Updated delivery lifecycle status.
        delivery_status: ChatMessageDeliveryStatus,
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
}

/// Evidence that a channel was established by a `ChannelCreated` fact.
///
/// Fields are private so an observed projection cannot manufacture a channel
/// from a metadata update, membership event, or raw identifier.
///
/// ```compile_fail
/// use aura_chat::view::CanonicalChannelCreation;
/// use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
/// let _forged = CanonicalChannelCreation {
///     channel_id: ChannelId::from_bytes([1; 32]),
///     context_id: ContextId::new_from_entropy([2; 32]),
///     name: "made up".into(),
///     topic: None,
///     is_dm: false,
///     member_count: 1,
///     created_at: 0,
///     creator_id: AuthorityId::new_from_entropy([3; 32]),
/// };
/// ```
///
/// ```compile_fail
/// use aura_chat::{view::CanonicalChannelCreation, ChatFact};
/// let fact: ChatFact = unreachable!();
/// let _forged = CanonicalChannelCreation::from_fact(&fact);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalChannelCreation {
    channel_id: ChannelId,
    context_id: ContextId,
    name: String,
    topic: Option<String>,
    is_dm: bool,
    member_count: u32,
    created_at: u64,
    creator_id: AuthorityId,
}

impl CanonicalChannelCreation {
    /// Channel identity established by the creation fact.
    #[must_use]
    pub fn channel_id(&self) -> ChannelId {
        self.channel_id
    }
    /// Relational context established by the creation fact.
    #[must_use]
    pub fn context_id(&self) -> ContextId {
        self.context_id
    }
    /// Canonical channel name from the creation fact.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Canonical channel topic from the creation fact.
    #[must_use]
    pub fn topic(&self) -> Option<&str> {
        self.topic.as_deref()
    }
    /// Whether creation established a direct-message channel.
    #[must_use]
    pub fn is_dm(&self) -> bool {
        self.is_dm
    }
    /// Initial member count hint from the creation fact.
    #[must_use]
    pub fn member_count(&self) -> u32 {
        self.member_count
    }
    /// Creation timestamp in milliseconds.
    #[must_use]
    pub fn created_at(&self) -> u64 {
        self.created_at
    }
    /// Authority that created the channel.
    #[must_use]
    pub fn creator_id(&self) -> AuthorityId {
        self.creator_id
    }
}

/// Keys for chat delta composition.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChatDeltaKey {
    /// Channel key (channel_id).
    Channel(String),
    /// Message key (channel_id, message_id).
    Message(String, String),
    /// Message read key (channel_id, message_id, reader_id).
    MessageRead(String, String, String),
}

impl ChatDelta {
    fn apply_if_newer(current_ts: &mut u64, incoming_ts: u64, update: impl FnOnce()) -> bool {
        if incoming_ts >= *current_ts {
            *current_ts = incoming_ts;
            update();
        }
        true
    }
}

impl ComposableDelta for ChatDelta {
    type Key = ChatDeltaKey;

    fn key(&self) -> Self::Key {
        match self {
            ChatDelta::ChannelAdded(creation) => {
                ChatDeltaKey::Channel(creation.channel_id.to_string())
            }
            ChatDelta::ChannelRemoved { channel_id }
            | ChatDelta::ChannelUpdated { channel_id, .. } => {
                ChatDeltaKey::Channel(channel_id.clone())
            }
            ChatDelta::MessageAdded {
                channel_id,
                message_id,
                ..
            }
            | ChatDelta::MessageUpdated {
                channel_id,
                message_id,
                ..
            }
            | ChatDelta::MessageDeliveryUpdated {
                channel_id,
                message_id,
                ..
            }
            | ChatDelta::MessageRemoved {
                channel_id,
                message_id,
            } => ChatDeltaKey::Message(channel_id.clone(), message_id.clone()),
            ChatDelta::MessageRead {
                channel_id,
                message_id,
                reader_id,
                ..
            } => {
                ChatDeltaKey::MessageRead(channel_id.clone(), message_id.clone(), reader_id.clone())
            }
        }
    }

    fn try_merge(&mut self, other: Self) -> bool {
        match (self, other) {
            // Keep channel creation and updates as distinct deltas. A metadata
            // update cannot become creation evidence during compaction, and
            // preserving timestamps lets the projection replay updates in order.
            (ChatDelta::ChannelRemoved { .. }, ChatDelta::ChannelRemoved { .. }) => true,
            (
                ChatDelta::MessageAdded {
                    timestamp,
                    channel_id: ch,
                    message_id: msg,
                    sender_id: sender,
                    sender_name: name,
                    content: body,
                    reply_to: reply,
                    epoch_hint: epoch,
                },
                ChatDelta::MessageAdded {
                    timestamp: other_ts,
                    channel_id,
                    message_id,
                    sender_id,
                    sender_name,
                    content,
                    reply_to,
                    epoch_hint: other_epoch,
                },
            ) => Self::apply_if_newer(timestamp, other_ts, || {
                *ch = channel_id;
                *msg = message_id;
                *sender = sender_id;
                *name = sender_name;
                *body = content;
                *reply = reply_to;
                *epoch = other_epoch;
            }),
            (
                ChatDelta::MessageUpdated {
                    edited_at,
                    channel_id: ch,
                    message_id: msg,
                    editor_id: editor,
                    new_content: content,
                },
                ChatDelta::MessageUpdated {
                    edited_at: other_ts,
                    channel_id,
                    message_id,
                    editor_id,
                    new_content,
                },
            ) => Self::apply_if_newer(edited_at, other_ts, || {
                *ch = channel_id;
                *msg = message_id;
                *editor = editor_id;
                *content = new_content;
            }),
            (
                ChatDelta::MessageDeliveryUpdated {
                    delivery_status, ..
                },
                ChatDelta::MessageDeliveryUpdated {
                    delivery_status: other_status,
                    ..
                },
            ) => {
                *delivery_status = other_status;
                true
            }
            (ChatDelta::MessageRemoved { .. }, ChatDelta::MessageRemoved { .. }) => true,
            (
                ChatDelta::MessageRead { read_at, .. },
                ChatDelta::MessageRead {
                    read_at: other_ts, ..
                },
            ) => Self::apply_if_newer(read_at, other_ts, || {}),
            _ => false,
        }
    }
}

/// View reducer for chat facts.
///
/// Transforms `ChatFact` instances into `ChatDelta` view updates.
pub struct ChatViewReducer;

impl ChatViewReducer {
    fn stringify_authority_ids(ids: Vec<AuthorityId>) -> Vec<String> {
        ids.into_iter().map(|id| id.to_string()).collect()
    }
}

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
                context_id,
                name,
                topic,
                is_dm,
                created_at,
                creator_id,
            } => ChatDelta::ChannelAdded(CanonicalChannelCreation {
                channel_id,
                context_id,
                name,
                topic,
                is_dm,
                member_count: 1,
                created_at: created_at.ts_ms,
                creator_id,
            }),
            ChatFact::ChannelClosed { channel_id, .. } => ChatDelta::ChannelRemoved {
                channel_id: channel_id.to_string(),
            },
            ChatFact::ChannelUpdated {
                context_id,
                channel_id,
                name,
                topic,
                member_count,
                member_ids,
                updated_at,
                ..
            } => ChatDelta::ChannelUpdated {
                channel_id: channel_id.to_string(),
                context_id: Some(context_id.to_string()),
                name,
                topic,
                member_count,
                member_ids: member_ids.map(Self::stringify_authority_ids),
                updated_at: updated_at.ts_ms,
            },
            ChatFact::MessageSentSealed {
                channel_id,
                message_id,
                sender_id,
                sender_name,
                payload: _,
                sent_at,
                reply_to,
                epoch_hint,
                ..
            } => ChatDelta::MessageAdded {
                channel_id: channel_id.to_string(),
                message_id,
                sender_id: sender_id.to_string(),
                sender_name,
                content: "<sealed message>".to_string(),
                timestamp: sent_at.ts_ms,
                reply_to,
                epoch_hint,
            },
            ChatFact::MessageRead {
                channel_id,
                message_id,
                reader_id,
                read_at,
                ..
            } => ChatDelta::MessageRead {
                channel_id: channel_id.to_string(),
                message_id,
                reader_id: reader_id.to_string(),
                read_at: read_at.ts_ms,
            },
            ChatFact::MessageEdited {
                channel_id,
                message_id,
                editor_id,
                new_payload,
                edited_at,
                ..
            } => ChatDelta::MessageUpdated {
                channel_id: channel_id.to_string(),
                message_id,
                editor_id: editor_id.to_string(),
                new_content: String::from_utf8_lossy(&new_payload).to_string(),
                edited_at: edited_at.ts_ms,
            },
            ChatFact::MessageDeliveryUpdated {
                channel_id,
                message_id,
                delivery_status,
                ..
            } => ChatDelta::MessageDeliveryUpdated {
                channel_id: channel_id.to_string(),
                message_id,
                delivery_status,
            },
            ChatFact::MessageDeleted {
                channel_id,
                message_id,
                ..
            } => ChatDelta::MessageRemoved {
                channel_id: channel_id.to_string(),
                message_id,
            },
        };

        vec![delta.into_view_delta()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{test_authority_id, test_channel_id, test_context_id};
    use aura_composition::compact_deltas;
    use aura_composition::downcast_delta;

    #[test]
    fn test_channel_created_reduction() {
        let reducer = ChatViewReducer;

        let fact = ChatFact::channel_created_ms(
            test_context_id(42),
            test_channel_id(0),
            "test-channel".to_string(),
            Some("A test topic".to_string()),
            false,
            1234567890,
            test_authority_id(1),
        );

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(CHAT_FACT_TYPE_ID, &bytes, None);

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<ChatDelta>(&deltas[0]).unwrap();
        match delta {
            ChatDelta::ChannelAdded(creation) => {
                assert_eq!(creation.name(), "test-channel");
                assert_eq!(creation.topic(), Some("A test topic"));
                assert!(!creation.is_dm());
                assert_eq!(creation.created_at(), 1234567890);
            }
            _ => panic!("Expected ChannelAdded delta"),
        }
    }

    #[test]
    fn test_ids_use_display() {
        let reducer = ChatViewReducer;

        let channel_id = test_channel_id(1);
        let creator = test_authority_id(2);

        let fact = ChatFact::channel_created_ms(
            test_context_id(42),
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
            ChatDelta::ChannelAdded(creation) => {
                assert_eq!(creation.channel_id(), channel_id);
                assert_eq!(creation.creator_id(), creator);
            }
            _ => panic!("Expected ChannelAdded delta"),
        }
    }

    #[test]
    fn metadata_update_cannot_supply_creation_witness() {
        let update = ChatFact::channel_updated_ms(
            test_context_id(1),
            test_channel_id(1),
            Some("forged".to_string()),
            None,
            Some(3),
            None,
            20,
            test_authority_id(1),
        );
        let deltas = ChatViewReducer.reduce_fact(CHAT_FACT_TYPE_ID, &update.to_bytes(), None);
        assert_eq!(deltas.len(), 1);
        assert!(matches!(
            downcast_delta::<ChatDelta>(&deltas[0]),
            Some(ChatDelta::ChannelUpdated { .. })
        ));
    }

    #[test]
    fn test_message_sent_reduction() {
        let reducer = ChatViewReducer;

        let fact = ChatFact::message_sent_sealed_ms(
            test_context_id(42),
            test_channel_id(0),
            "msg-123".to_string(),
            test_authority_id(1),
            "Alice".to_string(),
            b"Hello, world!".to_vec(),
            1234567890,
            None,
            None, // epoch_hint
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

    /// Compaction preserves creation evidence and metadata updates separately.
    #[test]
    fn test_compact_deltas_preserves_channel_creation_and_updates() {
        let channel_id = test_channel_id(1);
        let context_id = test_context_id(1);
        let created = ChatFact::channel_created_ms(
            context_id,
            channel_id,
            "general".to_string(),
            None,
            false,
            10,
            test_authority_id(1),
        );
        let creation_deltas =
            ChatViewReducer.reduce_fact(CHAT_FACT_TYPE_ID, &created.to_bytes(), None);
        let Some(ChatDelta::ChannelAdded(creation)) =
            downcast_delta::<ChatDelta>(&creation_deltas[0])
        else {
            panic!("ChannelCreated must reduce to ChannelAdded")
        };
        let deltas = vec![
            ChatDelta::ChannelAdded(creation.clone()),
            ChatDelta::ChannelUpdated {
                channel_id: channel_id.to_string(),
                context_id: Some(context_id.to_string()),
                name: Some("general-chat".to_string()),
                topic: Some("new topic".to_string()),
                member_count: Some(3),
                member_ids: None,
                updated_at: 20,
            },
        ];

        let compacted = compact_deltas(deltas);
        assert_eq!(compacted.len(), 2);
        match &compacted[0] {
            ChatDelta::ChannelAdded(creation) => {
                assert_eq!(creation.name(), "general");
                assert_eq!(creation.topic(), None);
                assert_eq!(creation.member_count(), 1);
            }
            _ => panic!("Expected ChannelAdded after compaction"),
        }
        assert!(
            matches!(&compacted[1], ChatDelta::ChannelUpdated { name: Some(name), .. } if name == "general-chat")
        );
    }
}
