//! AMP channel lifecycle effect traits (Layer 1 interface)
//!
//! This defines the interface for creating/closing AMP channels and sending
//! messages. Implementations live in higher layers (protocol/runtime). This
//! module must remain interface-only (no state or OS access).

use crate::identifiers::{AuthorityId, ChannelId, ContextId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// AMP channel error
#[derive(Debug, thiserror::Error, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum AmpChannelError {
    #[error("channel not found")]
    NotFound,
    #[error("context not found")]
    ContextNotFound,
    #[error("invalid state: {0}")]
    InvalidState(String),
    #[error("authorization failed")]
    Unauthorized,
    #[error("storage error: {0}")]
    Storage(String),
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl crate::ProtocolErrorCode for AmpChannelError {
    fn code(&self) -> &'static str {
        match self {
            AmpChannelError::NotFound => "not_found",
            AmpChannelError::ContextNotFound => "not_found",
            AmpChannelError::InvalidState(_) => "invalid_state",
            AmpChannelError::Unauthorized => "unauthorized",
            AmpChannelError::Storage(_) => "storage",
            AmpChannelError::Crypto(_) => "crypto",
            AmpChannelError::Internal(_) => "internal",
        }
    }
}

/// AMP message header (additional authenticated data)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AmpHeader {
    pub context: ContextId,
    pub channel: ChannelId,
    pub chan_epoch: u64,
    pub ratchet_gen: u64,
}

/// Result of a send operation (ciphertext + header)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AmpCiphertext {
    pub header: AmpHeader,
    pub ciphertext: Vec<u8>,
}

/// Optional parameters for channel creation
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelCreateParams {
    pub context: ContextId,
    /// Supply a channel id or let the implementation generate one
    pub channel: Option<ChannelId>,
    /// Optional skip window override (default 1024 in AMP spec)
    pub skip_window: Option<u32>,
    /// Human-friendly topic/label (metadata only)
    pub topic: Option<String>,
}

/// Channel close parameters
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelCloseParams {
    pub context: ContextId,
    pub channel: ChannelId,
}

/// Channel join parameters
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelJoinParams {
    pub context: ContextId,
    pub channel: ChannelId,
    /// The participant joining the channel
    pub participant: AuthorityId,
}

/// Channel leave parameters
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelLeaveParams {
    pub context: ContextId,
    pub channel: ChannelId,
    /// The participant leaving the channel
    pub participant: AuthorityId,
}

/// Send parameters (plaintext provided by caller)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelSendParams {
    pub context: ContextId,
    pub channel: ChannelId,
    pub sender: AuthorityId,
    /// UTF-8 or arbitrary bytes; implementation encrypts
    pub plaintext: Vec<u8>,
    pub reply_to: Option<Vec<u8>>, // message id if available
}

#[async_trait]
pub trait AmpChannelEffects: Send + Sync {
    /// Create a channel within a relational context. Returns the ChannelId.
    async fn create_channel(
        &self,
        params: ChannelCreateParams,
    ) -> Result<ChannelId, AmpChannelError>;

    /// Close/archive a channel.
    async fn close_channel(&self, params: ChannelCloseParams) -> Result<(), AmpChannelError>;

    /// Join an existing channel.
    async fn join_channel(&self, params: ChannelJoinParams) -> Result<(), AmpChannelError>;

    /// Leave a channel.
    async fn leave_channel(&self, params: ChannelLeaveParams) -> Result<(), AmpChannelError>;

    /// Send a message on a channel, returning ciphertext + AMP header.
    async fn send_message(
        &self,
        params: ChannelSendParams,
    ) -> Result<AmpCiphertext, AmpChannelError>;
}

// Blanket impl for Arc<T>
#[async_trait]
impl<T: AmpChannelEffects + ?Sized> AmpChannelEffects for std::sync::Arc<T> {
    async fn create_channel(
        &self,
        params: ChannelCreateParams,
    ) -> Result<ChannelId, AmpChannelError> {
        (**self).create_channel(params).await
    }

    async fn close_channel(&self, params: ChannelCloseParams) -> Result<(), AmpChannelError> {
        (**self).close_channel(params).await
    }

    async fn join_channel(&self, params: ChannelJoinParams) -> Result<(), AmpChannelError> {
        (**self).join_channel(params).await
    }

    async fn leave_channel(&self, params: ChannelLeaveParams) -> Result<(), AmpChannelError> {
        (**self).leave_channel(params).await
    }

    async fn send_message(
        &self,
        params: ChannelSendParams,
    ) -> Result<AmpCiphertext, AmpChannelError> {
        (**self).send_message(params).await
    }
}
