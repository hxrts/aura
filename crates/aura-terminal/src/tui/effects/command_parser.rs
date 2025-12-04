//! # Command Parser
//!
//! Defines effect commands, events, and their authorization requirements.
//!
//! This module contains:
//! - `EffectCommand`: Enum of all commands that can be dispatched to the effect system
//! - `CommandAuthorizationLevel`: Authorization level classification for commands
//! - `AuraEvent`: Events emitted by the effect system for TUI consumption
//! - `EventFilter`: Filter for subscribing to specific event types
//! - `EventSubscription`: Subscription handle for receiving filtered events
//!
//! Commands are organized by functional area (Recovery, Account, Chat, etc.) and
//! classified by the authorization level required to execute them.

use crate::tui::reactive::queries::Channel;
use tokio::sync::broadcast;

/// Authorization level required for a command
///
/// Commands are classified by sensitivity level, with each level
/// requiring progressively stronger authorization:
/// - **Public**: No authorization required (read-only, status queries)
/// - **Basic**: User token required (normal user operations)
/// - **Sensitive**: Elevated authorization (account modifications)
/// - **Admin**: Steward/admin capabilities (privileged operations)
///
/// Levels are ordered: Public < Basic < Sensitive < Admin
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommandAuthorizationLevel {
    /// No authorization required - read-only/status operations
    Public,
    /// Basic user token required - normal messaging and channels
    Basic,
    /// Elevated authorization - account/device modifications
    Sensitive,
    /// Admin/steward capabilities - moderation and privileged ops
    Admin,
}

impl CommandAuthorizationLevel {
    /// Check if this level requires any authorization
    pub fn requires_auth(&self) -> bool {
        !matches!(self, Self::Public)
    }

    /// Get human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            Self::Public => "public access",
            Self::Basic => "user authentication",
            Self::Sensitive => "elevated authorization",
            Self::Admin => "administrator privileges",
        }
    }
}

/// Commands that can be dispatched to the effect system
#[derive(Debug, Clone)]
pub enum EffectCommand {
    // === Recovery Commands ===
    /// Initiate recovery process
    StartRecovery,
    /// Submit guardian approval
    SubmitGuardianApproval {
        /// Guardian ID
        guardian_id: String,
    },
    /// Complete recovery after threshold met
    CompleteRecovery,
    /// Cancel ongoing recovery
    CancelRecovery,

    // === Account Commands ===
    /// Refresh account status
    RefreshAccount,
    /// Create new account (threshold configuration is done later in Settings)
    CreateAccount {
        /// Display name
        display_name: String,
    },

    // === Settings Commands ===
    /// Update threshold configuration (requires guardian setup)
    UpdateThreshold {
        /// Required signatures (K)
        threshold_k: u8,
        /// Total guardians (N)
        threshold_n: u8,
    },
    /// Add a device to the account
    AddDevice {
        /// Device name/identifier
        device_name: String,
    },
    /// Remove a device from the account
    RemoveDevice {
        /// Device ID to remove
        device_id: String,
    },
    /// Update multifactor policy
    UpdateMfaPolicy {
        /// Require MFA for sensitive operations
        require_mfa: bool,
    },

    // === Block Commands ===
    /// Set the active AMP context for channel operations
    SetContext {
        /// Context ID (UUID string or named context like "default", "dm")
        context_id: String,
    },
    /// Create a new block
    CreateBlock {
        /// Optional block name
        name: Option<String>,
    },
    /// Accept pending block invitation
    AcceptPendingBlockInvitation,
    /// Send block invitation to contact
    SendBlockInvitation {
        /// Contact ID to invite
        contact_id: String,
    },

