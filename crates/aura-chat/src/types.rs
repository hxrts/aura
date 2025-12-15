//! Chat data types and structures

use aura_core::identifiers::AuthorityId;
use aura_core::time::TimeStamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Sync Status Types (for optimistic UI)
// ============================================================================

/// UI-level tracking of operation/fact sync status
///
/// This is VIEW state, not journal state. It tracks whether local facts
/// have been synced to peers without affecting the underlying fact system.
///
/// # Usage
///
/// ```rust
/// use aura_chat::types::SyncStatus;
///
/// let status = SyncStatus::LocalOnly;
/// assert!(!status.is_synced());
///
/// let syncing = SyncStatus::Syncing { peers_synced: 2, peers_total: 5 };
/// assert!(syncing.is_partial());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncStatus {
    /// Fact committed locally only, not yet synced to any peer
    LocalOnly,

    /// Fact is being synced to peers
    Syncing {
        /// Number of peers that have received this fact
        peers_synced: usize,
        /// Total number of peers to sync with
        peers_total: usize,
    },

    /// Fact has been synced to all known peers
    Synced,

    /// Sync failed, will retry
    SyncFailed {
        /// Timestamp (ms since epoch) when retry will be attempted
        retry_at_ms: u64,
        /// Number of retry attempts so far
        retry_count: u32,
        /// Optional error message
        error: Option<String>,
    },
}

impl Default for SyncStatus {
    fn default() -> Self {
        SyncStatus::LocalOnly
    }
}

impl SyncStatus {
    /// Check if fully synced to all peers
    pub fn is_synced(&self) -> bool {
        matches!(self, SyncStatus::Synced)
    }

    /// Check if local only (not yet synced)
    pub fn is_local_only(&self) -> bool {
        matches!(self, SyncStatus::LocalOnly)
    }

    /// Check if currently syncing
    pub fn is_syncing(&self) -> bool {
        matches!(self, SyncStatus::Syncing { .. })
    }

    /// Check if sync failed
    pub fn is_failed(&self) -> bool {
        matches!(self, SyncStatus::SyncFailed { .. })
    }

    /// Check if partially synced (some but not all peers)
    pub fn is_partial(&self) -> bool {
        matches!(self, SyncStatus::Syncing { peers_synced, peers_total } if *peers_synced > 0 && peers_synced < peers_total)
    }

    /// Get sync progress as percentage (0-100)
    pub fn progress_percent(&self) -> u8 {
        match self {
            SyncStatus::LocalOnly => 0,
            SyncStatus::Syncing {
                peers_synced,
                peers_total,
            } => {
                if *peers_total == 0 {
                    100
                } else {
                    ((peers_synced * 100) / peers_total).min(100) as u8
                }
            }
            SyncStatus::Synced => 100,
            SyncStatus::SyncFailed { .. } => 0,
        }
    }

    /// Update to syncing state with one more peer synced
    pub fn peer_synced(self) -> Self {
        match self {
            SyncStatus::LocalOnly => SyncStatus::Syncing {
                peers_synced: 1,
                peers_total: 1,
            },
            SyncStatus::Syncing {
                peers_synced,
                peers_total,
            } => {
                let new_synced = peers_synced + 1;
                if new_synced >= peers_total {
                    SyncStatus::Synced
                } else {
                    SyncStatus::Syncing {
                        peers_synced: new_synced,
                        peers_total,
                    }
                }
            }
            other => other, // Already synced or failed
        }
    }

    /// Mark as failed with retry info
    pub fn mark_failed(self, retry_at_ms: u64, error: Option<String>) -> Self {
        let retry_count = match self {
            SyncStatus::SyncFailed { retry_count, .. } => retry_count + 1,
            _ => 1,
        };
        SyncStatus::SyncFailed {
            retry_at_ms,
            retry_count,
            error,
        }
    }
}

/// Message delivery status for tracking message delivery and read receipts
///
/// This extends SyncStatus with message-specific delivery semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryStatus {
    /// Message is being sent
    Sending,

    /// Message sent to server/relay but not yet delivered to recipient
    Sent {
        /// Timestamp when message was sent
        sent_at_ms: u64,
    },

    /// Message delivered to recipient's device
    Delivered {
        /// Timestamp when message was sent
        sent_at_ms: u64,
        /// Timestamp when delivery was confirmed
        delivered_at_ms: u64,
    },

    /// Message read by recipient
    Read {
        /// Timestamp when message was sent
        sent_at_ms: u64,
        /// Timestamp when delivery was confirmed
        delivered_at_ms: u64,
        /// Timestamp when message was read
        read_at_ms: u64,
    },

    /// Message delivery failed
    Failed {
        /// Error message
        error: String,
        /// Number of retry attempts
        retry_count: u32,
    },
}

