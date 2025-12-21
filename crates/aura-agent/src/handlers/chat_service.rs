//! Chat Service - Public API for Chat Operations
//!
//! Provides a clean public interface for chat operations at the agent layer.
//! Wraps `aura_chat::ChatHandler` with RwLock management and proper error handling.
//!
//! This service follows the two-layer architecture:
//! - Layer 5 (Domain): `aura_chat::ChatHandler` - stateless handler with per-call effects
//! - Layer 6 (Agent): `ChatService` - RwLock wrapper for effect system access

use crate::core::{AgentError, AgentResult};
use crate::runtime::AuraEffectSystem;
use aura_chat::{ChatGroup, ChatGroupId, ChatHandler, ChatMessage, ChatMessageId};
use aura_core::identifiers::AuthorityId;
use aura_core::time::TimeStamp;
use std::collections::HashMap;
use std::sync::Arc;

/// Chat service for the agent layer
///
/// Provides chat operations through a clean public API.
/// Wraps the domain-layer `ChatHandler` with RwLock management.
pub struct ChatService {
    handler: ChatHandler,
    effects: Arc<AuraEffectSystem>,
}

impl ChatService {
    /// Create a new chat service
    pub fn new(effects: Arc<AuraEffectSystem>) -> Self {
        Self {
            handler: ChatHandler::new(),
            effects,
        }
    }

    /// Create a new chat group with the given name and initial members
    ///
    /// # Arguments
    /// * `name` - Human-readable name for the group
    /// * `creator_id` - Authority ID of the group creator (becomes admin)
    /// * `initial_members` - List of authority IDs to invite to the group
    ///
    /// # Returns
    /// Created ChatGroup with generated ID and timestamps
    pub async fn create_group(
        &self,
        name: &str,
        creator_id: AuthorityId,
        initial_members: Vec<AuthorityId>,
    ) -> AgentResult<ChatGroup> {
        self.handler
            .create_group(&*self.effects, name, creator_id, initial_members)
            .await
            .map_err(AgentError::from)
    }

    /// Send a message to a chat group
    ///
    /// # Arguments
    /// * `group_id` - Target group to send message to
    /// * `sender_id` - Authority ID of message sender (must be group member)
    /// * `content` - Message content to send
    ///
    /// # Returns
    /// Created ChatMessage with generated ID and timestamp
    pub async fn send_message(
        &self,
        group_id: &ChatGroupId,
        sender_id: AuthorityId,
        content: String,
    ) -> AgentResult<ChatMessage> {
        self.handler
            .send_message(&*self.effects, group_id, sender_id, content)
            .await
            .map_err(AgentError::from)
    }

    /// Get message history for a group
    ///
    /// # Arguments
    /// * `group_id` - Group to retrieve history for
    /// * `limit` - Maximum number of messages to return
    /// * `before` - Only return messages before this timestamp (for pagination)
    ///
    /// # Returns
    /// Vector of ChatMessage in chronological order
    pub async fn get_history(
        &self,
        group_id: &ChatGroupId,
        limit: Option<usize>,
        before: Option<TimeStamp>,
    ) -> AgentResult<Vec<ChatMessage>> {
        self.handler
            .get_history(&*self.effects, group_id, limit, before)
            .await
            .map_err(AgentError::from)
    }

    /// Get a chat group by ID
    ///
    /// # Arguments
    /// * `group_id` - Group ID to retrieve
    ///
    /// # Returns
    /// Option<ChatGroup> if found, None if group doesn't exist
    pub async fn get_group(&self, group_id: &ChatGroupId) -> AgentResult<Option<ChatGroup>> {
        ChatHandler::get_group(&*self.effects, group_id)
            .await
            .map_err(AgentError::from)
    }

    /// List groups that an authority is a member of
    ///
    /// # Arguments
    /// * `authority_id` - Authority to list groups for
    ///
    /// # Returns
    /// Vector of ChatGroup that the authority is a member of
    pub async fn list_user_groups(
        &self,
        authority_id: &AuthorityId,
    ) -> AgentResult<Vec<ChatGroup>> {
        self.handler
            .list_user_groups(&*self.effects, authority_id)
            .await
            .map_err(AgentError::from)
    }

    /// Add a member to a chat group
    ///
    /// # Arguments
    /// * `group_id` - Group to add member to
    /// * `authority_id` - Authority performing the add (must be admin)
    /// * `new_member` - Authority to add to the group
    pub async fn add_member(
        &self,
        group_id: &ChatGroupId,
        authority_id: AuthorityId,
        new_member: AuthorityId,
    ) -> AgentResult<()> {
        self.handler
            .add_member(&*self.effects, group_id, authority_id, new_member)
            .await
            .map_err(AgentError::from)
    }

    /// Remove a member from a chat group
    ///
    /// # Arguments
    /// * `group_id` - Group to remove member from
    /// * `authority_id` - Authority performing the removal (must be admin or self)
    /// * `member_to_remove` - Authority to remove from the group
    pub async fn remove_member(
        &self,
        group_id: &ChatGroupId,
        authority_id: AuthorityId,
        member_to_remove: AuthorityId,
    ) -> AgentResult<()> {
        self.handler
            .remove_member(&*self.effects, group_id, authority_id, member_to_remove)
            .await
            .map_err(AgentError::from)
    }