    // === Chat Commands ===
    /// Send a message
    SendMessage {
        /// Channel ID
        channel: String,

        /// Message content
        content: String,
    },
    /// Create a new channel (AMP substream)
    CreateChannel {
        /// Human-friendly name
        name: String,
        /// Optional topic/description
        topic: Option<String>,
        /// Member authority IDs to add
        members: Vec<String>,
    },
    /// Close/archive a channel
    CloseChannel {
        /// Channel ID
        channel: String,
    },
    /// Send a direct message to a user
    SendDirectMessage {
        /// Target user
        target: String,

        /// Message content
        content: String,
    },
    /// Send an action/emote
    SendAction {
        /// Channel ID
        channel: String,

        /// Action text
        action: String,
    },
    /// Join a channel
    JoinChannel {
        /// Channel ID
        channel: String,
    },
    /// Leave a channel
    LeaveChannel {
        /// Channel ID
        channel: String,
    },
    /// Update contact suggestion (nickname)
    UpdateNickname {
        /// New nickname
        name: String,
    },
    /// List participants in channel
    ListParticipants {
        /// Channel ID
        channel: String,
    },
    /// Get user information
    GetUserInfo {
        /// Target user
        target: String,
    },
    /// Kick user from channel
    KickUser {
        /// Channel ID
        channel: String,

        /// Target user
        target: String,

        /// Optional reason
        reason: Option<String>,
    },
    /// Ban user from block
    BanUser {
        /// Target user
        target: String,

        /// Optional reason
        reason: Option<String>,
    },
    /// Unban user from block
    UnbanUser {
        /// Target user
        target: String,
    },
    /// Mute user temporarily
    MuteUser {
        /// Target user
        target: String,

        /// Duration in seconds (None = indefinite)
        duration_secs: Option<u64>,
    },
    /// Unmute user
    UnmuteUser {
        /// Target user
        target: String,
    },
    /// Invite user to channel/block
    InviteUser {
        /// Target user
        target: String,
    },

    // === Contact Commands ===
    /// Update a contact's petname
    UpdateContactPetname {
        /// Contact ID
        contact_id: String,
        /// New petname
        petname: String,
    },
    /// Toggle guardian status for a contact
    ToggleContactGuardian {
        /// Contact ID
        contact_id: String,
    },
    /// Invite a contact to become a guardian
    InviteGuardian {
        /// Contact ID (optional - if None, opens selection modal)
        contact_id: Option<String>,
    },

    // === Invitation Commands ===
    /// Accept an invitation
    AcceptInvitation {
        /// Invitation ID
        invitation_id: String,
    },
    /// Decline an invitation
    DeclineInvitation {
        /// Invitation ID
        invitation_id: String,
    },
    /// Set channel topic
    SetTopic {
        /// Channel ID
        channel: String,

        /// Topic text
        text: String,
    },
    /// Pin message
    PinMessage {
        /// Message ID
        message_id: String,
    },
    /// Unpin message
    UnpinMessage {
        /// Message ID
        message_id: String,
    },
    /// Grant steward capabilities
    GrantSteward {
        /// Target user
        target: String,
    },
    /// Revoke steward capabilities
    RevokeSteward {
        /// Target user
        target: String,
    },
    /// Set channel mode
    SetChannelMode {
        /// Channel ID
        channel: String,

        /// Mode flags
        flags: String,
    },

    // === Sync Commands ===
    /// Force sync with peers
    ForceSync,
    /// Request state from specific peer
    RequestState {
        /// Peer ID
        peer_id: String,
    },
    /// Add a peer to the known peers list
    AddPeer {
        /// Peer ID (UUID string)
        peer_id: String,
    },
    /// Remove a peer from the known peers list
    RemovePeer {
        /// Peer ID (UUID string)
        peer_id: String,
    },
    /// List known peers
    ListPeers,

    // === Neighborhood Traversal Commands ===
    /// Move to adjacent position in neighborhood
    MovePosition {
        /// Target neighborhood ID
        neighborhood_id: String,
        /// Target block ID
        block_id: String,
        /// Traversal depth (Street, Frontage, Interior)
        depth: String,
    },

    // === General Commands ===
    /// Ping/health check
    Ping,
    /// Shutdown the bridge
    Shutdown,
}

