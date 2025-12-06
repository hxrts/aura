//! Chat Handler - Core chat functionality and operations
//!
//! This module implements the main chat handler that coordinates
//! group management, messaging, and integration with the effect system.
//!
//! The handler follows the per-call effects pattern where each method
//! takes an effect reference rather than storing it in the struct.
//! This enables clean RwLock integration at the agent layer.

use crate::{
    types::{ChatMember, ChatRole},
    ChatGroup, ChatGroupId, ChatMessage, ChatMessageId,
};
use aura_core::{
    effects::{PhysicalTimeEffects, RandomEffects, StorageEffects},
    identifiers::AuthorityId,
    time::TimeStamp,
    AuraError, Result,
};
use std::collections::HashMap;
use uuid::Uuid;

/// Core chat handler that manages groups, messages, and integrates with AMP
///
/// This handler is stateless and takes effect references per-call,
/// following the unified service pattern for domain crates.
///
/// # Example
///
/// ```ignore
/// let handler = ChatHandler::new();
/// let group = handler.create_group(&effects, "My Group", creator_id, vec![]).await?;
/// ```
#[derive(Debug, Clone, Default)]
pub struct ChatHandler;

impl ChatHandler {
    fn map_time_err(err: aura_core::effects::time::TimeError) -> AuraError {
        AuraError::internal(format!("time error: {err}"))
    }

    fn display_name(authority_id: &AuthorityId) -> String {
        let id_str = authority_id.to_string();
        let short = &id_str[..12.min(id_str.len())];
        format!("{}...", short)
    }

    /// Create a new chat handler
    pub fn new() -> Self {
        Self
    }

    /// Create a new chat group with the given name and initial members
    ///
    /// # Arguments
    /// * `effects` - Effect system reference for storage, random, and time operations
    /// * `name` - Human-readable name for the group
    /// * `creator_id` - Authority ID of the group creator (becomes admin)
    /// * `initial_members` - List of authority IDs to invite to the group
    ///
    /// # Returns
    /// Created ChatGroup with generated ID and timestamps
    pub async fn create_group<E>(
        &self,
        effects: &E,
        name: &str,
        creator_id: AuthorityId,
        initial_members: Vec<AuthorityId>,
    ) -> Result<ChatGroup>
    where
        E: StorageEffects + RandomEffects + PhysicalTimeEffects + Send + Sync,
    {
        // Generate ID using RandomEffects (following effect system guidelines)
        let group_uuid = effects.random_uuid().await;
        let group_id = ChatGroupId::from_uuid(group_uuid);

        // Get timestamp using PhysicalTimeEffects (unified time system)
        let physical_time = effects.physical_time().await.map_err(Self::map_time_err)?;
        let now = TimeStamp::PhysicalClock(physical_time);

        // Create group metadata
        let mut members = Vec::new();

        // Creator becomes admin
        members.push(ChatMember {
            authority_id: creator_id,
            display_name: Self::display_name(&creator_id),
            joined_at: now.clone(),
            role: ChatRole::Admin,
        });

        // Add initial members
        for member_id in initial_members {
            if member_id != creator_id {
                members.push(ChatMember {
                    authority_id: member_id,
                    display_name: Self::display_name(&member_id),
                    joined_at: now.clone(),
                    role: ChatRole::Member,
                });
            }
        }

        let group = ChatGroup {
            id: group_id.clone(),
            name: name.to_string(),
            description: String::new(),
            created_at: now,
            created_by: creator_id,
            members,
            metadata: HashMap::new(),
        };

        // Store group data via effects system
        let group_key = format!("chat_group:{}", group_id);
        let group_data = serde_json::to_vec(&group)
            .map_err(|e| AuraError::serialization(format!("Failed to serialize group: {}", e)))?;

        effects
            .store(&group_key, group_data)
            .await
            .map_err(AuraError::from)?;

        // Create system message for group creation using effect system
        let msg_uuid = effects.random_uuid().await;
        let msg_id = ChatMessageId::from_uuid(msg_uuid);
        let msg_physical_time = effects.physical_time().await.map_err(Self::map_time_err)?;
        let msg_timestamp = TimeStamp::PhysicalClock(msg_physical_time);

        let system_msg = ChatMessage::new_system(
            msg_id,
            group_id.clone(),
            creator_id, // Creator acts as system for group creation
            format!("Chat group '{}' created", name),
            msg_timestamp,
        );

        Self::store_message(effects, &system_msg).await?;

        Ok(group)
    }

