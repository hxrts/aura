//! Chat domain facts
//!
//! This module defines chat-specific fact types that implement the `DomainFact`
//! trait from `aura-journal`. These facts are stored as `RelationalFact::Generic`
//! in the journal and reduced using the `ChatFactReducer`.
//!
//! # Architecture
//!
//! Following the Open/Closed Principle:
//! - `aura-journal` provides the generic fact infrastructure
//! - `aura-chat` defines domain-specific fact types without modifying `aura-journal`
//! - Runtime registers `ChatFactReducer` with the `FactRegistry`
//!
//! # Example
//!
//! ```ignore
//! use aura_chat::facts::{ChatFact, ChatFactReducer};
//! use aura_journal::{FactRegistry, DomainFact};
//!
//! // Create a chat fact using backward-compatible constructor
//! let fact = ChatFact::channel_created_ms(
//!     context_id,
//!     channel_id,
//!     "general".to_string(),
//!     None,
//!     false,
//!     1234567890,
//!     creator_id,
//! );
//!
//! // Convert to generic for storage
//! let generic = fact.to_generic();
//!
//! // Register reducer at runtime
//! registry.register::<ChatFact>("chat", Box::new(ChatFactReducer));
//! ```

use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::time::PhysicalTime;
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer,
};
use aura_macros::DomainFact;
use serde::{Deserialize, Serialize};

/// Type identifier for chat facts
pub const CHAT_FACT_TYPE_ID: &str = "chat";
/// Key for indexing chat facts in the journal
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatFactKey {
    /// Sub-type discriminator for the fact
    pub sub_type: &'static str,
    /// Serialized key data for lookup
    pub data: Vec<u8>,
}

/// Chat domain fact types
///
/// These facts represent chat-related state changes in the journal.
/// They are stored as `RelationalFact::Generic` and reduced by `ChatFactReducer`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, DomainFact)]
#[domain_fact(type_id = "chat", schema_version = 1, context = "context_id")]
pub enum ChatFact {
    /// Channel created in a relational context
    ChannelCreated {
        /// Relational context (home/group) where the channel exists
        context_id: ContextId,
        /// Unique channel identifier
        channel_id: ChannelId,
        /// Human-readable channel name
        name: String,
        /// Optional channel topic/description
        topic: Option<String>,
        /// Whether this is a direct message (1:1) channel
        is_dm: bool,
        /// Timestamp when channel was created (uses unified time system)
        created_at: PhysicalTime,
        /// Authority that created the channel
        creator_id: AuthorityId,
    },
    /// Channel closed/archived
    ChannelClosed {
        /// Relational context where the channel existed
        context_id: ContextId,
        /// Channel being closed
        channel_id: ChannelId,
        /// Timestamp when channel was closed (uses unified time system)
        closed_at: PhysicalTime,
        /// Authority that closed the channel
        actor_id: AuthorityId,
    },
    /// Channel metadata updated (name/topic)
    ChannelUpdated {
        /// Relational context where the channel exists
        context_id: ContextId,
        /// Channel being updated
        channel_id: ChannelId,
        /// Updated channel name (optional)
        name: Option<String>,
        /// Updated channel topic (optional)
        topic: Option<String>,
        /// Timestamp when channel was updated (uses unified time system)
        updated_at: PhysicalTime,
        /// Authority that updated the channel
        actor_id: AuthorityId,
    },
    /// Message sent with an opaque/sealed payload.
    ///
    /// The payload is treated as **opaque bytes** by default. Higher layers may
    /// choose to render it (e.g. UTF-8) *only if* policy permits.
    ///
    /// For delivery tracking, mark this fact as `ack_tracked` when committing.
    /// The generic acknowledgment system will automatically track per-peer
    /// delivery confirmations via the transport layer.
    MessageSentSealed {
        /// Relational context where the message was sent
        context_id: ContextId,
        /// Channel where the message was sent
        channel_id: ChannelId,
        /// Unique message identifier (derived from content hash + timestamp)
        message_id: String,
        /// Authority that sent the message
        sender_id: AuthorityId,
        /// Human-readable sender name (cached for display)
        sender_name: String,
        /// Opaque payload bytes (typically ciphertext)
        payload: Vec<u8>,
        /// Timestamp when message was sent (uses unified time system)
        sent_at: PhysicalTime,
        /// Optional message ID this is replying to
        reply_to: Option<String>,
        /// Channel epoch when message was sent (for consensus finalization tracking)
        epoch_hint: Option<u32>,
    },
    /// Message read by an authority
    ///
    /// This is a semantic "user read" event, distinct from delivery acknowledgment.
    /// Read receipts indicate the user has actually viewed the message.
    MessageRead {
        /// Relational context of the message
        context_id: ContextId,
        /// Channel containing the message
        channel_id: ChannelId,
        /// Message that was read
        message_id: String,
        /// Authority that read the message
        reader_id: AuthorityId,
        /// Timestamp when message was read (uses unified time system)
        read_at: PhysicalTime,
    },
    /// Message edited (Category A operation - optimistic)
    ///
    /// Edit facts are append-only - the original message is not modified.
    /// Clients reduce by displaying the latest edit for each message_id.
    MessageEdited {
        /// Relational context of the message
        context_id: ContextId,
        /// Channel containing the message
        channel_id: ChannelId,
        /// Message being edited
        message_id: String,
        /// Authority that edited the message (must be original sender)
        editor_id: AuthorityId,
        /// New content (opaque bytes, typically UTF-8)
        new_payload: Vec<u8>,
        /// Timestamp when message was edited
        edited_at: PhysicalTime,
    },
    /// Message deleted (Category B operation - deferred approval may apply)
    ///
    /// Delete facts mark a message as deleted. The original message remains
    /// in the journal but clients should not display deleted messages.
    MessageDeleted {
        /// Relational context of the message
        context_id: ContextId,
        /// Channel containing the message
        channel_id: ChannelId,
        /// Message being deleted
        message_id: String,
        /// Authority that deleted the message
        deleter_id: AuthorityId,
        /// Timestamp when message was deleted
        deleted_at: PhysicalTime,
    },
}

