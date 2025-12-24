//! # Shared Types
//!
//! Domain types used across iocraft components.
//! These use owned types (String, Vec) for compatibility with iocraft's 'static lifetime requirements.

use crate::tui::theme::Theme;
use iocraft::prelude::Color;

// Re-export source types for adapters
use aura_app::views::{
    chat::{Channel as AppChannel, Message as AppMessage},
    contacts::Contact as AppContact,
    invitations::{
        Invitation as AppInvitation, InvitationDirection as AppInvitationDirection,
        InvitationStatus as AppInvitationStatus, InvitationType as AppInvitationType,
    },
    recovery::{
        Guardian as AppGuardian, GuardianStatus as AppGuardianStatus,
        RecoveryApproval as AppRecoveryApproval, RecoveryProcess as AppRecoveryProcess,
        RecoveryProcessStatus as AppRecoveryProcessStatus, RecoveryState as AppRecoveryState,
    },
};

/// A chat channel
#[derive(Clone, Debug, Default)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub topic: Option<String>,
    pub unread_count: usize,
    pub is_selected: bool,
}

impl From<&AppChannel> for Channel {
    fn from(ch: &AppChannel) -> Self {
        Self {
            id: ch.id.to_string(),
            name: ch.name.clone(),
            topic: ch.topic.clone(),
            unread_count: ch.unread_count as usize,
            is_selected: false,
        }
    }
}

impl Channel {
    /// Create from aura_app Channel with selection state
    pub fn from_app(ch: &AppChannel, is_selected: bool) -> Self {
        Self {
            id: ch.id.to_string(),
            name: ch.name.clone(),
            topic: ch.topic.clone(),
            unread_count: ch.unread_count as usize,
            is_selected,
        }
    }
}

impl Channel {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            topic: None,
            unread_count: 0,
            is_selected: false,
        }
    }

    pub fn with_unread(mut self, count: usize) -> Self {
        self.unread_count = count;
        self
    }

    pub fn with_topic(mut self, topic: impl Into<String>) -> Self {
        self.topic = Some(topic.into());
        self
    }

    pub fn selected(mut self, is_selected: bool) -> Self {
        self.is_selected = is_selected;
        self
    }
}

/// Message delivery status
///
/// Tracks the delivery lifecycle of a message:
/// Sending → Sent → Delivered → Read
///
/// The status transitions as the message propagates through the network
/// and is received/read by recipients.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeliveryStatus {
    /// Message is being sent to the network
    Sending,
    /// Message was sent and acknowledged by the network
    #[default]
    Sent,
    /// Message was delivered to recipient's device (before read)
    Delivered,
    /// Message was read by the recipient
    Read,
    /// Message delivery failed (with retry available)
    Failed,
}

impl DeliveryStatus {
    /// Get the status indicator character
    pub fn indicator(&self) -> &'static str {
        match self {
            DeliveryStatus::Sending => "⏳",   // Hourglass
            DeliveryStatus::Sent => "✓",       // Single check (gray)
            DeliveryStatus::Delivered => "✓✓", // Double check (gray)
            DeliveryStatus::Read => "✓✓",      // Double check (blue) - color applied separately
            DeliveryStatus::Failed => "✗",     // X mark
        }
    }

    /// Get a short description
    pub fn description(&self) -> &'static str {
        match self {
            DeliveryStatus::Sending => "Sending...",
            DeliveryStatus::Sent => "Sent",
            DeliveryStatus::Delivered => "Delivered",
            DeliveryStatus::Read => "Read",
            DeliveryStatus::Failed => "Failed",
        }
    }

    /// Whether the message has reached the recipient's device
    pub fn is_delivered(&self) -> bool {
        matches!(self, DeliveryStatus::Delivered | DeliveryStatus::Read)
    }

    /// Whether the message has been read by the recipient
    pub fn is_read(&self) -> bool {
        matches!(self, DeliveryStatus::Read)
    }

    /// Whether the message is still pending (not yet confirmed delivered)
    pub fn is_pending(&self) -> bool {
        matches!(self, DeliveryStatus::Sending | DeliveryStatus::Sent)
    }
}

