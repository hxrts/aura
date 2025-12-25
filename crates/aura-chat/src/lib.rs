//! # Aura Chat
//!
//! Layer 5 feature crate for chat-related domain facts and guard-compatible services.
//!
//! ## What belongs here
//!
//! - **Fact-first chat**: `ChatFact` + `ChatFactReducer` encode chat state transitions as
//!   append-only facts stored via `RelationalFact::Generic`. Message payloads are represented as
//!   **opaque bytes** (`MessageSentSealed`); rendering/decryption is a higher-layer concern.
//! - **View reduction**: `ChatViewReducer` produces UI deltas from facts without assuming plaintext.
//! - **Guard-compatible service**: `ChatFactService` provides guard chain integration for all
//!   chat operations (capability checks, flow budget charging, fact emission).
//!
//! ## Canonical API
//!
//! Use `ChatFactService` for all chat operations. It integrates with:
//! - Capability guards (`CAP_CHAT_CHANNEL_CREATE`, `CAP_CHAT_MESSAGE_SEND`)
//! - Flow budget charging (`CHAT_CHANNEL_CREATE_COST`, `CHAT_MESSAGE_SEND_COST`)
//! - Journal fact emission via `EffectCommand::JournalAppend`
//!
//! The actual runtime integration is in `aura-agent/src/handlers/chat_service.rs`.
//!
//! ## Operation Categories (per docs/117_operation_categories.md)
//!
//! Chat operations follow the three-tier classification:
//!
//! | Operation | Category | Notes |
//! |-----------|----------|-------|
//! | Send message | A (Optimistic) | Keys already derived from context |
//! | Create channel | A (Optimistic) | Emit `ChannelCreated` fact |
//! | Add channel member | A (Optimistic) | If member already in relational context |
//! | Update topic | A (Optimistic) | CRDT, last-write-wins |
//! | Change permissions | B (Deferred) | Requires approval workflow |
//! | Kick from channel | B (Deferred) | May need approval |
//! | Create group | C (Blocking) | Multi-party key agreement ceremony |
//! | Add member to group | C (Blocking) | Changes group keys, requires ceremony |
//!
//! **Key insight**: Adding to a **channel** within existing context = Category A.
//! Adding to a **group** (new context relationship) = Category C ceremony
//! (see `docs/118_key_rotation_ceremonies.md`).
//!
//! ## Effect system compliance
//!
//! - Uses injected effect traits (no direct OS calls).
//! - Uses unified time via `PhysicalTimeEffects` (no legacy `TimeEffects`).
//!
//! ## Example
//!
//! ```ignore
//! use aura_chat::ChatFact;
//! use aura_journal::DomainFact;
//!
//! // Domain fact → Generic for storage in the journal
//! let fact = ChatFact::message_sent_sealed_ms(
//!     /* context_id */ todo!(),
//!     /* channel_id */ todo!(),
//!     "msg-123".to_string(),
//!     /* sender_id */ todo!(),
//!     "Alice".to_string(),
//!     b"opaque bytes".to_vec(),
//!     /* sent_at_ms */ 0,
//!     None,
//! );
//! let _generic = fact.to_generic();
//! ```

use aura_core::AuraError;

pub mod fact_service;
pub mod facts;
pub mod group;
pub mod guards;
pub mod types;
pub mod view;

/// Operation category map (A/B/C) for protocol gating and review.
pub const OPERATION_CATEGORIES: &[(&str, &str)] = &[
    ("chat:send-message", "A"),
    ("chat:create-channel", "A"),
    ("chat:add-member", "A"),
    ("chat:update-topic", "A"),
    ("chat:change-permissions", "B"),
    ("chat:kick-member", "B"),
    ("chat:create-group", "C"),
    ("chat:add-member-group", "C"),
];

pub use fact_service::ChatFactService;
pub use facts::{ChatFact, ChatFactReducer, CHAT_FACT_TYPE_ID};
pub use group::ChatGroup;
pub use types::*;
pub use view::{ChatDelta, ChatViewReducer};

/// Chat-specific errors
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    /// Group not found
    #[error("Chat group not found: {group_id}")]
    GroupNotFound {
        /// The group identifier that was not found
        group_id: String,
    },

    /// User not authorized for chat operation
    #[error("Not authorized for chat operation: {reason}")]
    NotAuthorized {
        /// Reason for authorization failure
        reason: String,
    },

    /// Message not found
    #[error("Message not found: {message_id}")]
    MessageNotFound {
        /// The message identifier that was not found
        message_id: String,
    },

    /// Invalid group configuration
    #[error("Invalid group configuration: {reason}")]
    InvalidGroup {
        /// Reason for invalid configuration
        reason: String,
    },

    /// Transport layer error
    #[error("Transport error: {error}")]
    Transport {
        /// Transport error message
        error: String,
    },

    /// Serialization error
    #[error("Serialization error: {error}")]
    Serialization {
        /// Serialization error message
        error: String,
    },

    /// Core Aura error
    #[error("Core error: {0}")]
    Core(#[from] AuraError),
}

impl From<ChatError> for AuraError {
    fn from(error: ChatError) -> Self {
        match error {
            ChatError::GroupNotFound { group_id } => {
                AuraError::not_found(format!("Chat group not found: {}", group_id))
            }
            ChatError::NotAuthorized { reason } => AuraError::permission_denied(reason),
            ChatError::MessageNotFound { message_id } => {
                AuraError::not_found(format!("Message not found: {}", message_id))
            }
            ChatError::InvalidGroup { reason } => AuraError::invalid(reason),
            ChatError::Transport { error } => AuraError::network(error),
            ChatError::Serialization { error } => AuraError::serialization(error),
            ChatError::Core(aura_error) => aura_error,
        }
    }
}
