//! # Command Parser
//!
//! Defines effect commands, events, and their authorization requirements.
//!
//! This module contains:
//! - `EffectCommand`: Enum of all commands that can be dispatched to the effect system
//! - `CommandAuthorizationLevel`: Authorization level classification for commands (re-exported from aura-app)
//! - `AuraEvent`: Events emitted by the effect system for TUI consumption
//! - `EventFilter`: Filter for subscribing to specific event types
//! - `EventSubscription`: Subscription handle for receiving filtered events
//!
//! Commands are organized by functional area (Recovery, Account, Chat, etc.) and
//! classified by the authorization level required to execute them.

use aura_app::ui::types::chat::Channel;
use tokio::sync::broadcast;

// Re-export portable authorization types from aura-app
pub use aura_app::ui::authorization::CommandAuthorizationLevel;

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

    // === Home Commands ===
    /// Set the active AMP context for channel operations
    SetContext {
        /// Context ID (UUID string or named context like "default", "dm")
        context_id: String,
    },
    /// Create a new home
    CreateHome {
        /// Optional home name
        name: Option<String>,
    },
    /// Accept pending home invitation
    AcceptPendingHomeInvitation,
    /// Send home invitation to contact
    SendHomeInvitation {
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
        /// Optional threshold k (m-of-n)
        threshold_k: u8,
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
    /// Start or navigate to a direct chat with a contact
    StartDirectChat {
        /// Contact ID to chat with
        contact_id: String,
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
    /// Retry sending a failed message
    RetryMessage {
        /// Message ID to retry
        message_id: String,
        /// Channel ID
        channel: String,
        /// Original message content
        content: String,
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
    /// Ban user from home
    BanUser {
        /// Target user
        target: String,

        /// Optional reason
        reason: Option<String>,
    },
    /// Unban user from home
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
    /// Invite user to channel/home
    InviteUser {
        /// Target user
        target: String,
        /// Channel to invite to
        channel: String,
    },

    // === Contact Commands ===
    /// Update a contact's nickname
    UpdateContactNickname {
        /// Contact ID
        contact_id: String,
        /// New nickname
        nickname: String,
    },
    /// Remove a contact
    RemoveContact {
        /// Contact ID
        contact_id: String,
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

    // === LAN Discovery Commands ===
    /// List LAN-discovered peers
    ListLanPeers,
    /// Invite a LAN-discovered peer as a contact
    InviteLanPeer {
        /// Authority ID of the peer
        authority_id: String,
        /// Address of the peer (IP:port)
        address: String,
    },

    // === Invitation Commands ===
    /// Create a new invitation
    CreateInvitation {
        /// Receiver authority ID
        receiver_id: String,
        /// Type of invitation (Contact, Guardian, Channel)
        invitation_type: String,
        /// Optional message to include
        message: Option<String>,
        /// TTL in seconds (None = no expiry)
        ttl_secs: Option<u64>,
    },
    /// Export an invitation as a shareable code
    ExportInvitation {
        /// Invitation ID to export
        invitation_id: String,
    },
    /// Import an invitation from a code
    ImportInvitation {
        /// The invitation code to import
        code: String,
    },
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
    /// Cancel/revoke a sent invitation
    CancelInvitation {
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
    /// Discover peers from rendezvous service and add to known peers
    DiscoverPeers,

    // === Neighborhood Traversal Commands ===
    /// Move to adjacent position in neighborhood
    MovePosition {
        /// Target neighborhood ID
        neighborhood_id: String,
        /// Target home ID
        home_id: String,
        /// Traversal depth (Street, Frontage, Interior)
        depth: String,
    },

    // === General Commands ===
    /// Ping/health check
    Ping,
    /// Shutdown the bridge
    Shutdown,

    // === Backup Commands ===
    /// Export account to a portable backup code
    ExportAccountBackup,
    /// Import account from backup code
    ImportAccountBackup {
        /// The backup code to import
        backup_code: String,
    },

    // === Test-only Commands (not user-parsed) ===
    /// Intentionally unmapped command used to exercise "unknown command" error paths in unit tests.
    ///
    /// This is not produced by the command parser and is not handled by the
    /// intent mapper or operational handlers.
    #[cfg(test)]
    UnknownCommandForTest,
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
            | Self::DiscoverPeers
            | Self::ListLanPeers
            | Self::Ping
            | Self::ListParticipants { .. }
            | Self::GetUserInfo { .. } => CommandAuthorizationLevel::Public,

            #[cfg(test)]
            Self::UnknownCommandForTest => CommandAuthorizationLevel::Public,

            // Basic - user token required
            Self::SendMessage { .. }
            | Self::SendDirectMessage { .. }
            | Self::StartDirectChat { .. }
            | Self::SendAction { .. }
            | Self::CreateChannel { .. }
            | Self::CloseChannel { .. }
            | Self::JoinChannel { .. }
            | Self::LeaveChannel { .. }
            | Self::RetryMessage { .. }
            | Self::UpdateNickname { .. }
            | Self::UpdateContactNickname { .. }
            | Self::SetTopic { .. }
            | Self::PinMessage { .. }
            | Self::UnpinMessage { .. }
            | Self::MovePosition { .. }
            | Self::InviteUser { .. }
            | Self::CreateInvitation { .. }
            | Self::ExportInvitation { .. }
            | Self::ImportInvitation { .. }
            | Self::AcceptInvitation { .. }
            | Self::DeclineInvitation { .. }
            | Self::CancelInvitation { .. }
            | Self::AcceptPendingHomeInvitation
            | Self::SendHomeInvitation { .. }
            | Self::InviteLanPeer { .. }
            | Self::SetContext { .. }
            | Self::RemoveContact { .. } => CommandAuthorizationLevel::Basic,

            // CreateAccount is special - it's the bootstrapping command that creates an account
            // It requires Basic level since users need to create an account before they have one
            Self::CreateAccount { .. } => CommandAuthorizationLevel::Basic,

            // Sensitive - elevated authorization (requires existing account/elevated token)
            Self::CreateHome { .. }
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
            | Self::UnmuteUser { .. }
            | Self::ExportAccountBackup
            | Self::ImportAccountBackup { .. } => CommandAuthorizationLevel::Sensitive,

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
    /// Direct chat started with a contact
    DirectChatStarted {
        /// Contact ID to chat with
        contact_id: String,
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
    /// Peers discovered from rendezvous/sync service
    PeersDiscovered {
        /// Number of peers discovered from effect system
        discovered: u32,
        /// Number of new peers added to known_peers
        new_peers: u32,
        /// Total known peers after discovery
        total: u32,
    },

    // === Home Events ===
    /// Home created
    HomeCreated {
        /// Home ID
        home_id: String,
        /// Home name
        name: Option<String>,
    },
    /// Joined a home
    HomeJoined {
        /// Home ID
        home_id: String,
    },

    // === LAN Discovery Events ===
    /// LAN peers list updated
    LanPeersUpdated {
        /// List of discovered peers (authority_id, address, age_secs)
        peers: Vec<(String, String, u64)>,
    },
    /// A new peer was discovered on the LAN
    LanPeerDiscovered {
        /// Authority ID of the peer
        authority_id: String,
        /// Address (IP:port)
        address: String,
    },
    /// LAN peer invitation sent
    LanPeerInvited {
        /// Authority ID of the invited peer
        authority_id: String,
    },

    // === Invitation Events ===
    /// Invitation created
    InvitationCreated {
        /// Invitation ID
        invitation_id: String,
        /// Invitation type
        invitation_type: String,
        /// Shareable code (if available)
        code: Option<String>,
    },
    /// Invitation code exported
    InvitationCodeExported {
        /// Invitation ID
        invitation_id: String,
        /// The shareable code
        code: String,
    },
    /// Invitation imported from code
    InvitationImported {
        /// Invitation ID (local)
        invitation_id: String,
        /// Sender name/ID
        from: String,
        /// Invitation type
        invitation_type: String,
    },
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
    /// User banned from home
    UserBanned {
        /// Target user ID
        target: String,
        /// Actor who performed the ban
        actor: String,
        /// Optional reason
        reason: Option<String>,
    },
    /// User unbanned from home
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
    /// User invited to home/channel
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
        /// Home ID where role was granted
        home_id: String,
    },
    /// Steward role revoked
    StewardRevoked {
        /// Target user ID
        target: String,
        /// Actor who revoked the role
        actor: String,
        /// Home ID where role was revoked
        home_id: String,
    },

    // === Contacts & Nicknames Events ===
    /// User nickname updated
    NicknameUpdated {
        /// New nickname
        nickname: String,
    },
    /// Contact nickname updated
    ContactNicknameUpdated {
        /// Contact ID
        contact_id: String,
        /// New nickname
        nickname: String,
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
        /// Home ID
        home_id: String,
        /// Traversal depth
        depth: String,
    },

    // === Channel Management Events ===
    /// Channel topic set
    TopicSet {
        /// Channel/home ID
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
        /// Channel/home ID where message was pinned
        channel: String,
        /// Actor who pinned the message
        actor: String,
    },
    /// Message unpinned
    MessageUnpinned {
        /// Message ID
        message_id: String,
        /// Channel/home ID where message was unpinned
        channel: String,
        /// Actor who unpinned the message
        actor: String,
    },
    /// Channel mode set
    ChannelModeSet {
        /// Channel/home ID
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
    /// Include home events
    pub home: bool,
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
    /// Include LAN discovery events
    pub lan_discovery: bool,
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
            home: true,
            invitation: true,
            settings: true,
            moderation: true,
            authorization: true,
            errors: true,
            system: true,
            lan_discovery: true,
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
            | AuraEvent::ContactNicknameUpdated { .. }
            | AuraEvent::ContactGuardianToggled { .. }
            | AuraEvent::GuardianInvitationSent { .. } => self.account,
            AuraEvent::MessageReceived { .. }
            | AuraEvent::UserJoined { .. }
            | AuraEvent::UserLeft { .. }
            | AuraEvent::ChannelCreated { .. }
            | AuraEvent::ChannelClosed { .. }
            | AuraEvent::DirectChatStarted { .. } => self.chat,
            AuraEvent::SyncStarted { .. }
            | AuraEvent::SyncCompleted { .. }
            | AuraEvent::SyncFailed { .. }
            | AuraEvent::PeerAdded { .. }
            | AuraEvent::PeerRemoved { .. }
            | AuraEvent::PeersListed { .. }
            | AuraEvent::PeersDiscovered { .. } => self.sync,
            AuraEvent::HomeCreated { .. } | AuraEvent::HomeJoined { .. } => self.home,
            AuraEvent::InvitationCreated { .. }
            | AuraEvent::InvitationCodeExported { .. }
            | AuraEvent::InvitationImported { .. }
            | AuraEvent::InvitationAccepted { .. }
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
            AuraEvent::LanPeersUpdated { .. }
            | AuraEvent::LanPeerDiscovered { .. }
            | AuraEvent::LanPeerInvited { .. } => self.lan_discovery,
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
