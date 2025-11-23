//! Chat History management and message retrieval
//!
//! This module provides functionality for storing, retrieving, and managing
//! chat message history with efficient pagination and filtering.

use crate::{ChatGroupId, ChatMessage, ChatMessageId};
use aura_core::{effects::StorageEffects, AuraError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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
    pub async fn store_message(&self, message: &ChatMessage) -> Result<()> {
        // Store the individual message
        let message_key = format!("chat_message:{}", message.id);
        let message_data = serde_json::to_vec(message)
            .map_err(|e| AuraError::serialization(format!("Failed to serialize message: {}", e)))?;

        self.storage
            .store(&message_key, message_data)
            .await
            .map_err(|e| AuraError::from(e))?;

        // Update group message index for efficient queries
        let index_key = format!("chat_group_messages:{}", message.group_id);
        self.add_to_message_index(&index_key, &message.id, message.timestamp)
            .await?;

        Ok(())
    }

    /// Retrieve a specific message by ID
    pub async fn get_message(&self, message_id: &ChatMessageId) -> Result<Option<ChatMessage>> {
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
        before: Option<DateTime<Utc>>,
    ) -> Result<Vec<ChatMessage>> {
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

        // Apply before filter
        if let Some(before_ts) = before {
            let cutoff = before_ts.timestamp_millis();
            entries.retain(|(ts, _)| *ts < cutoff);
        }

        // Sort newest first then apply limit
        entries.sort_by(|a, b| b.0.cmp(&a.0));
        entries.truncate(limit);

        // Fetch messages in chronological order
        let mut messages = Vec::new();
        for (_, key) in entries.into_iter().rev() {
            if let Ok(Some(raw)) = self.storage.retrieve(&key).await {
                if let Ok(msg) = serde_json::from_slice::<ChatMessage>(&raw) {
                    messages.push(msg);
                }
            }
        }

        Ok(messages)
    }

    /// Search messages in a group by content
    pub async fn search_messages(
        &self,
        group_id: &ChatGroupId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ChatMessage>> {
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
    pub async fn get_message_count(&self, group_id: &ChatGroupId) -> Result<usize> {
        let key_prefix = format!("chat_group_message:{}:", group_id);
        let keys = self.storage.list_keys(Some(&key_prefix)).await?;
        Ok(keys.len())
    }

    /// Delete a message (soft delete with tombstone)
    pub async fn delete_message(&self, message_id: &ChatMessageId) -> Result<()> {
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
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        let index_entry_key = format!(
            "{}{}:{}",
            index_key,
            timestamp.timestamp_millis(),
            message_id
        );
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
    pub timestamp: DateTime<Utc>,
    /// Message type for filtering
    pub message_type: crate::types::MessageType,
    /// Sender for filtering/authorization
    pub sender_id: aura_core::identifiers::AuthorityId,
}

/// Pagination cursor for message history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryCursor {
    /// Timestamp cursor
    pub timestamp: DateTime<Utc>,
    /// Message ID for tie-breaking
    pub message_id: ChatMessageId,
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

#[cfg(test)]
mod tests {
    use super::*;

    // TODO: Add tests with mock storage
    #[tokio::test]
    async fn test_chat_history_creation() {
        // This test requires mock storage implementation
        // Will be implemented when mock effects are available
    }
}
