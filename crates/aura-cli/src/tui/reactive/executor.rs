//! # Query Executor
//!
//! Executes TuiQuery instances and manages subscriptions to query results.
//! Currently uses an in-memory mock data backend for demonstration.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use super::queries::{
    Channel, ChannelsQuery, Guardian, GuardianStatus, GuardiansQuery, Invitation,
    InvitationDirection, InvitationStatus, InvitationType, InvitationsQuery, Message,
    MessagesQuery, RecoveryQuery, RecoveryState, RecoveryStatus,
};

/// Query executor that manages query execution and subscriptions
pub struct QueryExecutor {
    /// Mock data store
    store: Arc<RwLock<MockDataStore>>,
    /// Update broadcaster
    update_tx: broadcast::Sender<DataUpdate>,
}

/// Updates emitted when data changes
#[derive(Debug, Clone)]
pub enum DataUpdate {
    /// Channels were updated
    ChannelsUpdated,
    /// Messages in a channel were updated
    MessagesUpdated {
        /// The ID of the channel whose messages were updated
        channel_id: String,
    },
    /// Guardians were updated
    GuardiansUpdated,
    /// Recovery state was updated
    RecoveryUpdated,
    /// Invitations were updated
    InvitationsUpdated,
}

/// Mock data store for demonstration
struct MockDataStore {
    channels: Vec<Channel>,
    messages: HashMap<String, Vec<Message>>,
    guardians: Vec<Guardian>,
    recovery_status: RecoveryStatus,
    invitations: Vec<Invitation>,
}

impl Default for MockDataStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MockDataStore {
    fn new() -> Self {
        // Create mock channels
        let channels = vec![
            Channel {
                id: "general".to_string(),
                name: "general".to_string(),
                topic: Some("General discussion".to_string()),
                unread_count: 0,
                is_dm: false,
                member_count: 3,
                last_activity: now_millis(),
            },
            Channel {
                id: "random".to_string(),
                name: "random".to_string(),
                topic: Some("Random chat".to_string()),
                unread_count: 2,
                is_dm: false,
                member_count: 3,
                last_activity: now_millis() - 3600000,
            },
            Channel {
                id: "dm_alice".to_string(),
                name: "Alice".to_string(),
                topic: None,
                unread_count: 0,
                is_dm: true,
                member_count: 2,
                last_activity: now_millis() - 7200000,
            },
        ];

        // Create mock messages
        let mut messages = HashMap::new();
        messages.insert(
            "general".to_string(),
            vec![
                Message {
                    id: "msg1".to_string(),
                    channel_id: "general".to_string(),
                    sender_id: "alice_auth".to_string(),
                    sender_name: "Alice".to_string(),
                    content: "Hey everyone!".to_string(),
                    timestamp: now_millis() - 3600000,
                    read: true,
                    is_own: false,
                    reply_to: None,
                },
                Message {
                    id: "msg2".to_string(),
                    channel_id: "general".to_string(),
                    sender_id: "bob_auth".to_string(),
                    sender_name: "Bob".to_string(),
                    content: "Hi Alice! How's the threshold identity working?".to_string(),
                    timestamp: now_millis() - 3000000,
                    read: true,
                    is_own: true,
                    reply_to: Some("msg1".to_string()),
                },
                Message {
                    id: "msg3".to_string(),
                    channel_id: "general".to_string(),
                    sender_id: "charlie_auth".to_string(),
                    sender_name: "Charlie".to_string(),
                    content: "The 2-of-3 recovery is really solid!".to_string(),
                    timestamp: now_millis() - 1800000,
                    read: true,
                    is_own: false,
                    reply_to: None,
                },
            ],
        );

        messages.insert(
            "random".to_string(),
            vec![
                Message {
                    id: "msg4".to_string(),
                    channel_id: "random".to_string(),
                    sender_id: "alice_auth".to_string(),
                    sender_name: "Alice".to_string(),
                    content: "Anyone up for testing the new features?".to_string(),
                    timestamp: now_millis() - 7200000,
                    read: false,
                    is_own: false,
                    reply_to: None,
                },
                Message {
                    id: "msg5".to_string(),
                    channel_id: "random".to_string(),
                    sender_id: "charlie_auth".to_string(),
                    sender_name: "Charlie".to_string(),
                    content: "I'm in! Let me know when.".to_string(),
                    timestamp: now_millis() - 3600000,
                    read: false,
                    is_own: false,
                    reply_to: Some("msg4".to_string()),
                },
            ],
        );

        messages.insert("dm_alice".to_string(), vec![]);

        // Create mock guardians
        let guardians = vec![
            Guardian {
                authority_id: "alice_auth".to_string(),
                name: "Alice".to_string(),
                status: GuardianStatus::Active,
                added_at: now_millis() - 86400000 * 30,
                last_seen: Some(now_millis() - 3600000),
                share_index: Some(1),
            },
            Guardian {
                authority_id: "charlie_auth".to_string(),
                name: "Charlie".to_string(),
                status: GuardianStatus::Active,
                added_at: now_millis() - 86400000 * 30,
                last_seen: Some(now_millis() - 1800000),
                share_index: Some(2),
            },
        ];

        // Create mock recovery status
        let recovery_status = RecoveryStatus {
            session_id: None,
            state: RecoveryState::None,
            approvals_received: 0,
            threshold: 2,
            total_guardians: 2,
            approvals: vec![],
            started_at: None,
            expires_at: None,
            error: None,
        };

        // Create mock invitations
        let invitations = vec![
            Invitation {
                id: "inv1".to_string(),
                direction: InvitationDirection::Outbound,
                other_party_id: "dave_auth".to_string(),
                other_party_name: "Dave".to_string(),
                invitation_type: InvitationType::Guardian,
                status: InvitationStatus::Pending,
                created_at: now_millis() - 7200000,
                expires_at: Some(now_millis() + 86400000),
                message: Some("Join our block!".to_string()),
            },
            Invitation {
                id: "inv2".to_string(),
                direction: InvitationDirection::Inbound,
                other_party_id: "eve_auth".to_string(),
                other_party_name: "Eve".to_string(),
                invitation_type: InvitationType::Contact,
                status: InvitationStatus::Pending,
                created_at: now_millis() - 3600000,
                expires_at: Some(now_millis() + 172800000),
                message: Some("Want to join my neighborhood?".to_string()),
            },
        ];

        Self {
            channels,
            messages,
            guardians,
            recovery_status,
            invitations,
        }
    }