/// Synchronization status for items that sync across devices/peers
///
/// This is a UI-level view state, not a journal fact. It tracks whether
/// local state has been synchronized with peers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SyncStatus {
    /// Not yet synced to any peers
    #[default]
    LocalOnly,
    /// Currently syncing with peers
    Syncing {
        /// Number of peers successfully synced
        peers_synced: usize,
        /// Total number of peers to sync with
        peers_total: usize,
    },
    /// Successfully synced to all required peers
    Synced,
    /// Sync failed
    SyncFailed {
        /// Error message describing the failure
        error: String,
    },
}

impl SyncStatus {
    /// Get the status indicator character
    pub fn indicator(&self) -> &'static str {
        match self {
            SyncStatus::LocalOnly => "○",         // Empty circle
            SyncStatus::Syncing { .. } => "◐",    // Half-filled circle
            SyncStatus::Synced => "●",            // Filled circle
            SyncStatus::SyncFailed { .. } => "⊗", // Circle with X
        }
    }

    /// Get a short description
    pub fn description(&self) -> String {
        match self {
            SyncStatus::LocalOnly => "Local only".to_string(),
            SyncStatus::Syncing {
                peers_synced,
                peers_total,
            } => {
                format!("Syncing ({}/{})", peers_synced, peers_total)
            }
            SyncStatus::Synced => "Synced".to_string(),
            SyncStatus::SyncFailed { error } => format!("Sync failed: {}", error),
        }
    }

    /// Whether the item is fully synced
    pub fn is_synced(&self) -> bool {
        matches!(self, SyncStatus::Synced)
    }

    /// Whether the item is currently syncing
    pub fn is_syncing(&self) -> bool {
        matches!(self, SyncStatus::Syncing { .. })
    }

    /// Get sync progress as a percentage (0-100)
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
}

/// A chat message
#[derive(Clone, Debug, Default)]
pub struct Message {
    pub id: String,
    /// Channel this message belongs to
    pub channel_id: String,
    pub sender: String,
    pub content: String,
    pub timestamp: String,
    pub is_own: bool,
    /// Delivery status for own messages
    pub delivery_status: DeliveryStatus,
}

impl From<&AppMessage> for Message {
    fn from(msg: &AppMessage) -> Self {
        Self {
            id: msg.id.clone(),
            channel_id: msg.channel_id.to_string(),
            sender: msg.sender_name.clone(),
            content: msg.content.clone(),
            timestamp: format_timestamp(msg.timestamp),
            is_own: msg.is_own,
            // Default to Delivered for messages loaded from storage
            delivery_status: DeliveryStatus::Delivered,
        }
    }
}

impl Message {
    pub fn new(
        id: impl Into<String>,
        sender: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            channel_id: String::new(),
            sender: sender.into(),
            content: content.into(),
            timestamp: String::new(),
            is_own: false,
            delivery_status: DeliveryStatus::default(),
        }
    }

    /// Create a new message in sending state (for optimistic UI)
    pub fn sending(
        id: impl Into<String>,
        channel_id: impl Into<String>,
        sender: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            channel_id: channel_id.into(),
            sender: sender.into(),
            content: content.into(),
            timestamp: String::new(),
            is_own: true,
            delivery_status: DeliveryStatus::Sending,
        }
    }

    /// Builder method to set channel_id
    pub fn with_channel(mut self, channel_id: impl Into<String>) -> Self {
        self.channel_id = channel_id.into();
        self
    }

    /// Builder method to set delivery status
    pub fn with_status(mut self, status: DeliveryStatus) -> Self {
        self.delivery_status = status;
        self
    }

    pub fn with_timestamp(mut self, ts: impl Into<String>) -> Self {
        self.timestamp = ts.into();
        self
    }

    pub fn own(mut self, is_own: bool) -> Self {
        self.is_own = is_own;
        self
    }
}

/// A keyboard shortcut hint
#[derive(Clone, Debug)]
pub struct KeyHint {
    pub key: String,
    pub description: String,
}

impl KeyHint {
    pub fn new(key: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            description: description.into(),
        }
    }
}

/// Navigation direction
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// Invitation filter options
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InvitationFilter {
    #[default]
    All,
    Sent,
    Received,
}

impl InvitationFilter {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Sent,
            Self::Sent => Self::Received,
            Self::Received => Self::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Sent => "Sent",
            Self::Received => "Received",
        }
    }
}

/// Direction of an invitation
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InvitationDirection {
    #[default]
    Outbound,
    Inbound,
}

impl InvitationDirection {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Outbound => "→",
            Self::Inbound => "←",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Outbound => "Sent to",
            Self::Inbound => "Received from",
        }
    }
}