impl EffectCommand {
    /// Get the authorization level required for this command
    ///
    /// Commands are classified into four levels:
    /// - **Public**: No auth needed (RefreshAccount, ForceSync, Ping, etc.)
    /// - **Basic**: User token (SendMessage, CreateChannel, etc.)
    /// - **Sensitive**: Elevated auth (UpdateThreshold, AddDevice, StartRecovery, etc.)
    /// - **Admin**: Steward privileges (KickUser, BanUser, GrantSteward, etc.)
    pub fn authorization_level(&self) -> CommandAuthorizationLevel {
        match self {
            // Public - no authorization required
            Self::RefreshAccount
            | Self::ForceSync
            | Self::RequestState { .. }
            | Self::AddPeer { .. }
            | Self::RemovePeer { .. }
            | Self::ListPeers
            | Self::Ping
            | Self::ListParticipants { .. }
            | Self::GetUserInfo { .. } => CommandAuthorizationLevel::Public,

            // Basic - user token required
            Self::SendMessage { .. }
            | Self::SendDirectMessage { .. }
            | Self::SendAction { .. }
            | Self::CreateChannel { .. }
            | Self::CloseChannel { .. }
            | Self::JoinChannel { .. }
            | Self::LeaveChannel { .. }
            | Self::UpdateNickname { .. }
            | Self::UpdateContactPetname { .. }
            | Self::SetTopic { .. }
            | Self::PinMessage { .. }
            | Self::UnpinMessage { .. }
            | Self::MovePosition { .. }
            | Self::InviteUser { .. }
            | Self::AcceptInvitation { .. }
            | Self::DeclineInvitation { .. }
            | Self::AcceptPendingBlockInvitation
            | Self::SendBlockInvitation { .. }
            | Self::SetContext { .. } => CommandAuthorizationLevel::Basic,

            // Sensitive - elevated authorization
            Self::CreateAccount { .. }
            | Self::CreateBlock { .. }
            | Self::UpdateThreshold { .. }
            | Self::AddDevice { .. }
            | Self::RemoveDevice { .. }
            | Self::UpdateMfaPolicy { .. }
            | Self::StartRecovery
            | Self::SubmitGuardianApproval { .. }
            | Self::CompleteRecovery
            | Self::CancelRecovery
            | Self::ToggleContactGuardian { .. }
            | Self::InviteGuardian { .. }
            | Self::MuteUser { .. }
            | Self::UnmuteUser { .. } => CommandAuthorizationLevel::Sensitive,

            // Admin - steward/admin capabilities
            Self::KickUser { .. }
            | Self::BanUser { .. }
            | Self::UnbanUser { .. }
            | Self::GrantSteward { .. }
            | Self::RevokeSteward { .. }
            | Self::SetChannelMode { .. }
            | Self::Shutdown => CommandAuthorizationLevel::Admin,
        }
    }
}

/// Events emitted by the effect system for TUI consumption
#[derive(Debug, Clone)]
pub enum AuraEvent {
    // === Connection Events ===
    /// Connected to the network
    Connected,
    /// Disconnected from the network
    Disconnected {
        /// Disconnection reason
        reason: String,
    },
    /// Reconnecting after failure
    Reconnecting {
        /// Current attempt number
        attempt: u32,

        /// Maximum attempts
        max_attempts: u32,
    },

    // === Recovery Events ===
    /// Recovery process started
    RecoveryStarted {
        /// Session ID
        session_id: String,
    },
    /// Guardian approved recovery
    GuardianApproved {
        /// Guardian ID
        guardian_id: String,
        /// Current approval count
        current: u32,
        /// Required threshold
        threshold: u32,
    },
    /// Recovery threshold met
    ThresholdMet {
        /// Session ID
        session_id: String,
    },
    /// Recovery completed successfully
    RecoveryCompleted {
        /// Session ID
        session_id: String,
    },
    /// Recovery failed
    RecoveryFailed {
        /// Session ID
        session_id: String,

        /// Failure reason
        reason: String,
    },
    /// Recovery cancelled
    RecoveryCancelled {
        /// Session ID
        session_id: String,
    },

    // === Account Events ===
    /// Account state updated
    AccountUpdated {
        /// Authority ID
        authority_id: String,
    },
    /// New device added to account
    DeviceAdded {
        /// Device ID
        device_id: String,
    },
    /// Device removed from account
    DeviceRemoved {
        /// Device ID
        device_id: String,
    },

