//! # TUI Query Types
//!
//! Datalog query types for TUI data access. Each query type generates
//! Biscuit Datalog rules that can be executed against the journal.
//!
//! ## Type Sources
//!
//! - `Channel`, `ChannelType`, `Message` - imported from `aura-app` (portable view types)
//! - Query types (`ChannelsQuery`, `MessagesQuery`, etc.) - defined locally (TUI-specific)
//! - Guardian/Recovery/Invitation types - defined locally (have TUI-specific fields)

use std::fmt;

// Import portable view types from aura-app
pub use aura_app::{Channel, ChannelType, Message};

/// Trait for queries that generate Datalog rules
pub trait TuiQuery {
    /// The result type returned by this query
    type Result;

    /// Generate the Datalog rule string
    fn to_datalog(&self) -> String;

    /// Get predicates this query depends on (for subscription filtering)
    fn predicates(&self) -> Vec<&'static str>;
}

// =============================================================================
// Channel Query Types
// =============================================================================
// Note: Channel, ChannelType, and Message are imported from aura-app

/// Query for available channels
#[derive(Debug, Clone, Default)]
pub struct ChannelsQuery {
    /// Filter by channel type
    pub channel_type: Option<ChannelType>,
    /// Include channels with unread messages only
    pub unread_only: bool,
    /// Maximum number of channels to return
    pub limit: Option<usize>,
}

impl ChannelsQuery {
    /// Create a new channels query
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter to group/block channels only
    pub fn groups_only(mut self) -> Self {
        self.channel_type = Some(ChannelType::Block);
        self
    }

    /// Filter to DMs only
    pub fn dms_only(mut self) -> Self {
        self.channel_type = Some(ChannelType::DirectMessage);
        self
    }

    /// Show only channels with unread messages
    pub fn unread_only(mut self) -> Self {
        self.unread_only = true;
        self
    }

    /// Limit results
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }
}

impl TuiQuery for ChannelsQuery {
    type Result = Vec<Channel>;

    fn to_datalog(&self) -> String {
        let mut rules = vec!["channel($id, $name, $topic, $is_dm, $member_count, $last_activity) <- channel_fact($id, $name, $topic, $is_dm, $member_count, $last_activity)".to_string()];

        // Add type filter
        if let Some(channel_type) = &self.channel_type {
            match channel_type {
                ChannelType::Block => {
                    rules.push("channel($id, $name, $topic, $is_dm, $member_count, $last_activity), $is_dm == false".to_string());
                }
                ChannelType::DirectMessage => {
                    rules.push("channel($id, $name, $topic, $is_dm, $member_count, $last_activity), $is_dm == true".to_string());
                }
                ChannelType::Guardian => {
                    // Guardian channels - for now treat same as block channels
                    rules.push("channel($id, $name, $topic, $is_dm, $member_count, $last_activity), $is_dm == false".to_string());
                }
                ChannelType::All => {}
            }
        }

        rules.join("\n")
    }

    fn predicates(&self) -> Vec<&'static str> {
        vec!["channel_fact", "channel_membership", "message"]
    }
}

// =============================================================================
// Message Query Types
// =============================================================================
// Note: Message is imported from aura-app

/// Query for messages in a channel
#[derive(Debug, Clone)]
pub struct MessagesQuery {
    /// Channel to query messages from
    pub channel_id: String,
    /// Start timestamp (inclusive)
    pub since: Option<u64>,
    /// End timestamp (exclusive)
    pub until: Option<u64>,
    /// Maximum number of messages
    pub limit: Option<usize>,
    /// Offset for pagination
    pub offset: Option<usize>,
}

impl MessagesQuery {
    /// Create a new messages query for a channel
    pub fn new(channel_id: impl Into<String>) -> Self {
        Self {
            channel_id: channel_id.into(),
            since: None,
            until: None,
            limit: Some(100), // Default limit
            offset: None,
        }
    }

    /// Filter messages since timestamp
    pub fn since(mut self, timestamp: u64) -> Self {
        self.since = Some(timestamp);
        self
    }

    /// Filter messages until timestamp
    pub fn until(mut self, timestamp: u64) -> Self {
        self.until = Some(timestamp);
        self
    }

    /// Limit results
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    /// Offset for pagination
    pub fn offset(mut self, n: usize) -> Self {
        self.offset = Some(n);
        self
    }
}

impl TuiQuery for MessagesQuery {
    type Result = Vec<Message>;

    fn to_datalog(&self) -> String {
        let mut rule = format!(
            "message($id, $channel, $sender, $sender_name, $content, $timestamp, $read, $reply_to) <- \
             message_fact($id, $channel, $sender, $sender_name, $content, $timestamp, $read, $reply_to), \
             $channel == \"{}\"",
            self.channel_id
        );

        if let Some(since) = self.since {
            rule.push_str(&format!(", $timestamp >= {}", since));
        }

        if let Some(until) = self.until {
            rule.push_str(&format!(", $timestamp < {}", until));
        }

        rule
    }