    /// Send a message to a chat group
    ///
    /// # Arguments
    /// * `effects` - Effect system reference for storage, random, and time operations
    /// * `group_id` - Target group to send message to
    /// * `sender_id` - Authority ID of message sender (must be group member)
    /// * `content` - Message content to send
    ///
    /// # Returns
    /// Created ChatMessage with generated ID and timestamp
    ///
    /// # Errors
    /// * `AuraError::NotFound` if the group doesn't exist
    /// * `AuraError::PermissionDenied` if sender is not a member of the group
    pub async fn send_message<E>(
        &self,
        effects: &E,
        group_id: &ChatGroupId,
        sender_id: AuthorityId,
        content: String,
    ) -> Result<ChatMessage>
    where
        E: StorageEffects + RandomEffects + PhysicalTimeEffects + Send + Sync,
    {
        // Verify sender is a member of the group
        let group = Self::get_group(effects, group_id)
            .await?
            .ok_or_else(|| AuraError::not_found(format!("Chat group not found: {}", group_id)))?;

        if !group.is_member(&sender_id) {
            return Err(AuraError::permission_denied("Not a member of this group"));
        }

        // Create message using effect system
        let msg_uuid = effects.random_uuid().await;
        let msg_id = ChatMessageId::from_uuid(msg_uuid);
        let physical_time = effects.physical_time().await.map_err(Self::map_time_err)?;
        let timestamp = TimeStamp::PhysicalClock(physical_time);

        let message =
            ChatMessage::new_text(msg_id, group_id.clone(), sender_id, content, timestamp);

        // Store message
        Self::store_message(effects, &message).await?;

        // Send message via effect-backed broadcast (storage-indexed outboxes)
        Self::broadcast_message_to_group(effects, &group, &message).await?;

        Ok(message)
    }

    /// Get message history for a group
    ///
    /// # Arguments
    /// * `effects` - Effect system reference for storage operations
    /// * `group_id` - Group to retrieve history for
    /// * `limit` - Maximum number of messages to return (default: 100)
    /// * `before` - Only return messages before this timestamp (for pagination)
    ///
    /// # Returns
    /// Vector of ChatMessage in chronological order
    ///
    /// # Errors
    /// * `AuraError::NotFound` if the group doesn't exist
    pub async fn get_history<E>(
        &self,
        effects: &E,
        group_id: &ChatGroupId,
        limit: Option<usize>,
        before: Option<TimeStamp>,
    ) -> Result<Vec<ChatMessage>>
    where
        E: StorageEffects + Send + Sync,
    {
        let key_prefix = format!("chat_group_message:{}:", group_id);
        let all_keys = effects
            .list_keys(Some(&key_prefix))
            .await
            .map_err(AuraError::from)?;

        let mut entries: Vec<(i64, String)> = Vec::new();
        for key in all_keys {
            // key format: chat_group_message:{group}:{ts}:{id}
            if let Some(ts_part) = key
                .strip_prefix(&key_prefix)
                .and_then(|rest| rest.split(':').next())
            {
                if let Ok(ts) = ts_part.parse::<i64>() {
                    entries.push((ts, key));
                }
            }
        }

        // Apply before filter using unified time conversion
        if let Some(before_ts) = before {
            let cutoff = before_ts.to_index_ms();
            entries.retain(|(ts, _)| *ts < cutoff);
        }

        // Sort descending by timestamp then truncate limit
        entries.sort_by(|a, b| b.0.cmp(&a.0));
        let limit = limit.unwrap_or(100);
        entries.truncate(limit);

        let mut messages = Vec::new();
        for (_, key) in entries.into_iter().rev() {
            // Extract message ID from the index key
            // Key format: chat_group_message:{group_id}:{timestamp}:{message_id}
            if let Some(message_id_str) = key.split(':').next_back() {
                if let Ok(message_id_uuid) = Uuid::parse_str(message_id_str) {
                    let message_id = ChatMessageId(message_id_uuid);
                    if let Ok(Some(msg)) = Self::get_message(effects, &message_id).await {
                        messages.push(msg);
                    }
                }
            }
        }

        Ok(messages)
    }