/// Status of an invitation
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InvitationStatus {
    #[default]
    Pending,
    Accepted,
    Declined,
    Expired,
    Cancelled,
}

impl InvitationStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Accepted => "Accepted",
            Self::Declined => "Declined",
            Self::Expired => "Expired",
            Self::Cancelled => "Cancelled",
        }
    }

    /// Get the color for this status
    pub fn color(self) -> Color {
        match self {
            Self::Pending => Theme::WARNING,
            Self::Accepted => Theme::SUCCESS,
            Self::Declined => Theme::ERROR,
            Self::Expired | Self::Cancelled => Theme::LIST_TEXT_MUTED,
        }
    }
}

/// Type of invitation
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InvitationType {
    #[default]
    Guardian,
    Contact,
    Channel,
}

impl InvitationType {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Guardian => "◆",
            Self::Contact => "◯",
            Self::Channel => "◈",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Guardian => "Guardian Invitation",
            Self::Contact => "Contact Invitation",
            Self::Channel => "Channel Invitation",
        }
    }
}

/// An invitation
#[derive(Clone, Debug, Default)]
pub struct Invitation {
    pub id: String,
    pub direction: InvitationDirection,
    pub other_party_id: String,
    pub other_party_name: String,
    pub invitation_type: InvitationType,
    pub status: InvitationStatus,
    pub created_at: u64,
    pub expires_at: Option<u64>,
    pub message: Option<String>,
}

impl Invitation {
    pub fn new(
        id: impl Into<String>,
        other_party_name: impl Into<String>,
        direction: InvitationDirection,
    ) -> Self {
        Self {
            id: id.into(),
            other_party_name: other_party_name.into(),
            direction,
            ..Default::default()
        }
    }

    pub fn with_type(mut self, invitation_type: InvitationType) -> Self {
        self.invitation_type = invitation_type;
        self
    }

    pub fn with_status(mut self, status: InvitationStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

impl From<AppInvitationDirection> for InvitationDirection {
    fn from(d: AppInvitationDirection) -> Self {
        match d {
            AppInvitationDirection::Sent => Self::Outbound,
            AppInvitationDirection::Received => Self::Inbound,
        }
    }
}

impl From<AppInvitationStatus> for InvitationStatus {
    fn from(s: AppInvitationStatus) -> Self {
        match s {
            AppInvitationStatus::Pending => Self::Pending,
            AppInvitationStatus::Accepted => Self::Accepted,
            AppInvitationStatus::Rejected => Self::Declined,
            AppInvitationStatus::Expired => Self::Expired,
            AppInvitationStatus::Revoked => Self::Cancelled,
        }
    }
}

impl From<AppInvitationType> for InvitationType {
    fn from(t: AppInvitationType) -> Self {
        match t {
            AppInvitationType::Guardian => Self::Guardian,
            AppInvitationType::Chat => Self::Channel,
            AppInvitationType::Block => Self::Contact, // Block invitations → Contact in TUI
        }
    }
}

impl From<&AppInvitation> for Invitation {
    fn from(inv: &AppInvitation) -> Self {
        // For direction-aware display names
        let (other_party_id, other_party_name) = match inv.direction {
            AppInvitationDirection::Sent => (
                inv.to_id
                    .as_ref()
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
                inv.to_name.clone().unwrap_or_default(),
            ),
            AppInvitationDirection::Received => (inv.from_id.to_string(), inv.from_name.clone()),
        };

        Self {
            id: inv.id.clone(),
            direction: inv.direction.into(),
            other_party_id,
            other_party_name,
            invitation_type: inv.invitation_type.into(),
            status: inv.status.into(),
            created_at: inv.created_at,
            expires_at: inv.expires_at,
            message: inv.message.clone(),
        }
    }
}

/// Format a timestamp for display
pub fn format_timestamp(ts: u64) -> String {
    let hours = (ts / 3600000) % 24;
    let minutes = (ts / 60000) % 60;
    format!("{:02}:{:02}", hours, minutes)
}

/// Settings section
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsSection {
    #[default]
    Profile,
    Threshold,
    Devices,
    Mfa,
}

impl SettingsSection {
    pub fn all() -> &'static [Self] {
        &[Self::Profile, Self::Threshold, Self::Devices, Self::Mfa]
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Profile => "Profile",
            Self::Threshold => "Guardian Threshold",
            Self::Devices => "Devices",
            Self::Mfa => "Multifactor Auth",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Profile => "Your display name and account information",
            Self::Threshold => "Configure guardians for account recovery",
            Self::Devices => "Manage devices linked to your account",
            Self::Mfa => "Set multifactor authentication requirements",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Profile => Self::Threshold,
            Self::Threshold => Self::Devices,
            Self::Devices => Self::Mfa,
            Self::Mfa => Self::Profile,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Profile => Self::Mfa,
            Self::Threshold => Self::Profile,
            Self::Devices => Self::Threshold,
            Self::Mfa => Self::Devices,
        }
    }
}