    fn predicates(&self) -> Vec<&'static str> {
        vec!["message_fact", "read_receipt"]
    }
}

// =============================================================================
// Guardian Types
// =============================================================================

/// Guardian status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardianStatus {
    /// Guardian is active and reachable
    Active,
    /// Guardian is pending acceptance
    Pending,
    /// Guardian is offline or unreachable
    Offline,
    /// Guardian declined the invitation
    Declined,
    /// Guardian was removed
    Removed,
}

impl Default for GuardianStatus {
    fn default() -> Self {
        Self::Pending
    }
}

impl fmt::Display for GuardianStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "Active"),
            Self::Pending => write!(f, "Pending"),
            Self::Offline => write!(f, "Offline"),
            Self::Declined => write!(f, "Declined"),
            Self::Removed => write!(f, "Removed"),
        }
    }
}

/// A guardian in the account's recovery network
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guardian {
    /// Guardian's authority ID
    pub authority_id: String,
    /// Guardian's display name (petname)
    pub name: String,
    /// Guardian's status
    pub status: GuardianStatus,
    /// When this guardian was added
    pub added_at: u64,
    /// Last seen timestamp
    pub last_seen: Option<u64>,
    /// Guardian's share index (for threshold signing)
    pub share_index: Option<u32>,
}

impl Default for Guardian {
    fn default() -> Self {
        Self {
            authority_id: String::new(),
            name: String::new(),
            status: GuardianStatus::Pending,
            added_at: 0,
            last_seen: None,
            share_index: None,
        }
    }
}

/// Query for guardians
#[derive(Debug, Clone, Default)]
pub struct GuardiansQuery {
    /// Filter by status
    pub status: Option<GuardianStatus>,
    /// Include only guardians with shares
    pub with_shares_only: bool,
}

impl GuardiansQuery {
    /// Create a new guardians query
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by status
    pub fn with_status(mut self, status: GuardianStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// Only include guardians with shares
    pub fn with_shares_only(mut self) -> Self {
        self.with_shares_only = true;
        self
    }
}

impl TuiQuery for GuardiansQuery {
    type Result = Vec<Guardian>;

    fn to_datalog(&self) -> String {
        let base_rule = "guardian($authority_id, $name, $status, $added_at, $last_seen, $share_index) <- \
                         guardian_fact($authority_id, $name, $status, $added_at, $last_seen, $share_index)";

        let mut rule = base_rule.to_string();

        if let Some(status) = &self.status {
            let status_str = match status {
                GuardianStatus::Active => "active",
                GuardianStatus::Pending => "pending",
                GuardianStatus::Offline => "offline",
                GuardianStatus::Declined => "declined",
                GuardianStatus::Removed => "removed",
            };
            rule.push_str(&format!(", $status == \"{}\"", status_str));
        }

        if self.with_shares_only {
            rule.push_str(", $share_index != null");
        }

        rule
    }

    fn predicates(&self) -> Vec<&'static str> {
        vec!["guardian_fact", "guardian_share", "guardian_status"]
    }
}

// =============================================================================
// Recovery Types
// =============================================================================

/// Recovery session state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryState {
    /// No recovery in progress
    None,
    /// Recovery initiated, waiting for guardian approvals
    Initiated,
    /// Threshold met, ready to complete
    ThresholdMet,
    /// Recovery in progress (key reconstruction)
    InProgress,
    /// Recovery completed successfully
    Completed,
    /// Recovery failed
    Failed,
    /// Recovery cancelled
    Cancelled,
}

impl Default for RecoveryState {
    fn default() -> Self {
        Self::None
    }
}

impl fmt::Display for RecoveryState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Initiated => write!(f, "Initiated"),
            Self::ThresholdMet => write!(f, "Threshold Met"),
            Self::InProgress => write!(f, "In Progress"),
            Self::Completed => write!(f, "Completed"),
            Self::Failed => write!(f, "Failed"),
            Self::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// Guardian approval for recovery
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardianApproval {
    /// Guardian's authority ID
    pub guardian_id: String,
    /// Guardian's display name
    pub guardian_name: String,
    /// Whether approved
    pub approved: bool,
    /// Approval timestamp
    pub timestamp: Option<u64>,
}

/// Recovery session status
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryStatus {
    /// Session identifier
    pub session_id: Option<String>,
    /// Current state
    pub state: RecoveryState,
    /// Number of approvals received
    pub approvals_received: u32,
    /// Required threshold
    pub threshold: u32,
    /// Total guardians
    pub total_guardians: u32,
    /// Individual guardian approvals
    pub approvals: Vec<GuardianApproval>,
    /// Session start time
    pub started_at: Option<u64>,
    /// Session expiry time
    pub expires_at: Option<u64>,
    /// Error message (if failed)
    pub error: Option<String>,
}