    /// Get a chat group by ID
    ///
    /// # Arguments
    /// * `effects` - Effect system reference for storage operations
    /// * `group_id` - Group ID to retrieve
    ///
    /// # Returns
    /// Option<ChatGroup> if found, None if group doesn't exist
    pub async fn get_group<E>(effects: &E, group_id: &ChatGroupId) -> Result<Option<ChatGroup>>
    where
        E: StorageEffects + Send + Sync,
    {
        let group_key = format!("chat_group:{}", group_id);

        match effects.retrieve(&group_key).await {
            Ok(Some(data)) => {
                let group: ChatGroup = serde_json::from_slice(&data).map_err(|e| {
                    AuraError::serialization(format!("Failed to deserialize group: {}", e))
                })?;
                Ok(Some(group))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(AuraError::from(e)),
        }
    }

    /// List groups that an authority is a member of
    ///
    /// # Arguments
    /// * `effects` - Effect system reference for storage operations
    /// * `authority_id` - Authority to list groups for
    ///
    /// # Returns
    /// Vector of ChatGroup that the authority is a member of
    pub async fn list_user_groups<E>(
        &self,
        effects: &E,
        authority_id: &AuthorityId,
    ) -> Result<Vec<ChatGroup>>
    where
        E: StorageEffects + Send + Sync,
    {
        // Scan stored groups and collect those containing the authority
        let keys = effects
            .list_keys(Some("chat_group:"))
            .await
            .map_err(AuraError::from)?;

        let mut groups = Vec::new();
        for key in keys {
            if let Ok(Some(raw)) = effects.retrieve(&key).await {
                if let Ok(group) = serde_json::from_slice::<ChatGroup>(&raw) {
                    if group.is_member(authority_id) {
                        groups.push(group);
                    }
                }
            }
        }

        Ok(groups)
    }

    /// Add a member to a chat group
    pub async fn add_member<E>(
        &self,
        effects: &E,
        group_id: &ChatGroupId,
        authority_id: AuthorityId,
        new_member: AuthorityId,
    ) -> Result<()>
    where
        E: StorageEffects + RandomEffects + PhysicalTimeEffects + Send + Sync,
    {
        let mut group = Self::get_group(effects, group_id)
            .await?
            .ok_or_else(|| AuraError::not_found(format!("Chat group not found: {}", group_id)))?;

        // Check if requester has permission to add members
        let requester_member = group
            .members
            .iter()
            .find(|m| m.authority_id == authority_id)
            .ok_or_else(|| AuraError::permission_denied("Not a member of this group"))?;

        if !matches!(requester_member.role, ChatRole::Admin) {
            return Err(AuraError::permission_denied("Only admins can add members"));
        }

        // Check if member is already in group
        if group.is_member(&new_member) {
            return Err(AuraError::invalid("Member already in group"));
        }

        // Add new member with timestamp from effect system
        let joined_physical_time = effects.physical_time().await.map_err(Self::map_time_err)?;
        let joined_timestamp = TimeStamp::PhysicalClock(joined_physical_time);
        group.members.push(ChatMember {
            authority_id: new_member,
            display_name: Self::display_name(&new_member),
            joined_at: joined_timestamp,
            role: ChatRole::Member,
        });

        // Update group
        Self::update_group(effects, &group).await?;

        // Create system message using effect system
        let msg_uuid = effects.random_uuid().await;
        let msg_id = ChatMessageId::from_uuid(msg_uuid);
        let msg_physical_time = effects.physical_time().await.map_err(Self::map_time_err)?;
        let msg_timestamp = TimeStamp::PhysicalClock(msg_physical_time);

        let system_msg = ChatMessage::new_system(
            msg_id,
            group_id.clone(),
            authority_id, // Admin who added the member acts as system
            format!("Member {} joined the group", new_member),
            msg_timestamp,
        );

        Self::store_message(effects, &system_msg).await?;

        Ok(())
    }

    /// Update group metadata (name/description/metadata)
    pub async fn update_group_details<E>(
        &self,
        effects: &E,
        group_id: &ChatGroupId,
        requester: AuthorityId,
        name: Option<String>,
        description: Option<String>,
        metadata: Option<HashMap<String, String>>,
    ) -> Result<ChatGroup>
    where
        E: StorageEffects + Send + Sync,
    {
        let mut group = Self::get_group(effects, group_id)
            .await?
            .ok_or_else(|| AuraError::not_found(format!("Chat group not found: {}", group_id)))?;

        // Only admins can update group metadata
        let requester_member = group
            .members
            .iter()
            .find(|m| m.authority_id == requester)
            .ok_or_else(|| AuraError::permission_denied("Not a member of this group"))?;

        if !matches!(requester_member.role, ChatRole::Admin) {
            return Err(AuraError::permission_denied(
                "Only admins can update group metadata",
            ));
        }

        if let Some(name) = name {
            group.name = name;
        }
        if let Some(desc) = description {
            group.description = desc;
        }
        if let Some(meta) = metadata {
            for (k, v) in meta {
                group.metadata.insert(k, v);
            }
        }

        Self::update_group(effects, &group).await?;
        Ok(group)
    }

    /// Retrieve a single message by ID
    pub async fn get_message<E>(
        effects: &E,
        message_id: &ChatMessageId,
    ) -> Result<Option<ChatMessage>>
    where
        E: StorageEffects + Send + Sync,
    {
        let message_key = format!("chat_message:{}", message_id);
        match effects.retrieve(&message_key).await {
            Ok(Some(data)) => {
                let message: ChatMessage = serde_json::from_slice(&data).map_err(|e| {
                    AuraError::serialization(format!("Failed to deserialize message: {}", e))
                })?;
                Ok(Some(message))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(AuraError::from(e)),
        }
    }

    /// Edit an existing message (sender or admin only)
    pub async fn edit_message<E>(
        &self,
        effects: &E,
        group_id: &ChatGroupId,
        editor: AuthorityId,
        message_id: &ChatMessageId,
        new_content: &str,
    ) -> Result<ChatMessage>
    where
        E: StorageEffects + PhysicalTimeEffects + Send + Sync,
    {
        let group = Self::get_group(effects, group_id)
            .await?
            .ok_or_else(|| AuraError::not_found(format!("Chat group not found: {}", group_id)))?;

        let requester_member = group
            .members
            .iter()
            .find(|m| m.authority_id == editor)
            .ok_or_else(|| AuraError::permission_denied("Not a member of this group"))?;

        let mut message = Self::get_message(effects, message_id)
            .await?
            .ok_or_else(|| AuraError::not_found(format!("Message not found: {}", message_id)))?;

        // Only sender or admin can edit
        let is_sender = message.sender_id == editor;
        let is_admin = matches!(requester_member.role, ChatRole::Admin);
        if !is_sender && !is_admin {
            return Err(AuraError::permission_denied(
                "Only sender or admin may edit messages",
            ));
        }

        let physical_time = effects.physical_time().await.map_err(Self::map_time_err)?;
        let timestamp = TimeStamp::PhysicalClock(physical_time);

        message.content = new_content.to_string();
        message.message_type = crate::types::MessageType::Edit;
        message.timestamp = timestamp;
        message
            .metadata
            .insert("edited_by".to_string(), editor.to_string());

        Self::store_message(effects, &message).await?;
        Ok(message)
    }

    /// Soft-delete a message (sender or admin only)
    pub async fn delete_message<E>(
        &self,
        effects: &E,
        group_id: &ChatGroupId,
        requester: AuthorityId,
        message_id: &ChatMessageId,
    ) -> Result<()>
    where
        E: StorageEffects + PhysicalTimeEffects + Send + Sync,
    {
        let group = Self::get_group(effects, group_id)
            .await?
            .ok_or_else(|| AuraError::not_found(format!("Chat group not found: {}", group_id)))?;

        let requester_member = group
            .members
            .iter()
            .find(|m| m.authority_id == requester)
            .ok_or_else(|| AuraError::permission_denied("Not a member of this group"))?;

        let mut message = Self::get_message(effects, message_id)
            .await?
            .ok_or_else(|| AuraError::not_found(format!("Message not found: {}", message_id)))?;

        let is_sender = message.sender_id == requester;
        let is_admin = matches!(requester_member.role, ChatRole::Admin);
        if !is_sender && !is_admin {
            return Err(AuraError::permission_denied(
                "Only sender or admin may delete messages",
            ));
        }

        let physical_time = effects.physical_time().await.map_err(Self::map_time_err)?;
        let timestamp = TimeStamp::PhysicalClock(physical_time);

        message.message_type = crate::types::MessageType::Delete;
        message.timestamp = timestamp;
        message.content.clear();
        message
            .metadata
            .insert("deleted_by".to_string(), requester.to_string());

        Self::store_message(effects, &message).await?;
        Ok(())
    }

    /// Search messages by substring across a group
    pub async fn search_messages<E>(
        &self,
        effects: &E,
        group_id: &ChatGroupId,
        query: &str,
        limit: usize,
        sender: Option<&AuthorityId>,
    ) -> Result<Vec<ChatMessage>>
    where
        E: StorageEffects + Send + Sync,
    {
        let mut results = Vec::new();
        let history = self
            .get_history(effects, group_id, Some(limit.saturating_mul(2)), None)
            .await?;
        for msg in history {
            if let Some(sender_filter) = sender {
                if &msg.sender_id != sender_filter {
                    continue;
                }
            }

            if msg.content.contains(query) {
                results.push(msg);
            }

            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
    }

    /// Remove a member from a chat group
    pub async fn remove_member<E>(
        &self,
        effects: &E,
        group_id: &ChatGroupId,
        authority_id: AuthorityId,
        member_to_remove: AuthorityId,
    ) -> Result<()>
    where
        E: StorageEffects + RandomEffects + PhysicalTimeEffects + Send + Sync,
    {
        let mut group = Self::get_group(effects, group_id)
            .await?
            .ok_or_else(|| AuraError::not_found(format!("Chat group not found: {}", group_id)))?;

        // Check permissions
        let requester_member = group
            .members
            .iter()
            .find(|m| m.authority_id == authority_id)
            .ok_or_else(|| AuraError::permission_denied("Not a member of this group"))?;

        // Members can remove themselves, only admins can remove others
        if member_to_remove != authority_id && !matches!(requester_member.role, ChatRole::Admin) {
            return Err(AuraError::permission_denied(
                "Only admins can remove other members",
            ));
        }

        // Remove member
        group.members.retain(|m| m.authority_id != member_to_remove);

        // Update group
        Self::update_group(effects, &group).await?;

        // Create system message using effect system
        let msg_uuid = effects.random_uuid().await;
        let msg_id = ChatMessageId::from_uuid(msg_uuid);
        let msg_physical_time = effects.physical_time().await.map_err(Self::map_time_err)?;
        let msg_timestamp = TimeStamp::PhysicalClock(msg_physical_time);

        let action = if member_to_remove == authority_id {
            "left"
        } else {
            "was removed from"
        };
        let system_msg = ChatMessage::new_system(
            msg_id,
            group_id.clone(),
            authority_id, // Authority performing the action
            format!("Member {} {} the group", member_to_remove, action),
            msg_timestamp,
        );

        Self::store_message(effects, &system_msg).await?;

        Ok(())
    }

    /// Store a message in the storage system
    async fn store_message<E>(effects: &E, message: &ChatMessage) -> Result<()>
    where
        E: StorageEffects + Send + Sync,
    {
        let message_key = format!("chat_message:{}", message.id);
        let message_data = serde_json::to_vec(message)
            .map_err(|e| AuraError::serialization(format!("Failed to serialize message: {}", e)))?;

        effects
            .store(&message_key, message_data)
            .await
            .map_err(AuraError::from)?;

        // Index by group_id and timestamp for efficient queries
        let index_key = format!(
            "chat_group_message:{}:{}:{}",
            message.group_id,
            message.timestamp.to_index_ms(),
            message.id
        );
        effects
            .store(&index_key, b"1".to_vec())
            .await
            .map_err(AuraError::from)?;

        Ok(())
    }

    /// Update a group's metadata
    async fn update_group<E>(effects: &E, group: &ChatGroup) -> Result<()>
    where
        E: StorageEffects + Send + Sync,
    {
        let group_key = format!("chat_group:{}", group.id);
        let group_data = serde_json::to_vec(group)
            .map_err(|e| AuraError::serialization(format!("Failed to serialize group: {}", e)))?;

        effects
            .store(&group_key, group_data)
            .await
            .map_err(AuraError::from)?;

        Ok(())
    }

    async fn broadcast_message_to_group<E>(
        effects: &E,
        group: &ChatGroup,
        message: &ChatMessage,
    ) -> Result<()>
    where
        E: StorageEffects + Send + Sync,
    {
        let serialized = serde_json::to_vec(message)
            .map_err(|e| AuraError::serialization(format!("Failed to serialize message: {}", e)))?;

        for member in &group.members {
            let inbox_key = format!("chat_inbox:{}:{}", member.authority_id, message.id);
            // Store a copy per member to simulate AMP fan-out using storage effects
            let _ = effects.store(&inbox_key, serialized.clone()).await;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::effects::storage::{StorageError, StorageStats};
    use aura_core::time::PhysicalTime;
    use futures::lock::Mutex;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use uuid::Uuid;

    #[derive(Debug, Default)]
    struct MockEffects {
        data: Mutex<HashMap<String, Vec<u8>>>,
        uuid_counter: AtomicU64,
        time_ms: AtomicU64,
    }

    #[async_trait]
    impl StorageEffects for MockEffects {
        async fn store(&self, key: &str, value: Vec<u8>) -> std::result::Result<(), StorageError> {
            self.data.lock().await.insert(key.to_string(), value);
            Ok(())
        }

        async fn retrieve(&self, key: &str) -> std::result::Result<Option<Vec<u8>>, StorageError> {
            Ok(self.data.lock().await.get(key).cloned())
        }

        async fn remove(&self, key: &str) -> std::result::Result<bool, StorageError> {
            Ok(self.data.lock().await.remove(key).is_some())
        }

        async fn list_keys(
            &self,
            prefix: Option<&str>,
        ) -> std::result::Result<Vec<String>, StorageError> {
            let guard = self.data.lock().await;
            Ok(guard
                .keys()
                .filter(|k| prefix.map(|p| k.starts_with(p)).unwrap_or(true))
                .cloned()
                .collect())
        }

        async fn exists(&self, key: &str) -> std::result::Result<bool, StorageError> {
            Ok(self.data.lock().await.contains_key(key))
        }

        async fn store_batch(
            &self,
            pairs: HashMap<String, Vec<u8>>,
        ) -> std::result::Result<(), StorageError> {
            let mut guard = self.data.lock().await;
            for (k, v) in pairs {
                guard.insert(k, v);
            }
            Ok(())
        }

        async fn retrieve_batch(
            &self,
            keys: &[String],
        ) -> std::result::Result<HashMap<String, Vec<u8>>, StorageError> {
            let guard = self.data.lock().await;
            Ok(keys
                .iter()
                .filter_map(|k| guard.get(k).map(|v| (k.clone(), v.clone())))
                .collect())
        }

        async fn clear_all(&self) -> std::result::Result<(), StorageError> {
            self.data.lock().await.clear();
            Ok(())
        }

        async fn stats(&self) -> std::result::Result<StorageStats, StorageError> {
            let guard = self.data.lock().await;
            Ok(StorageStats {
                key_count: guard.len() as u64,
                total_size: guard.values().map(|v| v.len() as u64).sum(),
                available_space: None,
                backend_type: "mock".to_string(),
            })
        }
    }

    #[async_trait]
    impl RandomEffects for MockEffects {
        async fn random_bytes(&self, len: usize) -> Vec<u8> {
            vec![0u8; len]
        }

        async fn random_bytes_32(&self) -> [u8; 32] {
            [0u8; 32]
        }

        async fn random_u64(&self) -> u64 {
            self.uuid_counter.fetch_add(1, Ordering::SeqCst)
        }

        async fn random_range(&self, min: u64, _max: u64) -> u64 {
            min
        }

        async fn random_uuid(&self) -> Uuid {
            let counter = self.uuid_counter.fetch_add(1, Ordering::SeqCst);
            Uuid::from_u128(counter as u128)
        }
    }

    #[async_trait]
    impl PhysicalTimeEffects for MockEffects {
        async fn physical_time(
            &self,
        ) -> std::result::Result<PhysicalTime, aura_core::effects::time::TimeError> {
            let now = self.time_ms.fetch_add(1, Ordering::SeqCst);
            Ok(PhysicalTime {
                ts_ms: now,
                uncertainty: None,
            })
        }

        async fn sleep_ms(
            &self,
            _ms: u64,
        ) -> std::result::Result<(), aura_core::effects::time::TimeError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn create_group_persists_and_lists_membership() {
        let effects = Arc::new(MockEffects::default());
        let handler = ChatHandler::new();
        let creator = AuthorityId::from_uuid(Uuid::from_u128(1));
        let member = AuthorityId::from_uuid(Uuid::from_u128(2));

        let group = handler
            .create_group(&*effects, "test", creator, vec![member])
            .await
            .unwrap();

        let fetched = ChatHandler::get_group(&*effects, &group.id).await.unwrap();
        assert!(fetched.is_some());

        let creator_groups = handler.list_user_groups(&*effects, &creator).await.unwrap();
        assert_eq!(creator_groups.len(), 1);
        let member_groups = handler.list_user_groups(&*effects, &member).await.unwrap();
        assert_eq!(member_groups.len(), 1);
    }

    #[tokio::test]
    async fn send_message_stores_history_and_inboxes() {
        let effects = Arc::new(MockEffects::default());
        let handler = ChatHandler::new();
        let creator = AuthorityId::from_uuid(Uuid::from_u128(10));
        let other = AuthorityId::from_uuid(Uuid::from_u128(11));
        let group = handler
            .create_group(&*effects, "chat", creator, vec![other])
            .await
            .unwrap();

        let sent = handler
            .send_message(&*effects, &group.id, creator, "hello world".into())
            .await
            .unwrap();

        let history = handler
            .get_history(&*effects, &group.id, None, None)
            .await
            .unwrap();
        // Should have 2 messages: system message for group creation + our sent message
        assert_eq!(history.len(), 2);
        assert_eq!(
            history[0].content,
            format!("Chat group '{}' created", "chat")
        );
        assert_eq!(history[1].content, sent.content);

        let inbox_keys = effects.list_keys(Some("chat_inbox:")).await.unwrap();
        assert_eq!(inbox_keys.len(), group.members.len());
    }
}
