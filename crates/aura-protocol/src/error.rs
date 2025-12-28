//! Unified protocol error type for aura-protocol.
//!
//! This consolidates overlapping error enums (consensus, guards, messaging)
//! and provides straightforward conversions between them.

use aura_anti_entropy::SyncError;
use aura_consensus::ConsensusError;
use aura_core::effects::amp::AmpChannelError;
use aura_core::AuraError;
use aura_guards::GuardError;
use thiserror::Error;

/// Error types for protocol orchestration.
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// Consensus subsystem failure.
    #[error("Consensus error: {0}")]
    Consensus(#[from] ConsensusError),

    /// Guard-chain enforcement failure.
    #[error("Guard error: {0}")]
    Guard(#[from] GuardError),

    /// AMP transport/channel failure.
    #[error("AMP error: {0}")]
    Amp(#[from] AmpChannelError),

    /// Anti-entropy sync failure.
    #[error("Sync error: {0}")]
    Sync(#[from] SyncError),

    /// Protocol message handling failure.
    #[error("Message error: {0}")]
    Message(#[from] AuraError),

    /// Serialization failure for protocol messages.
    #[error("Serialization failed: {0}")]
    Serialization(#[from] aura_core::util::serialization::SerializationError),

    /// Catch-all error for unexpected cases.
    #[error("Other error: {0}")]
    Other(String),
}

impl aura_core::ProtocolErrorCode for ProtocolError {
    fn code(&self) -> &'static str {
        match self {
            ProtocolError::Consensus(err) => err.code(),
            ProtocolError::Guard(err) => err.code(),
            ProtocolError::Amp(err) => err.code(),
            ProtocolError::Sync(err) => err.code(),
            ProtocolError::Message(err) => err.code(),
            ProtocolError::Serialization(_) => "serialization",
            ProtocolError::Other(_) => "other",
        }
    }
}