impl Default for DeliveryStatus {
    fn default() -> Self {
        DeliveryStatus::Sending
    }
}

impl DeliveryStatus {
    /// Create a new Sent status
    pub fn sent(sent_at_ms: u64) -> Self {
        DeliveryStatus::Sent { sent_at_ms }
    }

    /// Upgrade to Delivered status
    pub fn mark_delivered(self, delivered_at_ms: u64) -> Self {
        match self {
            DeliveryStatus::Sent { sent_at_ms } => DeliveryStatus::Delivered {
                sent_at_ms,
                delivered_at_ms,
            },
            DeliveryStatus::Delivered { sent_at_ms, .. } => DeliveryStatus::Delivered {
                sent_at_ms,
                delivered_at_ms,
            },
            other => other,
        }
    }

    /// Upgrade to Read status
    pub fn mark_read(self, read_at_ms: u64) -> Self {
        match self {
            DeliveryStatus::Delivered {
                sent_at_ms,
                delivered_at_ms,
            } => DeliveryStatus::Read {
                sent_at_ms,
                delivered_at_ms,
                read_at_ms,
            },
            DeliveryStatus::Sent { sent_at_ms } => DeliveryStatus::Read {
                sent_at_ms,
                delivered_at_ms: read_at_ms,
                read_at_ms,
            },
            DeliveryStatus::Read {
                sent_at_ms,
                delivered_at_ms,
                ..
            } => DeliveryStatus::Read {
                sent_at_ms,
                delivered_at_ms,
                read_at_ms,
            },
            other => other,
        }
    }

    /// Mark as failed
    pub fn mark_failed(self, error: String) -> Self {
        let retry_count = match self {
            DeliveryStatus::Failed { retry_count, .. } => retry_count + 1,
            _ => 1,
        };
        DeliveryStatus::Failed { error, retry_count }
    }

    /// Check if message is still in transit
    pub fn is_pending(&self) -> bool {
        matches!(self, DeliveryStatus::Sending)
    }

    /// Check if message was sent successfully
    pub fn is_sent(&self) -> bool {
        matches!(
            self,
            DeliveryStatus::Sent { .. }
                | DeliveryStatus::Delivered { .. }
                | DeliveryStatus::Read { .. }
        )
    }

    /// Check if message was delivered
    pub fn is_delivered(&self) -> bool {
        matches!(
            self,
            DeliveryStatus::Delivered { .. } | DeliveryStatus::Read { .. }
        )
    }

    /// Check if message was read
    pub fn is_read(&self) -> bool {
        matches!(self, DeliveryStatus::Read { .. })
    }

    /// Check if delivery failed
    pub fn is_failed(&self) -> bool {
        matches!(self, DeliveryStatus::Failed { .. })
    }

    /// Get UI indicator symbol
    pub fn indicator(&self) -> &'static str {
        match self {
            DeliveryStatus::Sending => "◐", // Pulsing/loading
            DeliveryStatus::Sent { .. } => "✓",
            DeliveryStatus::Delivered { .. } => "✓✓",
            DeliveryStatus::Read { .. } => "✓✓", // Could be colored differently in UI
            DeliveryStatus::Failed { .. } => "✗",
        }
    }
}

/// Confirmation status for multi-party operations
///
/// Tracks distributed confirmation for optimistic operations that require
/// agreement from multiple parties (e.g., channel creation, permission changes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfirmationStatus {
    /// Applied locally only, no confirmation ceremony started
    LocalOnly,

    /// Background confirmation ceremony in progress
    Confirming {
        /// Number of parties that have confirmed
        confirmed_count: usize,
        /// Total number of parties that need to confirm
        total_parties: usize,
        /// Timestamp when confirmation started
        started_at_ms: u64,
    },

    /// All required parties confirmed
    Confirmed {
        /// Timestamp when confirmation completed
        confirmed_at_ms: u64,
    },

    /// Some parties confirmed, some declined or unavailable
    PartiallyConfirmed {
        /// Number of parties that confirmed
        confirmed_count: usize,
        /// Number of parties that declined
        declined_count: usize,
        /// Number of parties that are unavailable
        unavailable_count: usize,
    },

    /// Confirmation failed or was rejected
    Unconfirmed {
        /// Reason for non-confirmation
        reason: String,
        /// Number of retry attempts
        retry_count: u32,
        /// Optional timestamp for next retry
        next_retry_at_ms: Option<u64>,
    },

    /// Operation was rolled back due to conflict or rejection
    RolledBack {
        /// Reason for rollback
        reason: String,
        /// Timestamp when rollback occurred
        rolled_back_at_ms: u64,
    },
}

