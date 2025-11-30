//! # Mock Store
//!
//! Pre-populated demo data for the simulated backend.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::tui::reactive::{
    Channel, Guardian, GuardianApproval, GuardianStatus, Invitation, InvitationDirection,
    InvitationStatus, InvitationType, Message, RecoveryState, RecoveryStatus,
};

/// Mock data store for demo mode
pub struct MockStore {
    /// Current user authority ID
    pub authority_id: String,
    /// Current user name
    pub user_name: String,
    /// Channels
    pub channels: Arc<RwLock<Vec<Channel>>>,
    /// Messages by channel
    pub messages: Arc<RwLock<HashMap<String, Vec<Message>>>>,
    /// Guardians
    pub guardians: Arc<RwLock<Vec<Guardian>>>,
    /// Recovery status
    pub recovery: Arc<RwLock<RecoveryStatus>>,
    /// Invitations
    pub invitations: Arc<RwLock<Vec<Invitation>>>,
}

impl MockStore {
    /// Create a new mock store with demo data
    pub fn new() -> Self {
        let authority_id = "bob_authority_12345".to_string();
        let user_name = "Bob".to_string();

        Self {
            authority_id,
            user_name,
            channels: Arc::new(RwLock::new(Vec::new())),
            messages: Arc::new(RwLock::new(HashMap::new())),
            guardians: Arc::new(RwLock::new(Vec::new())),
            recovery: Arc::new(RwLock::new(RecoveryStatus::default())),
            invitations: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Load initial demo data
    pub async fn load_demo_data(&self) {
        self.load_channels().await;
        self.load_guardians().await;
        self.load_messages().await;
        self.load_invitations().await;
    }

    async fn load_channels(&self) {
        let channels = vec![
            Channel {
                id: "general".to_string(),
                name: "General".to_string(),
                topic: Some("Welcome to Aura!".to_string()),
                unread_count: 0,
                is_dm: false,
                member_count: 3,
                last_activity: now_millis(),
            },
            Channel {
                id: "guardians".to_string(),
                name: "Guardian Circle".to_string(),
                topic: Some("Guardian coordination channel".to_string()),
                unread_count: 0,
                is_dm: false,
                member_count: 3,
                last_activity: now_millis() - 3600000,
            },
        ];

        *self.channels.write().await = channels;
    }

    async fn load_guardians(&self) {
        let guardians = vec![
            Guardian {
                authority_id: "alice_authority_67890".to_string(),
                name: "Alice".to_string(),
                status: GuardianStatus::Active,
                added_at: now_millis() - 86400000 * 30,
                last_seen: Some(now_millis() - 3600000),
                share_index: Some(1),
            },
            Guardian {
                authority_id: "charlie_authority_11111".to_string(),
                name: "Charlie".to_string(),
                status: GuardianStatus::Active,
                added_at: now_millis() - 86400000 * 30,
                last_seen: Some(now_millis() - 1800000),
                share_index: Some(2),
            },
        ];

        *self.guardians.write().await = guardians;
    }

    async fn load_messages(&self) {
        let mut messages = HashMap::new();

        messages.insert(
            "general".to_string(),
            vec![
                Message {
                    id: "msg_1".to_string(),
                    channel_id: "general".to_string(),
                    sender_id: self.authority_id.clone(),
                    sender_name: "Bob".to_string(),
                    content: "Hey everyone! Welcome to our group chat.".to_string(),
                    timestamp: now_millis() - 3600000,
                    read: true,
                    is_own: true,
                    reply_to: None,
                },
                Message {
                    id: "msg_2".to_string(),
                    channel_id: "general".to_string(),
                    sender_id: "alice_authority_67890".to_string(),
                    sender_name: "Alice".to_string(),
                    content: "This is so cool! Secure messaging with social recovery.".to_string(),
                    timestamp: now_millis() - 3000000,
                    read: true,
                    is_own: false,
                    reply_to: None,
                },
                Message {
                    id: "msg_3".to_string(),
                    channel_id: "general".to_string(),
                    sender_id: "charlie_authority_11111".to_string(),
                    sender_name: "Charlie".to_string(),
                    content: "I love how we can recover our data if something goes wrong!"
                        .to_string(),
                    timestamp: now_millis() - 1800000,
                    read: true,
                    is_own: false,
                    reply_to: None,
                },
            ],
        );

        *self.messages.write().await = messages;
    }

    async fn load_invitations(&self) {
        let invitations = vec![Invitation {
            id: "inv_1".to_string(),
            direction: InvitationDirection::Inbound,
            other_party_id: "diana_authority_22222".to_string(),
            other_party_name: "Diana".to_string(),
            invitation_type: InvitationType::Guardian,
            status: InvitationStatus::Pending,
            created_at: now_millis() - 86400000,
            expires_at: Some(now_millis() + 86400000 * 7),
            message: Some("Would you be my guardian?".to_string()),
        }];

        *self.invitations.write().await = invitations;
    }

    /// Add a message to a channel
    pub async fn add_message(&self, channel_id: &str, message: Message) {
        let mut messages = self.messages.write().await;
        messages
            .entry(channel_id.to_string())
            .or_default()
            .push(message);
    }

    /// Start recovery process
    pub async fn start_recovery(&self) {
        let mut recovery = self.recovery.write().await;
        *recovery = RecoveryStatus {
            session_id: Some(format!("recovery_{}", now_millis())),
            state: RecoveryState::Initiated,
            approvals_received: 0,
            threshold: 2,
            total_guardians: 2,
            approvals: vec![],
            started_at: Some(now_millis()),
            expires_at: Some(now_millis() + 3600000),
            error: None,
        };
    }

    /// Add a guardian approval
    pub async fn add_approval(&self, guardian_id: &str, guardian_name: &str) {
        let mut recovery = self.recovery.write().await;
        if recovery.state == RecoveryState::Initiated {
            recovery.approvals.push(GuardianApproval {
                guardian_id: guardian_id.to_string(),
                guardian_name: guardian_name.to_string(),
                approved: true,
                timestamp: Some(now_millis()),
            });
            recovery.approvals_received = recovery.approvals.len() as u32;

            // Check threshold
            if recovery.approvals_received >= recovery.threshold {
                recovery.state = RecoveryState::Completed;
            }
        }
    }

    /// Complete recovery
    pub async fn complete_recovery(&self) {
        let mut recovery = self.recovery.write().await;
        recovery.state = RecoveryState::Completed;
    }

    /// Cancel recovery
    pub async fn cancel_recovery(&self) {
        let mut recovery = self.recovery.write().await;
        *recovery = RecoveryStatus::default();
    }

    /// Get current channels
    pub async fn get_channels(&self) -> Vec<Channel> {
        self.channels.read().await.clone()
    }

    /// Get messages for a channel
    pub async fn get_messages(&self, channel_id: &str) -> Vec<Message> {
        self.messages
            .read()
            .await
            .get(channel_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Get guardians
    pub async fn get_guardians(&self) -> Vec<Guardian> {
        self.guardians.read().await.clone()
    }

    /// Get recovery status
    pub async fn get_recovery(&self) -> RecoveryStatus {
        self.recovery.read().await.clone()
    }

    /// Get invitations
    pub async fn get_invitations(&self) -> Vec<Invitation> {
        self.invitations.read().await.clone()
    }
}

impl Default for MockStore {
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
    async fn test_mock_store_creation() {
        let store = MockStore::new();
        assert_eq!(store.user_name, "Bob");
    }

    #[tokio::test]
    async fn test_load_demo_data() {
        let store = MockStore::new();
        store.load_demo_data().await;

        let channels = store.get_channels().await;
        assert_eq!(channels.len(), 2);

        let guardians = store.get_guardians().await;
        assert_eq!(guardians.len(), 2);

        let messages = store.get_messages("general").await;
        assert_eq!(messages.len(), 3);
    }

    #[tokio::test]
    async fn test_recovery_flow() {
        let store = MockStore::new();
        store.load_demo_data().await;

        // Start recovery
        store.start_recovery().await;
        let recovery = store.get_recovery().await;
        assert_eq!(recovery.state, RecoveryState::Initiated);

        // Add approvals
        store.add_approval("alice_authority_67890", "Alice").await;
        let recovery = store.get_recovery().await;
        assert_eq!(recovery.approvals_received, 1);

        store
            .add_approval("charlie_authority_11111", "Charlie")
            .await;
        let recovery = store.get_recovery().await;
        assert_eq!(recovery.approvals_received, 2);
        assert_eq!(recovery.state, RecoveryState::Completed);
    }
}