    // === Chat Events ===
    /// New message received
    MessageReceived {
        /// Channel ID
        channel: String,

        /// Sender ID
        from: String,

        /// Message content
        content: String,

        /// Timestamp
        timestamp: u64,
    },
    /// User joined channel
    UserJoined {
        /// Channel ID
        channel: String,

        /// User ID
        user: String,
    },
    /// User left channel
    UserLeft {
        /// Channel ID
        channel: String,

        /// User ID
        user: String,
    },
    /// Channel created
    ChannelCreated {
        /// Created channel
        channel: Channel,
    },
    /// Channel closed/archived
    ChannelClosed {
        /// Channel ID
        channel_id: String,
    },

    // === Sync Events ===
    /// Sync started
    SyncStarted {
        /// Peer ID
        peer_id: String,
    },
    /// Sync completed
    SyncCompleted {
        /// Peer ID
        peer_id: String,

        /// Number of changes synced
        changes: u32,
    },
    /// Sync failed
    SyncFailed {
        /// Peer ID
        peer_id: String,

        /// Failure reason
        reason: String,
    },
    /// Peer added to known peers list
    PeerAdded {
        /// Peer ID
        peer_id: String,
    },
    /// Peer removed from known peers list
    PeerRemoved {
        /// Peer ID
        peer_id: String,
    },
    /// List of known peers
    PeersListed {
        /// Known peer IDs
        peers: Vec<String>,
    },

    // === Block Events ===
    /// Block created
    BlockCreated {
        /// Block ID
        block_id: String,
        /// Block name
        name: Option<String>,
    },
    /// Joined a block
    BlockJoined {
        /// Block ID
        block_id: String,
    },

    // === Invitation Events ===
    /// Invitation accepted
    InvitationAccepted {
        /// Invitation ID
        invitation_id: String,
    },
    /// Invitation declined
    InvitationDeclined {
        /// Invitation ID
        invitation_id: String,
    },
    /// Invitation sent
    InvitationSent {
        /// Invitation ID
        invitation_id: String,
        /// Recipient ID
        recipient: String,
    },

    // === Settings Events ===
    /// Threshold updated
    ThresholdUpdated {
        /// New K value (required signatures)
        threshold_k: u8,
        /// New N value (total signers)
        threshold_n: u8,
    },
    /// MFA policy updated
    MfaPolicyUpdated {
        /// Whether MFA is required
        require_mfa: bool,
    },

    // === Moderation Events ===
    /// Participants list retrieved
    ParticipantsList {
        /// Channel ID
        channel: String,
        /// List of participant IDs
        participants: Vec<String>,
        /// Number of participants
        count: usize,
    },
    /// User information retrieved
    UserInfo {
        /// User ID
        user_id: String,
        /// Display name
        name: String,
        /// Whether user is a steward
        is_steward: bool,
        /// When user joined (ms since epoch)
        joined_at: u64,
        /// Storage allocated to user
        storage_allocated: u64,
    },
    /// User kicked from channel
    UserKicked {
        /// Channel ID
        channel: String,
        /// Target user ID
        target: String,
        /// Actor who performed the kick
        actor: String,
        /// Optional reason
        reason: Option<String>,
    },
    /// User banned from block
    UserBanned {
        /// Target user ID
        target: String,
        /// Actor who performed the ban
        actor: String,
        /// Optional reason
        reason: Option<String>,
    },
    /// User unbanned from block
    UserUnbanned {
        /// Target user ID
        target: String,
        /// Actor who performed the unban
        actor: String,
    },
    /// User muted
    UserMuted {
        /// Target user ID
        target: String,
        /// Actor who performed the mute
        actor: String,
        /// Duration in seconds (None = indefinite)
        duration_secs: Option<u64>,
    },
    /// User unmuted
    UserUnmuted {
        /// Target user ID
        target: String,
        /// Actor who performed the unmute
        actor: String,
    },
    /// User invited to block/channel
    UserInvited {
        /// Target user ID
        target: String,
        /// Actor who sent the invitation
        actor: String,
    },
    /// Steward role granted
    StewardGranted {
        /// Target user ID
        target: String,
        /// Actor who granted the role
        actor: String,
        /// Block ID where role was granted
        block_id: String,
    },
    /// Steward role revoked
    StewardRevoked {
        /// Target user ID
        target: String,
        /// Actor who revoked the role
        actor: String,
        /// Block ID where role was revoked
        block_id: String,
    },