/// A registered device
#[derive(Clone, Debug, Default)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub is_current: bool,
    pub last_seen: Option<u64>,
}

impl Device {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            is_current: false,
            last_seen: None,
        }
    }

    pub fn current(mut self) -> Self {
        self.is_current = true;
        self
    }
}

/// MFA policy
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MfaPolicy {
    #[default]
    Disabled,
    SensitiveOnly,
    AlwaysRequired,
}

impl MfaPolicy {
    pub fn name(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::SensitiveOnly => "Sensitive Only",
            Self::AlwaysRequired => "Always Required",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Disabled => "No additional authentication required",
            Self::SensitiveOnly => "MFA for recovery, device changes, and guardian updates",
            Self::AlwaysRequired => "MFA for all authenticated operations",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Disabled => Self::SensitiveOnly,
            Self::SensitiveOnly => Self::AlwaysRequired,
            Self::AlwaysRequired => Self::Disabled,
        }
    }

    /// Returns true if MFA is required for at least some operations
    pub fn requires_mfa(self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

/// Channel mode flags
///
/// IRC-style mode flags for channel configuration:
/// - `m` - moderated: only admins can send messages
/// - `p` - private: channel not visible to non-members
/// - `t` - topic protected: only admins can change topic
/// - `i` - invite only: members must be invited
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelMode {
    /// Moderated - only admins can send messages
    pub moderated: bool,
    /// Private - not visible to non-members
    pub private: bool,
    /// Topic protected - only admins can change topic
    pub topic_protected: bool,
    /// Invite only - must be invited to join
    pub invite_only: bool,
}

impl ChannelMode {
    /// Parse mode flags from a string like "+mpt" or "-i"
    pub fn parse_flags(&mut self, flags: &str) {
        let mut adding = true;
        for c in flags.chars() {
            match c {
                '+' => adding = true,
                '-' => adding = false,
                'm' => self.moderated = adding,
                'p' => self.private = adding,
                't' => self.topic_protected = adding,
                'i' => self.invite_only = adding,
                _ => {} // Ignore unknown flags
            }
        }
    }

    /// Get human-readable description of active modes
    pub fn description(&self) -> Vec<&'static str> {
        let mut desc = Vec::new();
        if self.moderated {
            desc.push("Moderated");
        }
        if self.private {
            desc.push("Private");
        }
        if self.topic_protected {
            desc.push("Topic Protected");
        }
        if self.invite_only {
            desc.push("Invite Only");
        }
        desc
    }
}

impl std::fmt::Display for ChannelMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut flags = String::from("+");
        if self.moderated {
            flags.push('m');
        }
        if self.private {
            flags.push('p');
        }
        if self.topic_protected {
            flags.push('t');
        }
        if self.invite_only {
            flags.push('i');
        }

        if flags.len() == 1 {
            write!(f, "")
        } else {
            write!(f, "{}", flags)
        }
    }
}

/// Recovery screen tab
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecoveryTab {
    #[default]
    Guardians,
    Recovery,
    /// Pending requests from others that we can approve (we are their guardian)
    Requests,
}

impl RecoveryTab {
    pub fn title(self) -> &'static str {
        match self {
            Self::Guardians => "Guardians",
            Self::Recovery => "Recovery",
            Self::Requests => "Requests",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Guardians => Self::Recovery,
            Self::Recovery => Self::Requests,
            Self::Requests => Self::Guardians,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Guardians => Self::Requests,
            Self::Recovery => Self::Guardians,
            Self::Requests => Self::Recovery,
        }
    }

    /// Returns all tabs in order
    pub fn all() -> [Self; 3] {
        [Self::Guardians, Self::Recovery, Self::Requests]
    }
}

