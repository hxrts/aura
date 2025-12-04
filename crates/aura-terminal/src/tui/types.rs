//! # Shared Types
//!
//! Domain types used across iocraft components.
//! These use owned types (String, Vec) for compatibility with iocraft's 'static lifetime requirements.

// Re-export source types for adapters
use aura_app::{Channel as AppChannel, Message as AppMessage};

use crate::tui::reactive::{queries, views};

/// A chat channel
#[derive(Clone, Debug, Default)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub unread_count: usize,
    pub is_selected: bool,
}

impl From<&AppChannel> for Channel {
    fn from(ch: &AppChannel) -> Self {
        Self {
            id: ch.id.clone(),
            name: ch.name.clone(),
            unread_count: ch.unread_count as usize,
            is_selected: false,
        }
    }
}

impl Channel {
    /// Create from aura_app Channel with selection state
    pub fn from_app(ch: &AppChannel, is_selected: bool) -> Self {
        Self {
            id: ch.id.clone(),
            name: ch.name.clone(),
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
            unread_count: 0,
            is_selected: false,
        }
    }

    pub fn with_unread(mut self, count: usize) -> Self {
        self.unread_count = count;
        self
    }

    pub fn selected(mut self, is_selected: bool) -> Self {
        self.is_selected = is_selected;
        self
    }
}

/// A chat message
#[derive(Clone, Debug, Default)]
pub struct Message {
    pub id: String,
    pub sender: String,
    pub content: String,
    pub timestamp: String,
    pub is_own: bool,
}

impl From<&AppMessage> for Message {
    fn from(msg: &AppMessage) -> Self {
        Self {
            id: msg.id.clone(),
            sender: msg.sender_name.clone(),
            content: msg.content.clone(),
            timestamp: format_timestamp(msg.timestamp),
            is_own: msg.is_own,
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
            sender: sender.into(),
            content: content.into(),
            timestamp: String::new(),
            is_own: false,
        }
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

impl From<queries::InvitationDirection> for InvitationDirection {
    fn from(d: queries::InvitationDirection) -> Self {
        match d {
            queries::InvitationDirection::Outbound => Self::Outbound,
            queries::InvitationDirection::Inbound => Self::Inbound,
        }
    }
}

impl From<queries::InvitationStatus> for InvitationStatus {
    fn from(s: queries::InvitationStatus) -> Self {
        match s {
            queries::InvitationStatus::Pending => Self::Pending,
            queries::InvitationStatus::Accepted => Self::Accepted,
            queries::InvitationStatus::Declined => Self::Declined,
            queries::InvitationStatus::Expired => Self::Expired,
            queries::InvitationStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<queries::InvitationType> for InvitationType {
    fn from(t: queries::InvitationType) -> Self {
        match t {
            queries::InvitationType::Guardian => Self::Guardian,
            queries::InvitationType::Channel => Self::Channel,
            queries::InvitationType::Contact => Self::Contact,
        }
    }
}

impl From<&queries::Invitation> for Invitation {
    fn from(inv: &queries::Invitation) -> Self {
        Self {
            id: inv.id.clone(),
            direction: inv.direction.into(),
            other_party_id: inv.other_party_id.clone(),
            other_party_name: inv.other_party_name.clone(),
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

/// Recovery screen tab
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecoveryTab {
    #[default]
    Guardians,
    Recovery,
}

impl RecoveryTab {
    pub fn title(self) -> &'static str {
        match self {
            Self::Guardians => "Guardians",
            Self::Recovery => "Recovery",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Guardians => Self::Recovery,
            Self::Recovery => Self::Guardians,
        }
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
            Self::Pending => "○",
            Self::Offline => "◌",
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
}

/// A guardian
#[derive(Clone, Debug, Default)]
pub struct Guardian {
    pub id: String,
    pub name: String,
    pub status: GuardianStatus,
    pub has_share: bool,
}

impl From<queries::GuardianStatus> for GuardianStatus {
    fn from(status: queries::GuardianStatus) -> Self {
        match status {
            queries::GuardianStatus::Active => Self::Active,
            queries::GuardianStatus::Pending => Self::Pending,
            queries::GuardianStatus::Offline => Self::Offline,
            queries::GuardianStatus::Declined => Self::Declined,
            queries::GuardianStatus::Removed => Self::Removed,
        }
    }
}

impl From<&queries::Guardian> for Guardian {
    fn from(g: &queries::Guardian) -> Self {
        Self {
            id: g.authority_id.clone(),
            name: g.name.clone(),
            status: g.status.into(),
            has_share: g.share_index.is_some(),
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

impl From<queries::RecoveryState> for RecoveryState {
    fn from(state: queries::RecoveryState) -> Self {
        match state {
            queries::RecoveryState::None => Self::None,
            queries::RecoveryState::Initiated => Self::Initiated,
            queries::RecoveryState::ThresholdMet => Self::ThresholdMet,
            queries::RecoveryState::InProgress => Self::InProgress,
            queries::RecoveryState::Completed => Self::Completed,
            queries::RecoveryState::Failed => Self::Failed,
            queries::RecoveryState::Cancelled => Self::Cancelled,
        }
    }
}

impl From<&queries::GuardianApproval> for GuardianApproval {
    fn from(a: &queries::GuardianApproval) -> Self {
        Self {
            guardian_name: a.guardian_name.clone(),
            approved: a.approved,
        }
    }
}

impl From<&queries::RecoveryStatus> for RecoveryStatus {
    fn from(rs: &queries::RecoveryStatus) -> Self {
        Self {
            state: rs.state.into(),
            approvals_received: rs.approvals_received,
            threshold: rs.threshold,
            approvals: rs.approvals.iter().map(|a| a.into()).collect(),
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
    Pending,
    Blocked,
}

impl ContactStatus {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Active => "●",
            Self::Pending => "○",
            Self::Blocked => "⊗",
        }
    }
}

/// A contact
#[derive(Clone, Debug, Default)]
pub struct Contact {
    pub id: String,
    pub petname: String,
    pub suggested_name: Option<String>,
    pub status: ContactStatus,
    pub is_guardian: bool,
}

impl Contact {
    pub fn new(id: impl Into<String>, petname: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            petname: petname.into(),
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
// Adapters from views:: types
// =============================================================================

impl From<&views::Contact> for Contact {
    fn from(c: &views::Contact) -> Self {
        Self {
            id: c.authority_id.clone(),
            petname: c.petname.clone(),
            suggested_name: c.suggested_name.clone(),
            status: if c.is_online.unwrap_or(false) {
                ContactStatus::Active
            } else {
                ContactStatus::Pending
            },
            is_guardian: false, // Would need to cross-reference with guardians
        }
    }
}

impl From<&views::Resident> for Resident {
    fn from(r: &views::Resident) -> Self {
        Self {
            id: r.authority_id.clone(),
            name: r.name.clone(),
            is_steward: matches!(r.role, views::ResidentRole::Steward),
            is_self: r.is_self,
        }
    }
}

impl From<&views::StorageInfo> for BlockBudget {
    fn from(s: &views::StorageInfo) -> Self {
        Self {
            total: s.total_bytes,
            used: s.used_bytes,
            resident_count: 0, // Not part of StorageInfo
            max_residents: 8,  // Default
        }
    }
}
