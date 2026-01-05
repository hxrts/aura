//! Chat data types and structures

use aura_core::identifiers::AuthorityId;
use aura_core::time::TimeStamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Re-export consistency types for convenience
pub use aura_core::domain::{
    Acknowledgment, Agreement, ApprovalProgress, ApprovalThreshold, Consistency, ConsistencyMap,
    DeferredStatus, OperationCategory, OptimisticStatus, Propagation, ProposalState,
};

// ============================================================================
// Chat Types
// ============================================================================

/// Unique identifier for a chat group
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChatGroupId(pub Uuid);

impl ChatGroupId {
    /// Create from UUID (typically from RandomEffects)
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Get the underlying UUID
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl std::fmt::Display for ChatGroupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a chat message
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChatMessageId(pub Uuid);

impl ChatMessageId {
    /// Create from UUID (typically from RandomEffects)
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Get the underlying UUID
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl std::fmt::Display for ChatMessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Chat group member information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMember {
    /// Authority ID of the member
    pub authority_id: AuthorityId,
    /// What the member wants to be called (their nickname suggestion)
    pub nickname_suggestion: String,
    /// When the member joined the group (using unified time system)
    pub joined_at: TimeStamp,
    /// Role in the group (admin, member, etc.)
    pub role: ChatRole,
}

/// Role of a member in a chat group
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatRole {
    /// Group administrator with full permissions
    Admin,
    /// Regular group member
    Member,
    /// Read-only member
    Observer,
}

/// Type of chat message
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageType {
    /// Regular text message
    Text,
    /// System message (member joined, etc.)
    System,
    /// Message was edited
    Edit,
    /// Message was deleted
    Delete,
}

/// Individual chat message
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Unique message identifier
    pub id: ChatMessageId,
    /// Group this message belongs to
    pub group_id: ChatGroupId,
    /// Authority that sent the message
    pub sender_id: AuthorityId,
    /// Message content
    pub content: String,
    /// Type of message
    pub message_type: MessageType,
    /// When the message was sent (using unified time system)
    pub timestamp: TimeStamp,
    /// Optional message this is a reply to
    pub reply_to: Option<ChatMessageId>,
    /// Message metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl ChatMessage {
    /// Create a new text message with provided IDs and timestamp (from effects)
    pub fn new_text(
        id: ChatMessageId,
        group_id: ChatGroupId,
        sender_id: AuthorityId,
        content: String,
        timestamp: TimeStamp,
    ) -> Self {
        Self {
            id,
            group_id,
            sender_id,
            content,
            message_type: MessageType::Text,
            timestamp,
            reply_to: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Create a system message with provided IDs and timestamp (from effects)
    pub fn new_system(
        id: ChatMessageId,
        group_id: ChatGroupId,
        system_authority: AuthorityId,
        content: String,
        timestamp: TimeStamp,
    ) -> Self {
        Self {
            id,
            group_id,
            sender_id: system_authority,
            content,
            message_type: MessageType::System,
            timestamp,
            reply_to: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Check if this is a system message
    pub fn is_system(&self) -> bool {
        matches!(self.message_type, MessageType::System)
    }

    /// Set reply target
    pub fn set_reply_to(mut self, reply_to: ChatMessageId) -> Self {
        self.reply_to = Some(reply_to);
        self
    }

    /// Add metadata
    pub fn add_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}
