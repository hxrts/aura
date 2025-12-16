//! Operational command types
//!
//! Response and error types for operational commands.

/// Result type for operational commands
pub type OpResult = Result<OpResponse, OpError>;

/// Response from an operational command
#[derive(Debug, Clone)]
pub enum OpResponse {
    /// Command succeeded with no data
    Ok,
    /// Command returned data
    Data(String),
    /// Command returned a list
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