/// Guardian status
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GuardianStatus {
    #[default]
    Active,
    Pending,
    Offline,
    Declined,
    Removed,
}

impl GuardianStatus {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Active => "●",
            Self::Offline => "○",
            Self::Pending => "○",
            Self::Declined => "✕",
            Self::Removed => "⊝",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Pending => "Pending",
            Self::Offline => "Offline",
            Self::Declined => "Declined",
            Self::Removed => "Removed",
        }
    }

    /// Get the color for this status
    pub fn color(self) -> Color {
        match self {
            Self::Active => Theme::SUCCESS,
            Self::Offline => Theme::TEXT_DISABLED,
            Self::Pending => Theme::WARNING,
            Self::Declined | Self::Removed => Theme::ERROR,
        }
    }
}

/// A guardian
#[derive(Clone, Debug, Default)]
pub struct Guardian {
    pub id: String,
    pub name: String,
    pub status: GuardianStatus,
    pub has_share: bool,
}

impl From<AppGuardianStatus> for GuardianStatus {
    fn from(status: AppGuardianStatus) -> Self {
        match status {
            AppGuardianStatus::Active => Self::Active,
            AppGuardianStatus::Pending => Self::Pending,
            AppGuardianStatus::Offline => Self::Offline,
            AppGuardianStatus::Revoked => Self::Removed, // Map Revoked to Removed for TUI
        }
    }
}

impl From<&AppGuardian> for Guardian {
    fn from(g: &AppGuardian) -> Self {
        Self {
            id: g.id.to_string(),
            name: g.name.clone(),
            status: g.status.into(),
            has_share: true, // In the unified model, guardians always have shares
        }
    }
}

impl Guardian {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            status: GuardianStatus::Active,
            has_share: false,
        }
    }

    pub fn with_status(mut self, status: GuardianStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_share(mut self) -> Self {
        self.has_share = true;
        self
    }
}

/// Recovery state
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecoveryState {
    #[default]
    None,
    Initiated,
    ThresholdMet,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

impl RecoveryState {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Not Started",
            Self::Initiated => "Awaiting Guardian Approvals",
            Self::ThresholdMet => "Threshold Met - Ready to Complete",
            Self::InProgress => "Reconstructing Keys...",
            Self::Completed => "Recovery Completed!",
            Self::Failed => "Recovery Failed",
            Self::Cancelled => "Recovery Cancelled",
        }
    }
}

/// Guardian approval for recovery
#[derive(Clone, Debug, Default)]
pub struct GuardianApproval {
    pub guardian_name: String,
    pub approved: bool,
}

/// Recovery status
#[derive(Clone, Debug, Default)]
pub struct RecoveryStatus {
    pub state: RecoveryState,
    pub approvals_received: u32,
    pub threshold: u32,
    pub approvals: Vec<GuardianApproval>,
}

impl From<AppRecoveryProcessStatus> for RecoveryState {
    fn from(status: AppRecoveryProcessStatus) -> Self {
        match status {
            AppRecoveryProcessStatus::Idle => Self::None,
            AppRecoveryProcessStatus::Initiated => Self::Initiated,
            AppRecoveryProcessStatus::WaitingForApprovals => Self::Initiated, // Still waiting
            AppRecoveryProcessStatus::Approved => Self::ThresholdMet,
            AppRecoveryProcessStatus::Completed => Self::Completed,
            AppRecoveryProcessStatus::Failed => Self::Failed,
        }
    }
}

impl From<&AppRecoveryApproval> for GuardianApproval {
    fn from(a: &AppRecoveryApproval) -> Self {
        Self {
            guardian_name: a.guardian_id.to_string(), // Will be resolved to name by UI
            approved: true, // If there's an approval record, it's approved
        }
    }
}

impl From<&AppRecoveryState> for RecoveryStatus {
    fn from(rs: &AppRecoveryState) -> Self {
        // Determine state from active_recovery if present
        let (state, approvals_received, threshold, approvals) = match &rs.active_recovery {
            Some(process) => (
                process.status.into(),
                process.approvals_received,
                process.approvals_required,
                process.approvals.iter().map(|a| a.into()).collect(),
            ),
            None => (RecoveryState::None, 0, rs.threshold, Vec::new()),
        };

        Self {
            state,
            approvals_received,
            threshold,
            approvals,
        }
    }
}

