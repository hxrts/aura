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
use serde::{Deserialize, Serialize};

/// Type identifier for chat facts
pub const CHAT_FACT_TYPE_ID: &str = "chat";

/// Chat domain fact types
///
/// These facts represent chat-related state changes in the journal.
/// They are stored as `RelationalFact::Generic` and reduced by `ChatFactReducer`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChatFact {
    /// Channel created in a relational context
    ChannelCreated {
        /// Relational context (block/group) where the channel exists
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
    /// Message sent in a channel
    MessageSent {
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
        /// Message content (plaintext; encryption handled at transport layer)
        content: String,
        /// Timestamp when message was sent (uses unified time system)
        sent_at: PhysicalTime,
        /// Optional message ID this is replying to
        reply_to: Option<String>,
    },
    /// Message read by an authority
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
}

impl ChatFact {
    /// Get the timestamp in milliseconds (backward compatibility)
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            ChatFact::ChannelCreated { created_at, .. } => created_at.ts_ms,
            ChatFact::ChannelClosed { closed_at, .. } => closed_at.ts_ms,
            ChatFact::MessageSent { sent_at, .. } => sent_at.ts_ms,
            ChatFact::MessageRead { read_at, .. } => read_at.ts_ms,
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

    /// Create a MessageSent fact with millisecond timestamp (backward compatibility)
    #[allow(clippy::too_many_arguments)]
    pub fn message_sent_ms(
        context_id: ContextId,
        channel_id: ChannelId,
        message_id: String,
        sender_id: AuthorityId,
        sender_name: String,
        content: String,
        sent_at_ms: u64,
        reply_to: Option<String>,
    ) -> Self {
        Self::MessageSent {
            context_id,
            channel_id,
            message_id,
            sender_id,
            sender_name,
            content,
            sent_at: PhysicalTime {
                ts_ms: sent_at_ms,
                uncertainty: None,
            },
            reply_to,
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
}

impl DomainFact for ChatFact {
    fn type_id(&self) -> &'static str {
        CHAT_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        match self {
            ChatFact::ChannelCreated { context_id, .. } => *context_id,
            ChatFact::ChannelClosed { context_id, .. } => *context_id,
            ChatFact::MessageSent { context_id, .. } => *context_id,
            ChatFact::MessageRead { context_id, .. } => *context_id,
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self>
    where
        Self: Sized,
    {
        serde_json::from_slice(bytes).ok()
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

        let fact: ChatFact = serde_json::from_slice(binding_data).ok()?;

        let (sub_type, data) = match &fact {
            ChatFact::ChannelCreated { channel_id, .. } => (
                "channel-created".to_string(),
                channel_id.to_string().into_bytes(),
            ),
            ChatFact::ChannelClosed { channel_id, .. } => (
                "channel-closed".to_string(),
                channel_id.to_string().into_bytes(),
            ),
            ChatFact::MessageSent { message_id, .. } => {
                ("message-sent".to_string(), message_id.as_bytes().to_vec())
            }
            ChatFact::MessageRead { message_id, .. } => {
                ("message-read".to_string(), message_id.as_bytes().to_vec())
            }
        };

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(sub_type),
            context_id,
            data,
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
    fn test_chat_fact_to_generic() {
        let fact = ChatFact::message_sent_ms(
            test_context_id(),
            test_channel_id(),
            "msg-123".to_string(),
            test_authority_id(),
            "Alice".to_string(),
            "Hello, world!".to_string(),
            1234567890,
            None,
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
            ChatFact::message_sent_ms(
                test_context_id(),
                test_channel_id(),
                "msg".to_string(),
                test_authority_id(),
                "Test".to_string(),
                "Hello".to_string(),
                0,
                None,
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
}