impl ChatFact {
    /// Get the timestamp in milliseconds (backward compatibility)
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            ChatFact::ChannelCreated { created_at, .. } => created_at.ts_ms,
            ChatFact::ChannelClosed { closed_at, .. } => closed_at.ts_ms,
            ChatFact::ChannelUpdated { updated_at, .. } => updated_at.ts_ms,
            ChatFact::MessageSentSealed { sent_at, .. } => sent_at.ts_ms,
            ChatFact::MessageRead { read_at, .. } => read_at.ts_ms,
            ChatFact::MessageEdited { edited_at, .. } => edited_at.ts_ms,
            ChatFact::MessageDeleted { deleted_at, .. } => deleted_at.ts_ms,
        }
    }

    /// Validate that this fact can be reduced under the provided context.
    pub fn validate_for_reduction(&self, context_id: ContextId) -> bool {
        self.context_id() == context_id
    }

    /// Derive the relational binding subtype and key data for this fact.
    pub fn binding_key(&self) -> ChatFactKey {
        match self {
            ChatFact::ChannelCreated { channel_id, .. } => ChatFactKey {
                sub_type: "channel-created",
                data: channel_id.to_string().into_bytes(),
            },
            ChatFact::ChannelClosed { channel_id, .. } => ChatFactKey {
                sub_type: "channel-closed",
                data: channel_id.to_string().into_bytes(),
            },
            ChatFact::ChannelUpdated { channel_id, .. } => ChatFactKey {
                sub_type: "channel-updated",
                data: channel_id.to_string().into_bytes(),
            },
            ChatFact::MessageSentSealed { message_id, .. } => ChatFactKey {
                sub_type: "message-sent",
                data: message_id.as_bytes().to_vec(),
            },
            ChatFact::MessageRead { message_id, .. } => ChatFactKey {
                sub_type: "message-read",
                data: message_id.as_bytes().to_vec(),
            },
            ChatFact::MessageEdited { message_id, .. } => ChatFactKey {
                sub_type: "message-edited",
                data: message_id.as_bytes().to_vec(),
            },
            ChatFact::MessageDeleted { message_id, .. } => ChatFactKey {
                sub_type: "message-deleted",
                data: message_id.as_bytes().to_vec(),
            },
        }
    }

    /// Create a ChannelCreated fact with millisecond timestamp (backward compatibility)
    pub fn channel_created_ms(
        context_id: ContextId,
        channel_id: ChannelId,
        name: String,
        topic: Option<String>,
        is_dm: bool,
        created_at_ms: u64,
        creator_id: AuthorityId,
    ) -> Self {
        Self::ChannelCreated {
            context_id,
            channel_id,
            name,
            topic,
            is_dm,
            created_at: PhysicalTime {
                ts_ms: created_at_ms,
                uncertainty: None,
            },
            creator_id,
        }
    }

    /// Create a ChannelClosed fact with millisecond timestamp (backward compatibility)
    pub fn channel_closed_ms(
        context_id: ContextId,
        channel_id: ChannelId,
        closed_at_ms: u64,
        actor_id: AuthorityId,
    ) -> Self {
        Self::ChannelClosed {
            context_id,
            channel_id,
            closed_at: PhysicalTime {
                ts_ms: closed_at_ms,
                uncertainty: None,
            },
            actor_id,
        }
    }

    /// Create a ChannelUpdated fact with millisecond timestamp.
    pub fn channel_updated_ms(
        context_id: ContextId,
        channel_id: ChannelId,
        name: Option<String>,
        topic: Option<String>,
        updated_at_ms: u64,
        actor_id: AuthorityId,
    ) -> Self {
        Self::ChannelUpdated {
            context_id,
            channel_id,
            name,
            topic,
            updated_at: PhysicalTime {
                ts_ms: updated_at_ms,
                uncertainty: None,
            },
            actor_id,
        }
    }

    /// Create a MessageSentSealed fact with millisecond timestamp.
    #[allow(clippy::too_many_arguments)]
    pub fn message_sent_sealed_ms(
        context_id: ContextId,
        channel_id: ChannelId,
        message_id: String,
        sender_id: AuthorityId,
        sender_name: String,
        payload: Vec<u8>,
        sent_at_ms: u64,
        reply_to: Option<String>,
        epoch_hint: Option<u32>,
    ) -> Self {
        Self::MessageSentSealed {
            context_id,
            channel_id,
            message_id,
            sender_id,
            sender_name,
            payload,
            sent_at: PhysicalTime {
                ts_ms: sent_at_ms,
                uncertainty: None,
            },
            reply_to,
            epoch_hint,
        }
    }

    /// Create a MessageRead fact with millisecond timestamp (backward compatibility)
    pub fn message_read_ms(
        context_id: ContextId,
        channel_id: ChannelId,
        message_id: String,
        reader_id: AuthorityId,
        read_at_ms: u64,
    ) -> Self {
        Self::MessageRead {
            context_id,
            channel_id,
            message_id,
            reader_id,
            read_at: PhysicalTime {
                ts_ms: read_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a MessageEdited fact with millisecond timestamp (Category A operation)
    ///
    /// Edit facts are append-only - clients display the latest edit for each message.
    pub fn message_edited_ms(
        context_id: ContextId,
        channel_id: ChannelId,
        message_id: String,
        editor_id: AuthorityId,
        new_payload: Vec<u8>,
        edited_at_ms: u64,
    ) -> Self {
        Self::MessageEdited {
            context_id,
            channel_id,
            message_id,
            editor_id,
            new_payload,
            edited_at: PhysicalTime {
                ts_ms: edited_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a MessageDeleted fact with millisecond timestamp (Category B operation)
    ///
    /// Delete facts mark a message as deleted - clients should not display deleted messages.
    pub fn message_deleted_ms(
        context_id: ContextId,
        channel_id: ChannelId,
        message_id: String,
        deleter_id: AuthorityId,
        deleted_at_ms: u64,
    ) -> Self {
        Self::MessageDeleted {
            context_id,
            channel_id,
            message_id,
            deleter_id,
            deleted_at: PhysicalTime {
                ts_ms: deleted_at_ms,
                uncertainty: None,
            },
        }
    }
}

/// Reducer for chat facts
///
/// Converts chat facts to relational bindings during journal reduction.
pub struct ChatFactReducer;

impl FactReducer for ChatFactReducer {
    fn handles_type(&self) -> &'static str {
        CHAT_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != CHAT_FACT_TYPE_ID {
            return None;
        }

        let fact = ChatFact::from_bytes(binding_data)?;

        if !fact.validate_for_reduction(context_id) {
            return None;
        }

        let key = fact.binding_key();

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(key.sub_type.to_string()),
            context_id,
            data: key.data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([42u8; 32])
    }

    fn test_channel_id() -> ChannelId {
        ChannelId::from_bytes([1u8; 32])
    }

    fn test_authority_id() -> AuthorityId {
        AuthorityId::new_from_entropy([2u8; 32])
    }

    #[test]
    fn test_chat_fact_serialization() {
        let fact = ChatFact::channel_created_ms(
            test_context_id(),
            test_channel_id(),
            "general".to_string(),
            Some("General discussion".to_string()),
            false,
            1234567890,
            test_authority_id(),
        );

        let bytes = fact.to_bytes();
        let restored = ChatFact::from_bytes(&bytes);
        assert!(restored.is_some());
        assert_eq!(restored.unwrap(), fact);
    }

    #[test]
    fn reducer_rejects_context_mismatch() {
        let reducer = ChatFactReducer;
        let fact = ChatFact::channel_created_ms(
            test_context_id(),
            test_channel_id(),
            "general".to_string(),
            None,
            false,
            123,
            test_authority_id(),
        );

        // Reduce under a different context id should be rejected.
        let other_context = ContextId::new_from_entropy([43u8; 32]);
        let binding = reducer.reduce(other_context, CHAT_FACT_TYPE_ID, &fact.to_bytes());
        assert!(binding.is_none());
    }

    #[test]
    fn test_chat_fact_to_generic() {
        let fact = ChatFact::message_sent_sealed_ms(
            test_context_id(),
            test_channel_id(),
            "msg-123".to_string(),
            test_authority_id(),
            "Alice".to_string(),
            b"Hello, world!".to_vec(),
            1234567890,
            None,
            None, // epoch_hint
        );

        let generic = fact.to_generic();

        if let aura_journal::RelationalFact::Generic {
            binding_type,
            binding_data,
            ..
        } = generic
        {
            assert_eq!(binding_type, CHAT_FACT_TYPE_ID);
            let restored = ChatFact::from_bytes(&binding_data);
            assert!(restored.is_some());
        } else {
            panic!("Expected Generic variant");
        }
    }

    #[test]
    fn test_chat_fact_reducer() {
        let reducer = ChatFactReducer;
        assert_eq!(reducer.handles_type(), CHAT_FACT_TYPE_ID);

        let fact = ChatFact::channel_created_ms(
            test_context_id(),
            test_channel_id(),
            "test".to_string(),
            None,
            false,
            0,
            test_authority_id(),
        );

        let bytes = fact.to_bytes();
        let binding = reducer.reduce(test_context_id(), CHAT_FACT_TYPE_ID, &bytes);

        assert!(binding.is_some());
        let binding = binding.unwrap();
        assert!(matches!(
            binding.binding_type,
            RelationalBindingType::Generic(ref s) if s == "channel-created"
        ));
    }

    #[test]
    fn test_type_id_consistency() {
        let facts = vec![
            ChatFact::channel_created_ms(
                test_context_id(),
                test_channel_id(),
                "test".to_string(),
                None,
                false,
                0,
                test_authority_id(),
            ),
            ChatFact::channel_closed_ms(
                test_context_id(),
                test_channel_id(),
                0,
                test_authority_id(),
            ),
            ChatFact::message_sent_sealed_ms(
                test_context_id(),
                test_channel_id(),
                "msg".to_string(),
                test_authority_id(),
                "Test".to_string(),
                b"Hello".to_vec(),
                0,
                None,
                None, // epoch_hint
            ),
            ChatFact::message_read_ms(
                test_context_id(),
                test_channel_id(),
                "msg".to_string(),
                test_authority_id(),
                0,
            ),
        ];

        for fact in facts {
            assert_eq!(fact.type_id(), CHAT_FACT_TYPE_ID);
        }
    }

    #[test]
    fn test_reducer_idempotence() {
        let reducer = ChatFactReducer;
        let context_id = test_context_id();
        let fact = ChatFact::channel_created_ms(
            context_id,
            test_channel_id(),
            "general".to_string(),
            None,
            false,
            0,
            test_authority_id(),
        );

        let bytes = fact.to_bytes();
        let binding1 = reducer.reduce(context_id, CHAT_FACT_TYPE_ID, &bytes);
        let binding2 = reducer.reduce(context_id, CHAT_FACT_TYPE_ID, &bytes);
        assert!(binding1.is_some());
        assert!(binding2.is_some());
        let binding1 = binding1.unwrap();
        let binding2 = binding2.unwrap();
        assert_eq!(binding1.binding_type, binding2.binding_type);
        assert_eq!(binding1.context_id, binding2.context_id);
        assert_eq!(binding1.data, binding2.data);
    }

    #[test]
    fn test_message_lifecycle_facts() {
        // Test the message lifecycle: Sent -> Read
        let context = test_context_id();
        let channel = test_channel_id();
        let message_id = "lifecycle-msg-001".to_string();
        let sender = test_authority_id();
        let reader = AuthorityId::new_from_entropy([3u8; 32]);

        // 1. Message sent
        let sent = ChatFact::message_sent_sealed_ms(
            context,
            channel,
            message_id.clone(),
            sender,
            "Sender".to_string(),
            b"Hello".to_vec(),
            1000,
            None,
            Some(1), // epoch_hint
        );
        assert_eq!(sent.timestamp_ms(), 1000);

        // 2. Message read by recipient
        let read = ChatFact::message_read_ms(context, channel, message_id, reader, 3000);
        assert_eq!(read.timestamp_ms(), 3000);

        // All facts should have the same context
        assert_eq!(sent.context_id(), context);
        assert_eq!(read.context_id(), context);
    }
}