impl From<&AppRecoveryProcess> for RecoveryStatus {
    fn from(p: &AppRecoveryProcess) -> Self {
        Self {
            state: p.status.into(),
            approvals_received: p.approvals_received,
            threshold: p.approvals_required,
            approvals: p.approvals.iter().map(|a| a.into()).collect(),
        }
    }
}

/// A pending recovery request that we can approve (we are their guardian)
#[derive(Clone, Debug, Default)]
pub struct PendingRequest {
    /// Recovery request ID
    pub id: String,
    /// Account being recovered (display name or ID)
    pub account_name: String,
    /// Number of approvals received
    pub approvals_received: u32,
    /// Number of approvals required
    pub approvals_required: u32,
    /// Whether we have already approved this request
    pub we_approved: bool,
    /// When the request was initiated (ms since epoch)
    pub initiated_at: u64,
}

impl From<&AppRecoveryProcess> for PendingRequest {
    fn from(p: &AppRecoveryProcess) -> Self {
        Self {
            id: p.id.clone(),
            account_name: p.account_id.to_string(), // Will be resolved to name by UI if possible
            approvals_received: p.approvals_received,
            approvals_required: p.approvals_required,
            we_approved: false, // Caller should set this based on our guardian ID
            initiated_at: p.initiated_at,
        }
    }
}

// =============================================================================
// Contacts Types
// =============================================================================

/// Contact status
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContactStatus {
    #[default]
    Active,
    Offline,
    Pending,
    Blocked,
}

impl ContactStatus {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Active => "●",
            Self::Offline => "○",
            Self::Pending => "○",
            Self::Blocked => "⊗",
        }
    }

    /// Get the color for this status
    pub fn color(self) -> Color {
        match self {
            Self::Active => Theme::SUCCESS,
            Self::Offline => Theme::TEXT_DISABLED,
            Self::Pending => Theme::WARNING,
            Self::Blocked => Theme::ERROR,
        }
    }
}

/// A contact
#[derive(Clone, Debug, Default)]
pub struct Contact {
    pub id: String,
    pub nickname: String,
    pub suggested_name: Option<String>,
    pub status: ContactStatus,
    pub is_guardian: bool,
}

impl Contact {
    pub fn new(id: impl Into<String>, nickname: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            nickname: nickname.into(),
            ..Default::default()
        }
    }

    pub fn with_status(mut self, status: ContactStatus) -> Self {
        self.status = status;
        self
    }

    pub fn guardian(mut self) -> Self {
        self.is_guardian = true;
        self
    }

    pub fn with_suggestion(mut self, name: impl Into<String>) -> Self {
        self.suggested_name = Some(name.into());
        self
    }
}

// =============================================================================
// Block Types
// =============================================================================

/// A resident in a block
#[derive(Clone, Debug, Default)]
pub struct Resident {
    pub id: String,
    pub name: String,
    pub is_steward: bool,
    pub is_self: bool,
}

impl Resident {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn steward(mut self) -> Self {
        self.is_steward = true;
        self
    }

    pub fn is_current_user(mut self) -> Self {
        self.is_self = true;
        self
    }
}

impl From<&aura_app::views::block::Resident> for Resident {
    fn from(r: &aura_app::views::block::Resident) -> Self {
        Self {
            id: r.id.to_string(),
            name: r.name.clone(),
            is_steward: r.is_steward(),
            is_self: false, // Cannot determine from aura-app Resident alone
        }
    }
}

/// Block storage budget
#[derive(Clone, Debug, Default)]
pub struct BlockBudget {
    pub total: u64,
    pub used: u64,
    pub resident_count: u8,
    pub max_residents: u8,
}

impl BlockBudget {
    pub fn usage_percent(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.used as f32 / self.total as f32) * 100.0
        }
    }
}

impl From<&aura_app::BlockFlowBudget> for BlockBudget {
    fn from(budget: &aura_app::BlockFlowBudget) -> Self {
        Self {
            total: budget.total_allocation(),
            used: budget.total_used(),
            resident_count: budget.resident_count,
            max_residents: aura_app::MAX_RESIDENTS,
        }
    }
}

// =============================================================================
// Neighborhood Types
// =============================================================================

/// Block visibility/access level
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TraversalDepth {
    #[default]
    Street,
    Frontage,
    Interior,
}