    /// Add a new message to a channel
    fn add_message(&mut self, channel_id: &str, message: Message) {
        let is_own = message.is_own;

        self.messages
            .entry(channel_id.to_string())
            .or_insert_with(Vec::new)
            .push(message);

        // Update channel's last activity
        if let Some(channel) = self.channels.iter_mut().find(|c| c.id == channel_id) {
            channel.last_activity = now_millis();
            if !is_own {
                channel.unread_count += 1;
            }
        }
    }
}

impl QueryExecutor {
    /// Create a new query executor
    pub fn new() -> Self {
        let (update_tx, _) = broadcast::channel(256);
        Self {
            store: Arc::new(RwLock::new(MockDataStore::new())),
            update_tx,
        }
    }

    /// Execute a channels query
    pub async fn execute_channels_query(
        &self,
        query: &ChannelsQuery,
    ) -> Result<Vec<Channel>, String> {
        let store = self.store.read().await;
        let mut channels = store.channels.clone();

        // Apply filters
        if let Some(channel_type) = &query.channel_type {
            use super::queries::ChannelType;
            channels.retain(|c| match channel_type {
                ChannelType::Group => !c.is_dm,
                ChannelType::DirectMessage => c.is_dm,
                ChannelType::All => true,
            });
        }

        if query.unread_only {
            channels.retain(|c| c.unread_count > 0);
        }

        // Sort by last activity
        channels.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));

        // Apply limit
        if let Some(limit) = query.limit {
            channels.truncate(limit);
        }

        Ok(channels)
    }

    /// Execute a messages query
    pub async fn execute_messages_query(
        &self,
        query: &MessagesQuery,
    ) -> Result<Vec<Message>, String> {
        let store = self.store.read().await;
        let messages = store
            .messages
            .get(&query.channel_id)
            .cloned()
            .unwrap_or_default();

        let mut filtered: Vec<Message> = messages
            .into_iter()
            .filter(|m| {
                if let Some(since) = query.since {
                    if m.timestamp < since {
                        return false;
                    }
                }
                if let Some(until) = query.until {
                    if m.timestamp >= until {
                        return false;
                    }
                }
                true
            })
            .collect();

        // Sort by timestamp (oldest first)
        filtered.sort_by_key(|m| m.timestamp);

        // Apply pagination
        if let Some(offset) = query.offset {
            filtered = filtered.into_iter().skip(offset).collect();
        }

        if let Some(limit) = query.limit {
            filtered.truncate(limit);
        }

        Ok(filtered)
    }

    /// Execute a guardians query
    pub async fn execute_guardians_query(
        &self,
        _query: &GuardiansQuery,
    ) -> Result<Vec<Guardian>, String> {
        let store = self.store.read().await;
        Ok(store.guardians.clone())
    }

    /// Execute a recovery query
    pub async fn execute_recovery_query(
        &self,
        _query: &RecoveryQuery,
    ) -> Result<RecoveryStatus, String> {
        let store = self.store.read().await;
        Ok(store.recovery_status.clone())
    }

    /// Execute an invitations query
    pub async fn execute_invitations_query(
        &self,
        _query: &InvitationsQuery,
    ) -> Result<Vec<Invitation>, String> {
        let store = self.store.read().await;
        Ok(store.invitations.clone())
    }

    /// Subscribe to data updates
    pub fn subscribe(&self) -> broadcast::Receiver<DataUpdate> {
        self.update_tx.subscribe()
    }

    /// Add a message (for testing/demo)
    pub async fn add_message(&self, channel_id: &str, message: Message) {
        let mut store = self.store.write().await;
        store.add_message(channel_id, message);
        drop(store);

        let _ = self.update_tx.send(DataUpdate::MessagesUpdated {
            channel_id: channel_id.to_string(),
        });
        let _ = self.update_tx.send(DataUpdate::ChannelsUpdated);
    }

    /// Mark channel as read (for testing/demo)
    pub async fn mark_channel_read(&self, channel_id: &str) {
        let mut store = self.store.write().await;
        if let Some(channel) = store.channels.iter_mut().find(|c| c.id == channel_id) {
            channel.unread_count = 0;
        }
        drop(store);

        let _ = self.update_tx.send(DataUpdate::ChannelsUpdated);
    }
}