    /// Retrieve a single message by ID
    ///
    /// # Arguments
    /// * `message_id` - Message ID to retrieve
    ///
    /// # Returns
    /// Option<ChatMessage> if found
    pub async fn get_message(
        &self,
        message_id: &ChatMessageId,
    ) -> AgentResult<Option<ChatMessage>> {
        ChatHandler::get_message(&*self.effects, message_id)
            .await
            .map_err(AgentError::from)
    }

    /// Edit an existing message (sender or admin only)
    ///
    /// # Arguments
    /// * `group_id` - Group containing the message
    /// * `editor` - Authority editing the message
    /// * `message_id` - Message to edit
    /// * `new_content` - New message content
    pub async fn edit_message(
        &self,
        group_id: &ChatGroupId,
        editor: AuthorityId,
        message_id: &ChatMessageId,
        new_content: &str,
    ) -> AgentResult<ChatMessage> {
        self.handler
            .edit_message(&*self.effects, group_id, editor, message_id, new_content)
            .await
            .map_err(AgentError::from)
    }

    /// Soft-delete a message (sender or admin only)
    ///
    /// # Arguments
    /// * `group_id` - Group containing the message
    /// * `requester` - Authority requesting deletion
    /// * `message_id` - Message to delete
    pub async fn delete_message(
        &self,
        group_id: &ChatGroupId,
        requester: AuthorityId,
        message_id: &ChatMessageId,
    ) -> AgentResult<()> {
        self.handler
            .delete_message(&*self.effects, group_id, requester, message_id)
            .await
            .map_err(AgentError::from)
    }

    /// Search messages by substring across a group
    ///
    /// # Arguments
    /// * `group_id` - Group to search in
    /// * `query` - Search query string
    /// * `limit` - Maximum number of results
    /// * `sender` - Optional filter by sender
    pub async fn search_messages(
        &self,
        group_id: &ChatGroupId,
        query: &str,
        limit: usize,
        sender: Option<&AuthorityId>,
    ) -> AgentResult<Vec<ChatMessage>> {
        self.handler
            .search_messages(&*self.effects, group_id, query, limit, sender)
            .await
            .map_err(AgentError::from)
    }

    /// Update group metadata (name/description/metadata)
    ///
    /// # Arguments
    /// * `group_id` - Group to update
    /// * `requester` - Authority requesting the update (must be admin)
    /// * `name` - Optional new name
    /// * `description` - Optional new description
    /// * `metadata` - Optional metadata key-value pairs to add/update
    pub async fn update_group_details(
        &self,
        group_id: &ChatGroupId,
        requester: AuthorityId,
        name: Option<String>,
        description: Option<String>,
        metadata: Option<HashMap<String, String>>,
    ) -> AgentResult<ChatGroup> {
        self.handler
            .update_group_details(&*self.effects, group_id, requester, name, description, metadata)
            .await
            .map_err(AgentError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;

    #[tokio::test]
    async fn test_chat_service_creation() {
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));

        let _service = ChatService::new(effects);
        // Service created successfully
    }

    #[tokio::test]
    async fn test_create_group_via_service() {
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let service = ChatService::new(effects);

        let creator_id = AuthorityId::new_from_entropy([1u8; 32]);
        let member_id = AuthorityId::new_from_entropy([2u8; 32]);

        let group = service
            .create_group("Test Group", creator_id, vec![member_id])
            .await
            .unwrap();

        assert_eq!(group.name, "Test Group");
        assert!(group.is_member(&creator_id));
        assert!(group.is_member(&member_id));
    }

    #[tokio::test]
    async fn test_send_and_retrieve_message() {
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let service = ChatService::new(effects);

        let creator_id = AuthorityId::new_from_entropy([3u8; 32]);
        let group = service
            .create_group("Chat Room", creator_id, vec![])
            .await
            .unwrap();

        let message = service
            .send_message(&group.id, creator_id, "Hello, world!".to_string())
            .await
            .unwrap();

        assert_eq!(message.content, "Hello, world!");
        assert_eq!(message.sender_id, creator_id);

        // Retrieve history
        let history = service.get_history(&group.id, None, None).await.unwrap();
        assert!(!history.is_empty());
    }

    #[tokio::test]
    async fn test_list_user_groups() {
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let service = ChatService::new(effects);

        let user_id = AuthorityId::new_from_entropy([4u8; 32]);

        // Create two groups
        service
            .create_group("Group A", user_id, vec![])
            .await
            .unwrap();
        service
            .create_group("Group B", user_id, vec![])
            .await
            .unwrap();

        let groups = service.list_user_groups(&user_id).await.unwrap();
        assert!(
            groups.len() >= 2,
            "expected at least 2 groups, found {}",
            groups.len()
        );
    }
}