    // === Contacts & Petnames Events ===
    /// User nickname updated
    NicknameUpdated {
        /// New nickname
        nickname: String,
    },
    /// Contact petname updated
    ContactPetnameUpdated {
        /// Contact ID
        contact_id: String,
        /// New petname
        petname: String,
    },
    /// Contact guardian status toggled
    ContactGuardianToggled {
        /// Contact ID
        contact_id: String,
        /// New guardian status
        is_guardian: bool,
    },
    /// Guardian invitation sent
    GuardianInvitationSent {
        /// Invitation ID
        invitation_id: String,
        /// Contact ID (if specified)
        contact_id: Option<String>,
    },

    // === Neighborhood Traversal Events ===
    /// Position updated in neighborhood
    PositionUpdated {
        /// Neighborhood ID
        neighborhood_id: String,
        /// Block ID
        block_id: String,
        /// Traversal depth
        depth: String,
    },

    // === Channel Management Events ===
    /// Channel topic set
    TopicSet {
        /// Channel/block ID
        channel: String,
        /// Topic text
        text: String,
        /// Actor who set the topic
        actor: String,
    },
    /// Message pinned
    MessagePinned {
        /// Message ID
        message_id: String,
        /// Channel/block ID where message was pinned
        channel: String,
        /// Actor who pinned the message
        actor: String,
    },
    /// Message unpinned
    MessageUnpinned {
        /// Message ID
        message_id: String,
        /// Channel/block ID where message was unpinned
        channel: String,
        /// Actor who unpinned the message
        actor: String,
    },
    /// Channel mode set
    ChannelModeSet {
        /// Channel/block ID
        channel: String,
        /// Mode flags
        flags: String,
        /// Actor who set the mode
        actor: String,
    },

    // === Authorization Events ===
    /// Command authorization denied
    AuthorizationDenied {
        /// The command that was denied
        command: String,
        /// Required authorization level
        required_level: CommandAuthorizationLevel,
        /// Reason for denial
        reason: String,
    },

    // === Error Events ===
    /// General error occurred
    Error {
        /// Error code
        code: String,

        /// Error message
        message: String,
    },
    /// Warning (non-fatal)
    Warning {
        /// Warning message
        message: String,
    },

    // === System Events ===
    /// Pong response to ping
    Pong {
        /// Latency in milliseconds
        latency_ms: u64,
    },
    /// Bridge shutting down
    ShuttingDown,
}

/// Filter for subscribing to specific event types
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    /// Include connection events
    pub connection: bool,
    /// Include recovery events
    pub recovery: bool,
    /// Include account events
    pub account: bool,
    /// Include chat events
    pub chat: bool,
    /// Include sync events
    pub sync: bool,
    /// Include block events
    pub block: bool,
    /// Include invitation events
    pub invitation: bool,
    /// Include settings events
    pub settings: bool,
    /// Include moderation events
    pub moderation: bool,
    /// Include authorization events
    pub authorization: bool,
    /// Include error events
    pub errors: bool,
    /// Include system events
    pub system: bool,
}

impl EventFilter {
    /// Create a filter that accepts all events
    pub fn all() -> Self {
        Self {
            connection: true,
            recovery: true,
            account: true,
            chat: true,
            sync: true,
            block: true,
            invitation: true,
            settings: true,
            moderation: true,
            authorization: true,
            errors: true,
            system: true,
        }
    }

    /// Create a filter for connection and error events only
    pub fn essential() -> Self {
        Self {
            connection: true,
            errors: true,
            system: true,
            ..Default::default()
        }
    }

    /// Create a filter for recovery-related events
    pub fn recovery() -> Self {
        Self {
            recovery: true,
            errors: true,
            ..Default::default()
        }
    }