impl Default for QueryExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Get current time in milliseconds
fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_channels_query() {
        let executor = QueryExecutor::new();
        let query = ChannelsQuery::new();

        let channels = executor.execute_channels_query(&query).await.unwrap();
        assert_eq!(channels.len(), 3);
    }

    #[tokio::test]
    async fn test_channels_query_groups_only() {
        let executor = QueryExecutor::new();
        let query = ChannelsQuery::new().groups_only();

        let channels = executor.execute_channels_query(&query).await.unwrap();
        assert_eq!(channels.len(), 2);
        assert!(channels.iter().all(|c| !c.is_dm));
    }

    #[tokio::test]
    async fn test_channels_query_dms_only() {
        let executor = QueryExecutor::new();
        let query = ChannelsQuery::new().dms_only();

        let channels = executor.execute_channels_query(&query).await.unwrap();
        assert_eq!(channels.len(), 1);
        assert!(channels.iter().all(|c| c.is_dm));
    }

    #[tokio::test]
    async fn test_channels_query_unread_only() {
        let executor = QueryExecutor::new();
        let query = ChannelsQuery::new().unread_only();

        let channels = executor.execute_channels_query(&query).await.unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].id, "random");
    }

    #[tokio::test]
    async fn test_execute_messages_query() {
        let executor = QueryExecutor::new();
        let query = MessagesQuery::new("general".to_string());

        let messages = executor.execute_messages_query(&query).await.unwrap();
        assert_eq!(messages.len(), 3);
        // Should be sorted by timestamp
        assert_eq!(messages[0].id, "msg1");
        assert_eq!(messages[2].id, "msg3");
    }

    #[tokio::test]
    async fn test_add_message() {
        let executor = QueryExecutor::new();
        let mut updates = executor.subscribe();

        let message = Message {
            id: "new_msg".to_string(),
            channel_id: "general".to_string(),
            sender_id: "test_auth".to_string(),
            sender_name: "Test".to_string(),
            content: "Test message".to_string(),
            timestamp: now_millis(),
            read: false,
            is_own: true,
            reply_to: None,
        };

        executor.add_message("general", message).await;

        // Should receive two updates
        let update1 = updates.recv().await.unwrap();
        let update2 = updates.recv().await.unwrap();

        assert!(matches!(update1, DataUpdate::MessagesUpdated { .. }));
        assert!(matches!(update2, DataUpdate::ChannelsUpdated));

        // Verify message was added
        let query = MessagesQuery::new("general".to_string());
        let messages = executor.execute_messages_query(&query).await.unwrap();
        assert_eq!(messages.len(), 4);
    }
}