impl Default for ConfirmationStatus {
    fn default() -> Self {
        ConfirmationStatus::LocalOnly
    }
}

impl ConfirmationStatus {
    /// Check if fully confirmed
    pub fn is_confirmed(&self) -> bool {
        matches!(self, ConfirmationStatus::Confirmed { .. })
    }

    /// Check if still in progress
    pub fn is_pending(&self) -> bool {
        matches!(
            self,
            ConfirmationStatus::LocalOnly | ConfirmationStatus::Confirming { .. }
        )
    }

    /// Check if failed or rolled back
    pub fn is_failed(&self) -> bool {
        matches!(
            self,
            ConfirmationStatus::Unconfirmed { .. } | ConfirmationStatus::RolledBack { .. }
        )
    }

    /// Get UI indicator symbol
    pub fn indicator(&self) -> &'static str {
        match self {
            ConfirmationStatus::LocalOnly => "◌",
            ConfirmationStatus::Confirming { .. } => "◐",
            ConfirmationStatus::Confirmed { .. } => "✓",
            ConfirmationStatus::PartiallyConfirmed { .. } => "⚠",
            ConfirmationStatus::Unconfirmed { .. } => "⚠",
            ConfirmationStatus::RolledBack { .. } => "✗",
        }
    }
}

// ============================================================================
// Original Chat Types
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
    /// Display name for the member
    pub display_name: String,
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // SyncStatus Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_sync_status_default() {
        let status = SyncStatus::default();
        assert!(status.is_local_only());
        assert!(!status.is_synced());
        assert!(!status.is_syncing());
        assert!(!status.is_failed());
    }

    #[test]
    fn test_sync_status_progress_percent() {
        assert_eq!(SyncStatus::LocalOnly.progress_percent(), 0);
        assert_eq!(SyncStatus::Synced.progress_percent(), 100);
        assert_eq!(
            SyncStatus::Syncing {
                peers_synced: 2,
                peers_total: 5
            }
            .progress_percent(),
            40
        );
        assert_eq!(
            SyncStatus::Syncing {
                peers_synced: 0,
                peers_total: 10
            }
            .progress_percent(),
            0
        );
        assert_eq!(
            SyncStatus::SyncFailed {
                retry_at_ms: 0,
                retry_count: 1,
                error: None
            }
            .progress_percent(),
            0
        );
        // Edge case: zero peers_total
        assert_eq!(
            SyncStatus::Syncing {
                peers_synced: 0,
                peers_total: 0
            }
            .progress_percent(),
            100
        );
    }

    #[test]
    fn test_sync_status_peer_synced() {
        // LocalOnly -> Syncing with 1 peer
        let status = SyncStatus::LocalOnly.peer_synced();
        assert!(matches!(
            status,
            SyncStatus::Syncing {
                peers_synced: 1,
                peers_total: 1
            }
        ));

        // Syncing -> increment
        let status = SyncStatus::Syncing {
            peers_synced: 2,
            peers_total: 5,
        }
        .peer_synced();
        assert!(matches!(
            status,
            SyncStatus::Syncing {
                peers_synced: 3,
                peers_total: 5
            }
        ));

        // Syncing -> Synced when all peers done
        let status = SyncStatus::Syncing {
            peers_synced: 4,
            peers_total: 5,
        }
        .peer_synced();
        assert!(status.is_synced());

        // Already synced stays synced
        let status = SyncStatus::Synced.peer_synced();
        assert!(status.is_synced());
    }

    #[test]
    fn test_sync_status_mark_failed() {
        let status = SyncStatus::LocalOnly.mark_failed(1000, Some("network error".to_string()));
        assert!(status.is_failed());
        if let SyncStatus::SyncFailed {
            retry_at_ms,
            retry_count,
            error,
        } = status
        {
            assert_eq!(retry_at_ms, 1000);
            assert_eq!(retry_count, 1);
            assert_eq!(error, Some("network error".to_string()));
        }

        // Retry count increments
        let status = SyncStatus::SyncFailed {
            retry_at_ms: 500,
            retry_count: 2,
            error: None,
        }
        .mark_failed(2000, Some("still failing".to_string()));
        if let SyncStatus::SyncFailed { retry_count, .. } = status {
            assert_eq!(retry_count, 3);
        }
    }

    #[test]
    fn test_sync_status_is_partial() {
        assert!(!SyncStatus::LocalOnly.is_partial());
        assert!(!SyncStatus::Synced.is_partial());
        assert!(SyncStatus::Syncing {
            peers_synced: 2,
            peers_total: 5
        }
        .is_partial());
        assert!(!SyncStatus::Syncing {
            peers_synced: 0,
            peers_total: 5
        }
        .is_partial());
        assert!(!SyncStatus::Syncing {
            peers_synced: 5,
            peers_total: 5
        }
        .is_partial());
    }

    // ------------------------------------------------------------------------
    // DeliveryStatus Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_delivery_status_default() {
        let status = DeliveryStatus::default();
        assert!(status.is_pending());
        assert!(!status.is_sent());
        assert!(!status.is_delivered());
        assert!(!status.is_read());
        assert!(!status.is_failed());
    }

    #[test]
    fn test_delivery_status_sent() {
        let status = DeliveryStatus::sent(1000);
        assert!(status.is_sent());
        assert!(!status.is_pending());
        if let DeliveryStatus::Sent { sent_at_ms } = status {
            assert_eq!(sent_at_ms, 1000);
        }
    }

    #[test]
    fn test_delivery_status_mark_delivered() {
        let status = DeliveryStatus::sent(1000).mark_delivered(2000);
        assert!(status.is_delivered());
        assert!(status.is_sent()); // is_sent includes delivered
        if let DeliveryStatus::Delivered {
            sent_at_ms,
            delivered_at_ms,
        } = status
        {
            assert_eq!(sent_at_ms, 1000);
            assert_eq!(delivered_at_ms, 2000);
        }

        // mark_delivered on Sending does nothing
        let status = DeliveryStatus::Sending.mark_delivered(2000);
        assert!(matches!(status, DeliveryStatus::Sending));
    }

    #[test]
    fn test_delivery_status_mark_read() {
        let status = DeliveryStatus::Delivered {
            sent_at_ms: 1000,
            delivered_at_ms: 2000,
        }
        .mark_read(3000);
        assert!(status.is_read());
        assert!(status.is_delivered()); // is_delivered includes read
        if let DeliveryStatus::Read {
            sent_at_ms,
            delivered_at_ms,
            read_at_ms,
        } = status
        {
            assert_eq!(sent_at_ms, 1000);
            assert_eq!(delivered_at_ms, 2000);
            assert_eq!(read_at_ms, 3000);
        }

        // mark_read on Sent (skipping delivered)
        let status = DeliveryStatus::sent(1000).mark_read(3000);
        assert!(status.is_read());
        if let DeliveryStatus::Read {
            sent_at_ms,
            delivered_at_ms,
            read_at_ms,
        } = status
        {
            assert_eq!(sent_at_ms, 1000);
            assert_eq!(delivered_at_ms, 3000); // Uses read time as delivery time
            assert_eq!(read_at_ms, 3000);
        }
    }

    #[test]
    fn test_delivery_status_mark_failed() {
        let status = DeliveryStatus::Sending.mark_failed("network error".to_string());
        assert!(status.is_failed());
        if let DeliveryStatus::Failed { error, retry_count } = status {
            assert_eq!(error, "network error");
            assert_eq!(retry_count, 1);
        }

        // Retry count increments
        let status = DeliveryStatus::Failed {
            error: "first".to_string(),
            retry_count: 2,
        }
        .mark_failed("second".to_string());
        if let DeliveryStatus::Failed { retry_count, .. } = status {
            assert_eq!(retry_count, 3);
        }
    }

    #[test]
    fn test_delivery_status_indicator() {
        assert_eq!(DeliveryStatus::Sending.indicator(), "◐");
        assert_eq!(DeliveryStatus::sent(0).indicator(), "✓");
        assert_eq!(
            DeliveryStatus::Delivered {
                sent_at_ms: 0,
                delivered_at_ms: 0
            }
            .indicator(),
            "✓✓"
        );
        assert_eq!(
            DeliveryStatus::Read {
                sent_at_ms: 0,
                delivered_at_ms: 0,
                read_at_ms: 0
            }
            .indicator(),
            "✓✓"
        );
        assert_eq!(
            DeliveryStatus::Failed {
                error: String::new(),
                retry_count: 0
            }
            .indicator(),
            "✗"
        );
    }

    // ------------------------------------------------------------------------
    // ConfirmationStatus Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_confirmation_status_default() {
        let status = ConfirmationStatus::default();
        assert!(status.is_pending());
        assert!(!status.is_confirmed());
        assert!(!status.is_failed());
    }

    #[test]
    fn test_confirmation_status_confirming() {
        let status = ConfirmationStatus::Confirming {
            confirmed_count: 2,
            total_parties: 5,
            started_at_ms: 1000,
        };
        assert!(status.is_pending());
        assert!(!status.is_confirmed());
    }

    #[test]
    fn test_confirmation_status_confirmed() {
        let status = ConfirmationStatus::Confirmed {
            confirmed_at_ms: 2000,
        };
        assert!(status.is_confirmed());
        assert!(!status.is_pending());
        assert!(!status.is_failed());
    }

    #[test]
    fn test_confirmation_status_failed_states() {
        let unconfirmed = ConfirmationStatus::Unconfirmed {
            reason: "timeout".to_string(),
            retry_count: 1,
            next_retry_at_ms: Some(5000),
        };
        assert!(unconfirmed.is_failed());
        assert!(!unconfirmed.is_pending());

        let rolled_back = ConfirmationStatus::RolledBack {
            reason: "conflict".to_string(),
            rolled_back_at_ms: 3000,
        };
        assert!(rolled_back.is_failed());
    }

    #[test]
    fn test_confirmation_status_indicator() {
        assert_eq!(ConfirmationStatus::LocalOnly.indicator(), "◌");
        assert_eq!(
            ConfirmationStatus::Confirming {
                confirmed_count: 0,
                total_parties: 0,
                started_at_ms: 0
            }
            .indicator(),
            "◐"
        );
        assert_eq!(
            ConfirmationStatus::Confirmed { confirmed_at_ms: 0 }.indicator(),
            "✓"
        );
        assert_eq!(
            ConfirmationStatus::PartiallyConfirmed {
                confirmed_count: 0,
                declined_count: 0,
                unavailable_count: 0
            }
            .indicator(),
            "⚠"
        );
        assert_eq!(
            ConfirmationStatus::Unconfirmed {
                reason: String::new(),
                retry_count: 0,
                next_retry_at_ms: None
            }
            .indicator(),
            "⚠"
        );
        assert_eq!(
            ConfirmationStatus::RolledBack {
                reason: String::new(),
                rolled_back_at_ms: 0
            }
            .indicator(),
            "✗"
        );
    }

    // ------------------------------------------------------------------------
    // Serialization Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_sync_status_serialization() {
        let statuses = vec![
            SyncStatus::LocalOnly,
            SyncStatus::Syncing {
                peers_synced: 2,
                peers_total: 5,
            },
            SyncStatus::Synced,
            SyncStatus::SyncFailed {
                retry_at_ms: 1000,
                retry_count: 3,
                error: Some("test".to_string()),
            },
        ];

        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let deserialized: SyncStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, deserialized);
        }
    }

    #[test]
    fn test_delivery_status_serialization() {
        let statuses = vec![
            DeliveryStatus::Sending,
            DeliveryStatus::Sent { sent_at_ms: 1000 },
            DeliveryStatus::Delivered {
                sent_at_ms: 1000,
                delivered_at_ms: 2000,
            },
            DeliveryStatus::Read {
                sent_at_ms: 1000,
                delivered_at_ms: 2000,
                read_at_ms: 3000,
            },
            DeliveryStatus::Failed {
                error: "test".to_string(),
                retry_count: 1,
            },
        ];

        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let deserialized: DeliveryStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, deserialized);
        }
    }

    #[test]
    fn test_confirmation_status_serialization() {
        let statuses = vec![
            ConfirmationStatus::LocalOnly,
            ConfirmationStatus::Confirming {
                confirmed_count: 2,
                total_parties: 5,
                started_at_ms: 1000,
            },
            ConfirmationStatus::Confirmed {
                confirmed_at_ms: 2000,
            },
            ConfirmationStatus::PartiallyConfirmed {
                confirmed_count: 3,
                declined_count: 1,
                unavailable_count: 1,
            },
            ConfirmationStatus::Unconfirmed {
                reason: "test".to_string(),
                retry_count: 2,
                next_retry_at_ms: Some(5000),
            },
            ConfirmationStatus::RolledBack {
                reason: "conflict".to_string(),
                rolled_back_at_ms: 3000,
            },
        ];

        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let deserialized: ConfirmationStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, deserialized);
        }
    }
}
