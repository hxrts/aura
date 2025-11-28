//! Chat History management and message retrieval
//!
//! This module provides functionality for storing, retrieving, and managing
//! chat message history with efficient pagination and filtering.

use crate::{ChatGroupId, ChatMessage, ChatMessageId};
use aura_core::{
    effects::StorageEffects,
    time::{OrderingPolicy, TimeStamp},
    AuraError,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// Chat history manager for efficient message storage and retrieval
pub struct ChatHistory<S>
where
    S: StorageEffects + Send + Sync,
{
    /// Storage effect handler
    storage: Arc<S>,
}

impl<S> ChatHistory<S>
where
    S: StorageEffects + Send + Sync,
{
    /// Create a new chat history manager
    pub fn new(storage: Arc<S>) -> Self {
        Self { storage }
    }

    /// Store a message in the history
    pub async fn store_message(&self, message: &ChatMessage) -> std::result::Result<(), AuraError> {
        // Store the individual message
        let message_key = format!("chat_message:{}", message.id);
        let message_data = serde_json::to_vec(message)
            .map_err(|e| AuraError::serialization(format!("Failed to serialize message: {}", e)))?;

        self.storage
            .store(&message_key, message_data)
            .await
            .map_err(AuraError::from)?;

        // Update group message index for efficient queries
        let index_key = format!("chat_group_message:{}", message.group_id);
        self.add_to_message_index(&index_key, &message.id, message.timestamp.clone())
            .await?;

        Ok(())
    }

    /// Retrieve a specific message by ID
    pub async fn get_message(
        &self,
        message_id: &ChatMessageId,
    ) -> std::result::Result<Option<ChatMessage>, AuraError> {
        let message_key = format!("chat_message:{}", message_id);

        match self.storage.retrieve(&message_key).await {
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

    /// Get message history for a group with pagination
    pub async fn get_group_history(
        &self,
        group_id: &ChatGroupId,
        limit: usize,
        before: Option<TimeStamp>,
    ) -> std::result::Result<Vec<ChatMessage>, AuraError> {
        let key_prefix = format!("chat_group_message:{}:", group_id);
        let mut entries: Vec<(i64, String)> = Vec::new();

        let keys = self
            .storage
            .list_keys(Some(&key_prefix))
            .await
            .map_err(AuraError::from)?;

        for key in keys {
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

        // Sort newest first then apply limit (using raw i64 for index keys)
        entries.sort_by(|a, b| b.0.cmp(&a.0));
        entries.truncate(limit);

        // Fetch messages in chronological order
        let mut messages = Vec::new();
        for (_, key) in entries.into_iter().rev() {
            // Extract message ID from the index key
            // Key format: chat_group_message:{group_id}:{timestamp}:{message_id}
            if let Some(message_id_str) = key.split(':').next_back() {
                if let Ok(message_id_uuid) = Uuid::parse_str(message_id_str) {
                    let message_id = ChatMessageId(message_id_uuid);
                    if let Ok(Some(msg)) = self.get_message(&message_id).await {
                        messages.push(msg);
                    }
                }
            }
        }

        // Sort messages using proper unified time comparison (for cases where index ordering is imperfect)
        messages.sort_by(|a, b| {
            a.timestamp
                .sort_compare(&b.timestamp, OrderingPolicy::DeterministicTieBreak)
        });

        Ok(messages)
    }

    /// Search messages in a group by content
    pub async fn search_messages(
        &self,
        group_id: &ChatGroupId,
        query: &str,
        limit: usize,
    ) -> std::result::Result<Vec<ChatMessage>, AuraError> {
        let history = self
            .get_group_history(group_id, limit.saturating_mul(2), None)
            .await?;

        let mut results = Vec::new();
        for msg in history {
            if msg.content.contains(query) {
                results.push(msg);
            }
            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
    }

    /// Get message count for a group
    pub async fn get_message_count(
        &self,
        group_id: &ChatGroupId,
    ) -> std::result::Result<usize, AuraError> {
        let key_prefix = format!("chat_group_message:{}:", group_id);
        let keys = self.storage.list_keys(Some(&key_prefix)).await?;
        Ok(keys.len())
    }

    /// Delete a message (soft delete with tombstone)
    pub async fn delete_message(
        &self,
        message_id: &ChatMessageId,
    ) -> std::result::Result<(), AuraError> {
        // Mark a tombstone entry; leave index to preserve ordering
        let tombstone_key = format!("chat_message_tombstone:{}", message_id);
        self.storage
            .store(&tombstone_key, b"tombstone".to_vec())
            .await
            .map_err(AuraError::from)?;
        Ok(())
    }

    /// Add a message to the group's message index
    async fn add_to_message_index(
        &self,
        index_key: &str,
        message_id: &ChatMessageId,
        timestamp: TimeStamp,
    ) -> std::result::Result<(), AuraError> {
        let index_entry_key = format!("{}:{}:{}", index_key, timestamp.to_index_ms(), message_id);
        self.storage
            .store(&index_entry_key, b"1".to_vec())
            .await
            .map_err(AuraError::from)
    }
}

/// Message index entry for efficient querying
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageIndexEntry {
    /// Message ID
    pub message_id: ChatMessageId,
    /// Message timestamp for sorting
    pub timestamp: TimeStamp,
    /// Message type for filtering
    pub message_type: crate::types::MessageType,
    /// Sender for filtering/authorization
    pub sender_id: aura_core::identifiers::AuthorityId,
}

/// Pagination cursor for message history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryCursor {
    /// Timestamp cursor
    pub timestamp: TimeStamp,
    /// Message ID for tie-breaking
    pub message_id: ChatMessageId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::effects::storage::{StorageError, StorageStats};
    use aura_core::time::PhysicalTime;
    use futures::lock::Mutex;
    use std::collections::HashMap;
    use uuid::Uuid;

    #[derive(Debug, Default)]
    struct MemoryStorage {
        inner: Mutex<HashMap<String, Vec<u8>>>,
    }

    #[async_trait]
    impl StorageEffects for MemoryStorage {
        async fn store(&self, key: &str, value: Vec<u8>) -> std::result::Result<(), StorageError> {
            self.inner.lock().await.insert(key.to_string(), value);
            Ok(())
        }

        async fn retrieve(&self, key: &str) -> std::result::Result<Option<Vec<u8>>, StorageError> {
            Ok(self.inner.lock().await.get(key).cloned())
        }

        async fn remove(&self, key: &str) -> std::result::Result<bool, StorageError> {
            Ok(self.inner.lock().await.remove(key).is_some())
        }

        async fn list_keys(
            &self,
            prefix: Option<&str>,
        ) -> std::result::Result<Vec<String>, StorageError> {
            let guard = self.inner.lock().await;
            let keys = guard
                .keys()
                .filter(|k| prefix.map(|p| k.starts_with(p)).unwrap_or(true))
                .cloned()
                .collect();
            Ok(keys)
        }

        async fn exists(&self, key: &str) -> std::result::Result<bool, StorageError> {
            Ok(self.inner.lock().await.contains_key(key))
        }

        async fn store_batch(
            &self,
            pairs: HashMap<String, Vec<u8>>,
        ) -> std::result::Result<(), StorageError> {
            let mut guard = self.inner.lock().await;
            for (k, v) in pairs {
                guard.insert(k, v);
            }
            Ok(())
        }

        async fn retrieve_batch(
            &self,
            keys: &[String],
        ) -> std::result::Result<HashMap<String, Vec<u8>>, StorageError> {
            let guard = self.inner.lock().await;
            Ok(keys
                .iter()
                .filter_map(|k| guard.get(k).map(|v| (k.clone(), v.clone())))
                .collect())
        }

        async fn clear_all(&self) -> std::result::Result<(), StorageError> {
            self.inner.lock().await.clear();
            Ok(())
        }

        async fn stats(&self) -> std::result::Result<StorageStats, StorageError> {
            let guard = self.inner.lock().await;
            Ok(StorageStats {
                key_count: guard.len() as u64,
                total_size: guard.values().map(|v| v.len() as u64).sum(),
                available_space: None,
                backend_type: "memory".to_string(),
            })
        }
    }

    fn sample_group_id() -> ChatGroupId {
        ChatGroupId::from_uuid(Uuid::nil())
    }

    fn sample_message(ts_ms: u64) -> ChatMessage {
        use aura_core::time::PhysicalTime;

        let timestamp = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms,
            uncertainty: None,
        });
        // Use timestamp to generate a unique but deterministic UUID for testing
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&ts_ms.to_le_bytes());
        let message_id = Uuid::from_bytes(bytes);

        ChatMessage::new_text(
            ChatMessageId(message_id),
            sample_group_id(),
            aura_core::identifiers::AuthorityId::from_uuid(Uuid::nil()),
            format!("hello-{ts_ms}"),
            timestamp,
        )
    }

    #[tokio::test]
    async fn stores_and_retrieves_messages() {
        let storage = Arc::new(MemoryStorage::default());
        let history = ChatHistory::new(storage.clone());
        let msg = sample_message(1);

        history.store_message(&msg).await.unwrap();

        let fetched = history.get_message(&msg.id).await.unwrap();
        assert_eq!(fetched, Some(msg.clone()));

        let count = history
            .get_message_count(&msg.group_id)
            .await
            .expect("count");
        assert_eq!(count, 1);

        let group_history = history
            .get_group_history(&msg.group_id, 10, None)
            .await
            .unwrap();
        assert_eq!(group_history.len(), 1);
        assert_eq!(group_history[0].content, msg.content);
    }

    #[tokio::test]
    async fn filters_history_by_before_timestamp() {
        let storage = Arc::new(MemoryStorage::default());
        let history = ChatHistory::new(storage.clone());
        let early = sample_message(1);
        let late = sample_message(10);
        history.store_message(&early).await.unwrap();
        history.store_message(&late).await.unwrap();

        let cutoff = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 5,
            uncertainty: None,
        });
        let results = history
            .get_group_history(&early.group_id, 10, Some(cutoff))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, early.content);
    }

    #[tokio::test]
    async fn test_chat_history_creation() {
        let storage = Arc::new(MemoryStorage::default());
        let history = ChatHistory::new(storage.clone());

        // Should start empty
        assert_eq!(
            history
                .get_group_history(&sample_group_id(), 10, None)
                .await
                .unwrap()
                .len(),
            0
        );

        // Store and fetch a message
        let msg = sample_message(42);
        history.store_message(&msg).await.unwrap();
        let fetched = history.get_message(&msg.id).await.unwrap();
        assert_eq!(fetched, Some(msg));
    }
}

/// History query parameters
#[derive(Debug, Clone)]
pub struct HistoryQuery {
    /// Maximum number of messages to return
    pub limit: usize,
    /// Only return messages before this cursor
    pub before: Option<HistoryCursor>,
    /// Only return messages after this cursor
    pub after: Option<HistoryCursor>,
    /// Filter by message type
    pub message_type: Option<crate::types::MessageType>,
    /// Filter by sender
    pub sender_id: Option<aura_core::identifiers::AuthorityId>,
}

impl Default for HistoryQuery {
    fn default() -> Self {
        Self {
            limit: 50,
            before: None,
            after: None,
            message_type: None,
            sender_id: None,
        }
    }
}