impl Default for RecoveryStatus {
    fn default() -> Self {
        Self {
            session_id: None,
            state: RecoveryState::None,
            approvals_received: 0,
            threshold: 0,
            total_guardians: 0,
            approvals: Vec::new(),
            started_at: None,
            expires_at: None,
            error: None,
        }
    }
}

/// Query for recovery status
#[derive(Debug, Clone, Default)]
pub struct RecoveryQuery {
    /// Specific session ID to query (None = active session)
    pub session_id: Option<String>,
    /// Include historical sessions
    pub include_history: bool,
}

impl RecoveryQuery {
    /// Create a new recovery query for the active session
    pub fn active() -> Self {
        Self::default()
    }

    /// Query a specific session
    pub fn session(session_id: impl Into<String>) -> Self {
        Self {
            session_id: Some(session_id.into()),
            include_history: false,
        }
    }

    /// Include historical sessions
    pub fn with_history(mut self) -> Self {
        self.include_history = true;
        self
    }
}

impl TuiQuery for RecoveryQuery {
    type Result = RecoveryStatus;

    fn to_datalog(&self) -> String {
        let base_rule = "recovery_session($session_id, $state, $threshold, $total_guardians, $started_at, $expires_at) <- \
                         recovery_fact($session_id, $state, $threshold, $total_guardians, $started_at, $expires_at)";

        let mut rule = base_rule.to_string();

        if let Some(session_id) = &self.session_id {
            rule.push_str(&format!(", $session_id == \"{}\"", session_id));
        } else if !self.include_history {
            // Only active sessions (not completed, failed, or cancelled)
            rule.push_str(
                ", $state != \"completed\", $state != \"failed\", $state != \"cancelled\"",
            );
        }

        rule
    }

    fn predicates(&self) -> Vec<&'static str> {
        vec!["recovery_fact", "recovery_approval", "guardian_fact"]
    }
}

// =============================================================================
// Invitation Types
// =============================================================================

/// Invitation direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvitationDirection {
    /// Invitation sent by current user
    Outbound,
    /// Invitation received by current user
    Inbound,
}

/// Invitation status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvitationStatus {
    /// Invitation pending response
    Pending,
    /// Invitation accepted
    Accepted,
    /// Invitation declined
    Declined,
    /// Invitation expired
    Expired,
    /// Invitation cancelled
    Cancelled,
}

impl Default for InvitationStatus {
    fn default() -> Self {
        Self::Pending
    }
}

impl fmt::Display for InvitationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "Pending"),
            Self::Accepted => write!(f, "Accepted"),
            Self::Declined => write!(f, "Declined"),
            Self::Expired => write!(f, "Expired"),
            Self::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// An invitation (guardian or channel)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invitation {
    /// Invitation identifier
    pub id: String,
    /// Invitation direction
    pub direction: InvitationDirection,
    /// The other party's authority ID
    pub other_party_id: String,
    /// The other party's display name
    pub other_party_name: String,
    /// Invitation type (guardian, channel, etc.)
    pub invitation_type: InvitationType,
    /// Current status
    pub status: InvitationStatus,
    /// When the invitation was created
    pub created_at: u64,
    /// When the invitation expires
    pub expires_at: Option<u64>,
    /// Optional message
    pub message: Option<String>,
}

impl Default for Invitation {
    fn default() -> Self {
        Self {
            id: String::new(),
            direction: InvitationDirection::Inbound,
            other_party_id: String::new(),
            other_party_name: String::new(),
            invitation_type: InvitationType::Guardian,
            status: InvitationStatus::Pending,
            created_at: 0,
            expires_at: None,
            message: None,
        }
    }
}

/// Type of invitation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvitationType {
    /// Guardian invitation
    Guardian,
    /// Channel invitation
    Channel,
    /// Contact invitation
    Contact,
}

impl Default for InvitationType {
    fn default() -> Self {
        Self::Guardian
    }
}

/// Query for invitations
#[derive(Debug, Clone, Default)]
pub struct InvitationsQuery {
    /// Filter by direction
    pub direction: Option<InvitationDirection>,
    /// Filter by status
    pub status: Option<InvitationStatus>,
    /// Filter by type
    pub invitation_type: Option<InvitationType>,
    /// Include expired invitations
    pub include_expired: bool,
}

impl InvitationsQuery {
    /// Create a new invitations query
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter to inbound invitations
    pub fn inbound(mut self) -> Self {
        self.direction = Some(InvitationDirection::Inbound);
        self
    }

    /// Filter to outbound invitations
    pub fn outbound(mut self) -> Self {
        self.direction = Some(InvitationDirection::Outbound);
        self
    }