    /// Check if an event matches this filter
    pub fn matches(&self, event: &AuraEvent) -> bool {
        match event {
            AuraEvent::Connected
            | AuraEvent::Disconnected { .. }
            | AuraEvent::Reconnecting { .. } => self.connection,
            AuraEvent::RecoveryStarted { .. }
            | AuraEvent::GuardianApproved { .. }
            | AuraEvent::ThresholdMet { .. }
            | AuraEvent::RecoveryCompleted { .. }
            | AuraEvent::RecoveryFailed { .. }
            | AuraEvent::RecoveryCancelled { .. } => self.recovery,
            AuraEvent::AccountUpdated { .. }
            | AuraEvent::DeviceAdded { .. }
            | AuraEvent::DeviceRemoved { .. }
            | AuraEvent::NicknameUpdated { .. }
            | AuraEvent::PositionUpdated { .. }
            | AuraEvent::ContactPetnameUpdated { .. }
            | AuraEvent::ContactGuardianToggled { .. }
            | AuraEvent::GuardianInvitationSent { .. } => self.account,
            AuraEvent::MessageReceived { .. }
            | AuraEvent::UserJoined { .. }
            | AuraEvent::UserLeft { .. }
            | AuraEvent::ChannelCreated { .. }
            | AuraEvent::ChannelClosed { .. } => self.chat,
            AuraEvent::SyncStarted { .. }
            | AuraEvent::SyncCompleted { .. }
            | AuraEvent::SyncFailed { .. }
            | AuraEvent::PeerAdded { .. }
            | AuraEvent::PeerRemoved { .. }
            | AuraEvent::PeersListed { .. } => self.sync,
            AuraEvent::BlockCreated { .. } | AuraEvent::BlockJoined { .. } => self.block,
            AuraEvent::InvitationAccepted { .. }
            | AuraEvent::InvitationDeclined { .. }
            | AuraEvent::InvitationSent { .. } => self.invitation,
            AuraEvent::ThresholdUpdated { .. } | AuraEvent::MfaPolicyUpdated { .. } => {
                self.settings
            }
            AuraEvent::ParticipantsList { .. }
            | AuraEvent::UserInfo { .. }
            | AuraEvent::UserKicked { .. }
            | AuraEvent::UserBanned { .. }
            | AuraEvent::UserUnbanned { .. }
            | AuraEvent::UserMuted { .. }
            | AuraEvent::UserUnmuted { .. }
            | AuraEvent::UserInvited { .. }
            | AuraEvent::StewardGranted { .. }
            | AuraEvent::StewardRevoked { .. }
            | AuraEvent::TopicSet { .. }
            | AuraEvent::MessagePinned { .. }
            | AuraEvent::MessageUnpinned { .. }
            | AuraEvent::ChannelModeSet { .. } => self.moderation,
            AuraEvent::AuthorizationDenied { .. } => self.authorization,
            AuraEvent::Error { .. } | AuraEvent::Warning { .. } => self.errors,
            AuraEvent::Pong { .. } | AuraEvent::ShuttingDown => self.system,
        }
    }
}

/// Subscription handle for receiving filtered events
pub struct EventSubscription {
    pub(crate) receiver: broadcast::Receiver<AuraEvent>,
    pub(crate) filter: EventFilter,
}

impl EventSubscription {
    /// Create a new event subscription
    pub fn new(receiver: broadcast::Receiver<AuraEvent>, filter: EventFilter) -> Self {
        Self { receiver, filter }
    }
}

impl EventSubscription {
    /// Receive the next event that matches the filter
    pub async fn recv(&mut self) -> Option<AuraEvent> {
        loop {
            match self.receiver.recv().await {
                Ok(event) if self.filter.matches(&event) => return Some(event),
                Ok(_) => continue, // Skip non-matching events
                Err(broadcast::error::RecvError::Closed) => return None,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    }

    /// Try to receive an event without blocking
    pub fn try_recv(&mut self) -> Option<AuraEvent> {
        loop {
            match self.receiver.try_recv() {
                Ok(event) if self.filter.matches(&event) => return Some(event),
                Ok(_) => continue,
                Err(_) => return None,
            }
        }
    }
}