impl TraversalDepth {
    pub fn label(self) -> &'static str {
        match self {
            Self::Street => "Street",
            Self::Frontage => "Frontage",
            Self::Interior => "Interior",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Street => "→",
            Self::Frontage => "◇",
            Self::Interior => "⌂",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Street => Self::Frontage,
            Self::Frontage => Self::Interior,
            Self::Interior => Self::Street,
        }
    }
}

/// Block summary for neighborhood view
#[derive(Clone, Debug, Default)]
pub struct BlockSummary {
    pub id: String,
    pub name: Option<String>,
    pub resident_count: u8,
    pub max_residents: u8,
    pub is_home: bool,
    pub can_enter: bool,
}

impl BlockSummary {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            max_residents: 8,
            ..Default::default()
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_residents(mut self, count: u8) -> Self {
        self.resident_count = count;
        self
    }

    pub fn home(mut self) -> Self {
        self.is_home = true;
        self.can_enter = true;
        self
    }

    pub fn accessible(mut self) -> Self {
        self.can_enter = true;
        self
    }
}

// =============================================================================
// Adapters from aura_app types
// =============================================================================

impl From<&AppContact> for Contact {
    fn from(c: &AppContact) -> Self {
        // Use suggested_name as default nickname if nickname is empty
        let nickname = if !c.nickname.is_empty() {
            c.nickname.clone()
        } else if let Some(suggested) = &c.suggested_name {
            suggested.clone()
        } else {
            String::new()
        };

        Self {
            id: c.id.to_string(),
            nickname,
            suggested_name: c.suggested_name.clone(),
            status: if c.is_online {
                ContactStatus::Active
            } else {
                ContactStatus::Offline
            },
            is_guardian: c.is_guardian,
        }
    }
}

// =============================================================================
// Conflict Resolution Types
// =============================================================================

/// Type of operation that was conflicted
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConflictOperationType {
    /// Channel permission change
    #[default]
    PermissionChange,
    /// Member removal (kick)
    MemberRemoval,
    /// Channel rename
    ChannelRename,
    /// Topic change
    TopicChange,
    /// Ownership transfer
    OwnershipTransfer,
    /// Channel mode change
    ModeChange,
}

impl ConflictOperationType {
    /// Human-readable label for the operation type
    pub fn label(self) -> &'static str {
        match self {
            Self::PermissionChange => "Permission change",
            Self::MemberRemoval => "Member removal",
            Self::ChannelRename => "Channel rename",
            Self::TopicChange => "Topic change",
            Self::OwnershipTransfer => "Ownership transfer",
            Self::ModeChange => "Mode change",
        }
    }

    /// Icon for the operation type
    pub fn icon(self) -> &'static str {
        match self {
            Self::PermissionChange => "🔒",
            Self::MemberRemoval => "👤",
            Self::ChannelRename => "✏️",
            Self::TopicChange => "📝",
            Self::OwnershipTransfer => "👑",
            Self::ModeChange => "⚙️",
        }
    }
}

/// Resolution action for a conflict
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConflictResolution {
    /// Keep the local version (what we attempted)
    #[default]
    KeepLocal,
    /// Accept the remote version (what others did)
    AcceptRemote,
    /// Merge both changes if possible
    Merge,
    /// Discard both and revert to previous state
    Revert,
}

impl ConflictResolution {
    pub fn label(self) -> &'static str {
        match self {
            Self::KeepLocal => "Keep mine",
            Self::AcceptRemote => "Accept theirs",
            Self::Merge => "Merge both",
            Self::Revert => "Revert all",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::KeepLocal => "Apply your change, overwriting the other",
            Self::AcceptRemote => "Accept their change, discard yours",
            Self::Merge => "Try to combine both changes",
            Self::Revert => "Undo both changes, restore previous state",
        }
    }
}

/// A detected operation conflict
///
/// Represents a situation where an optimistic operation was rolled back
/// because another admin performed a conflicting operation.
#[derive(Clone, Debug, Default)]
pub struct OperationConflict {
    /// Unique identifier for this conflict
    pub id: String,
    /// Type of operation that conflicted
    pub operation_type: ConflictOperationType,
    /// Description of what we tried to do
    pub local_action: String,
    /// Description of what others did
    pub remote_action: String,
    /// Who performed the conflicting remote action
    pub remote_actor: String,
    /// When the conflict was detected (ms since epoch)
    pub detected_at: u64,
    /// Available resolution options
    pub available_resolutions: Vec<ConflictResolution>,
    /// Currently selected resolution (if any)
    pub selected_resolution: Option<ConflictResolution>,
    /// Whether the conflict has been resolved
    pub resolved: bool,
    /// Target entity (channel, member, etc.)
    pub target_name: String,
}