    /// Filter by status
    pub fn with_status(mut self, status: InvitationStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// Filter to guardian invitations
    pub fn guardians_only(mut self) -> Self {
        self.invitation_type = Some(InvitationType::Guardian);
        self
    }

    /// Include expired invitations
    pub fn include_expired(mut self) -> Self {
        self.include_expired = true;
        self
    }
}

impl TuiQuery for InvitationsQuery {
    type Result = Vec<Invitation>;

    fn to_datalog(&self) -> String {
        let base_rule = "invitation($id, $direction, $other_party_id, $other_party_name, $type, $status, $created_at, $expires_at, $message) <- \
                         invitation_fact($id, $direction, $other_party_id, $other_party_name, $type, $status, $created_at, $expires_at, $message)";

        let mut rule = base_rule.to_string();

        if let Some(direction) = &self.direction {
            let dir_str = match direction {
                InvitationDirection::Outbound => "outbound",
                InvitationDirection::Inbound => "inbound",
            };
            rule.push_str(&format!(", $direction == \"{}\"", dir_str));
        }

        if let Some(status) = &self.status {
            let status_str = match status {
                InvitationStatus::Pending => "pending",
                InvitationStatus::Accepted => "accepted",
                InvitationStatus::Declined => "declined",
                InvitationStatus::Expired => "expired",
                InvitationStatus::Cancelled => "cancelled",
            };
            rule.push_str(&format!(", $status == \"{}\"", status_str));
        }

        if let Some(inv_type) = &self.invitation_type {
            let type_str = match inv_type {
                InvitationType::Guardian => "guardian",
                InvitationType::Channel => "channel",
                InvitationType::Contact => "contact",
            };
            rule.push_str(&format!(", $type == \"{}\"", type_str));
        }

        if !self.include_expired {
            rule.push_str(", $status != \"expired\"");
        }

        rule
    }

    fn predicates(&self) -> Vec<&'static str> {
        vec!["invitation_fact"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channels_query_to_datalog() {
        let query = ChannelsQuery::new();
        let datalog = query.to_datalog();
        assert!(datalog.contains("channel_fact"));
    }

    #[test]
    fn test_channels_query_groups_only() {
        let query = ChannelsQuery::new().groups_only();
        let datalog = query.to_datalog();
        assert!(datalog.contains("$is_dm == false"));
    }

    #[test]
    fn test_messages_query_to_datalog() {
        let query = MessagesQuery::new("general");
        let datalog = query.to_datalog();
        assert!(datalog.contains("message_fact"));
        assert!(datalog.contains("\"general\""));
    }

    #[test]
    fn test_messages_query_with_time_range() {
        let query = MessagesQuery::new("channel1").since(1000).until(2000);
        let datalog = query.to_datalog();
        assert!(datalog.contains(">= 1000"));
        assert!(datalog.contains("< 2000"));
    }

    #[test]
    fn test_guardians_query_to_datalog() {
        let query = GuardiansQuery::new();
        let datalog = query.to_datalog();
        assert!(datalog.contains("guardian_fact"));
    }

    #[test]
    fn test_guardians_query_with_status() {
        let query = GuardiansQuery::new().with_status(GuardianStatus::Active);
        let datalog = query.to_datalog();
        assert!(datalog.contains("\"active\""));
    }

    #[test]
    fn test_recovery_query_to_datalog() {
        let query = RecoveryQuery::active();
        let datalog = query.to_datalog();
        assert!(datalog.contains("recovery_fact"));
    }

    #[test]
    fn test_recovery_query_specific_session() {
        let query = RecoveryQuery::session("session123");
        let datalog = query.to_datalog();
        assert!(datalog.contains("\"session123\""));
    }

    #[test]
    fn test_invitations_query_to_datalog() {
        let query = InvitationsQuery::new();
        let datalog = query.to_datalog();
        assert!(datalog.contains("invitation_fact"));
    }

    #[test]
    fn test_invitations_query_inbound() {
        let query = InvitationsQuery::new().inbound();
        let datalog = query.to_datalog();
        assert!(datalog.contains("\"inbound\""));
    }

    #[test]
    fn test_guardian_status_display() {
        assert_eq!(format!("{}", GuardianStatus::Active), "Active");
        assert_eq!(format!("{}", GuardianStatus::Pending), "Pending");
    }

    #[test]
    fn test_recovery_state_display() {
        assert_eq!(format!("{}", RecoveryState::ThresholdMet), "Threshold Met");
        assert_eq!(format!("{}", RecoveryState::InProgress), "In Progress");
    }

    #[test]
    fn test_invitation_status_display() {
        assert_eq!(format!("{}", InvitationStatus::Pending), "Pending");
        assert_eq!(format!("{}", InvitationStatus::Accepted), "Accepted");
    }
}
