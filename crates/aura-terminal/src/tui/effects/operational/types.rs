//! Operational command types
//!
//! Response and error types for operational commands.

use aura_core::types::Epoch;

/// Result type for operational commands
pub type OpResult = Result<OpResponse, OpError>;

/// Response from an operational command
#[derive(Debug, Clone)]
pub enum OpResponse {
    /// Command succeeded with no data
    Ok,
    /// Legacy free-form data response (avoid for core flows).
    Data(String),
    /// Legacy free-form list response (avoid for core flows).
    List(Vec<String>),
    /// Invitation code exported
    InvitationCode { id: String, code: String },
    /// Invitation code imported (parsed successfully)
    InvitationImported {
        /// The parsed invitation ID
        invitation_id: String,
        /// Sender authority ID
        sender_id: String,
        /// Invitation type (channel, guardian, contact)
        invitation_type: String,
        /// Optional expiration timestamp
        expires_at: Option<u64>,
        /// Optional message from sender
        message: Option<String>,
    },
    /// Device enrollment ceremony started (Settings → Add device)
    DeviceEnrollmentStarted {
        /// Ceremony identifier for polling/cancel
        ceremony_id: String,
        /// Shareable enrollment code to import on the new device
        enrollment_code: String,
        /// Pending epoch created during prepare
        pending_epoch: Epoch,
        /// Device id being enrolled
        device_id: String,
    },
    /// Device removal ceremony started (Settings → Remove device)
    DeviceRemovalStarted {
        /// Ceremony identifier for polling/cancel
        ceremony_id: String,
    },
    /// Context changed (for SetContext command)
    ContextChanged {
        /// The new context ID (None to clear)
        context_id: Option<String>,
    },
    /// Channel mode updated (for SetChannelMode command)
    ChannelModeSet {
        /// Channel ID that was updated
        channel_id: String,
        /// Mode flags that were applied
        flags: String,
    },
    /// Display name/nickname updated
    NicknameUpdated {
        /// The new display name
        name: String,
    },
    /// MFA policy updated
    MfaPolicyUpdated {
        /// Whether MFA is now required
        require_mfa: bool,
    },
    /// Known peer list (network)
    PeersListed { peers: Vec<String> },
    /// Discovered LAN peer list (network)
    LanPeersListed { peers: Vec<String> },
    /// Network discovery completed/triggered with current known-peer count.
    PeerDiscoveryTriggered { known_peers: usize },
    /// LAN invitation dispatch status.
    LanInvitationStatus {
        authority_id: String,
        address: String,
        message: String,
    },
    /// Query response: channel participants.
    ParticipantsListed { participants: Vec<String> },
    /// Query response: formatted user info.
    UserInfo { info: String },
    /// Recovery ceremony started.
    RecoveryStarted { ceremony_id: String },
    /// Recovery flow canceled.
    RecoveryCancelled,
    /// Recovery completed.
    RecoveryCompleted,
    /// Guardian invitation created during recovery setup.
    RecoveryGuardianInvited { invitation_id: String },
    /// Home invitation accepted.
    HomeInvitationAccepted { invitation_id: String },
    /// Home created.
    HomeCreated { home_id: String },
    /// Neighborhood created.
    NeighborhoodCreated { neighborhood_id: String },
    /// Home add-to-neighborhood outcome.
    HomeAddedToNeighborhood {
        target_home_id: String,
        message: Option<String>,
    },
    /// OneHopLink operation completed.
    HomeOneHopLinkSet { target_home_id: String },
    /// Message sent in a channel.
    ChannelMessageSent { message_id: String },
    /// New channel created.
    ChannelCreated { channel_id: String },
    /// Direct message channel sent/created.
    DirectMessageSent { channel_id: String },
    /// Action/emote message sent.
    ActionSent { message_id: String },
    /// Invitation sent to a channel target.
    ChannelInvitationSent { invitation_id: String },
    /// Channel joined.
    ChannelJoined { channel_id: String },
    /// Retry message dispatch completed.
    RetrySent { message_id: String },
    /// Sync request issued for peer.
    PeerStateRequested { peer_id: String },
    /// Contact guardian flag toggled.
    ContactGuardianToggled {
        contact_id: String,
        is_guardian: bool,
    },
}

/// Error from an operational command
#[derive(Debug, Clone, thiserror::Error)]
pub enum OpError {
    #[error("Not implemented: {0}")]
    NotImplemented(String),
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    #[error("Operation failed: {0}")]
    Failed(String),
}