impl OperationConflict {
    pub fn new(id: impl Into<String>, operation_type: ConflictOperationType) -> Self {
        Self {
            id: id.into(),
            operation_type,
            available_resolutions: vec![
                ConflictResolution::KeepLocal,
                ConflictResolution::AcceptRemote,
                ConflictResolution::Revert,
            ],
            ..Default::default()
        }
    }

    pub fn with_local_action(mut self, action: impl Into<String>) -> Self {
        self.local_action = action.into();
        self
    }

    pub fn with_remote_action(
        mut self,
        action: impl Into<String>,
        actor: impl Into<String>,
    ) -> Self {
        self.remote_action = action.into();
        self.remote_actor = actor.into();
        self
    }

    pub fn with_target(mut self, name: impl Into<String>) -> Self {
        self.target_name = name.into();
        self
    }

    pub fn with_resolutions(mut self, resolutions: Vec<ConflictResolution>) -> Self {
        self.available_resolutions = resolutions;
        self
    }

    /// Generate a human-readable summary of the conflict
    pub fn summary(&self) -> String {
        format!(
            "{} on #{}: {} vs {}",
            self.operation_type.label(),
            self.target_name,
            self.local_action,
            self.remote_action
        )
    }

    /// Generate a notification message for the conflict
    pub fn notification_message(&self) -> String {
        format!(
            "{} was rolled back - {} already {} on #{}",
            self.operation_type.label(),
            self.remote_actor,
            self.remote_action,
            self.target_name
        )
    }
}

/// Confirmation status for optimistic operations
///
/// Tracks whether an optimistic operation has been confirmed
/// by the required parties in a distributed ceremony.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ConfirmationStatus {
    /// Applied locally, not yet sent for confirmation
    #[default]
    LocalOnly,
    /// Background confirmation ceremony in progress
    Confirming {
        /// How many parties have confirmed
        confirmed_count: u32,
        /// How many parties are required
        required_count: u32,
    },
    /// All required parties confirmed
    Confirmed,
    /// Some parties confirmed, others declined/unavailable
    PartiallyConfirmed {
        /// How many confirmed
        confirmed_count: u32,
        /// How many declined
        declined_count: u32,
    },
    /// Confirmation failed - operation may need rollback
    Unconfirmed {
        /// Reason for failure
        reason: String,
    },
    /// Operation was rolled back due to conflict
    RolledBack {
        /// Associated conflict (if any)
        conflict_id: Option<String>,
    },
}

impl ConfirmationStatus {
    /// Status indicator icon
    pub fn indicator(&self) -> &'static str {
        match self {
            Self::LocalOnly => "○",                 // Empty circle
            Self::Confirming { .. } => "◐",         // Half-filled
            Self::Confirmed => "●",                 // Filled circle
            Self::PartiallyConfirmed { .. } => "◑", // Partial
            Self::Unconfirmed { .. } => "⊗",        // Circle with X
            Self::RolledBack { .. } => "⇄",         // Arrows (conflict)
        }
    }

    /// Human-readable status label
    pub fn label(&self) -> String {
        match self {
            Self::LocalOnly => "Local only".to_string(),
            Self::Confirming {
                confirmed_count,
                required_count,
            } => {
                format!("Confirming ({}/{})", confirmed_count, required_count)
            }
            Self::Confirmed => "Confirmed".to_string(),
            Self::PartiallyConfirmed {
                confirmed_count,
                declined_count,
            } => {
                format!(
                    "Partial ({} confirmed, {} declined)",
                    confirmed_count, declined_count
                )
            }
            Self::Unconfirmed { reason } => format!("Unconfirmed: {}", reason),
            Self::RolledBack { .. } => "Rolled back".to_string(),
        }
    }

    /// Whether the operation is fully confirmed
    pub fn is_confirmed(&self) -> bool {
        matches!(self, Self::Confirmed)
    }

    /// Whether the operation is still pending confirmation
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::LocalOnly | Self::Confirming { .. })
    }

    /// Whether the operation failed or was rolled back
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Unconfirmed { .. } | Self::RolledBack { .. })
    }
}
