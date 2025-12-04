//! Unified protocol error type for aura-protocol.
//!
//! This consolidates overlapping error enums (consensus, guards, messaging)
//! and provides straightforward conversions between them.

use crate::consensus::ConsensusError;
use crate::guards::GuardError;
use aura_core::AuraError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("Consensus error: {0}")]
    Consensus(#[from] ConsensusError),

    #[error("Guard error: {0}")]
    Guard(#[from] GuardError),

    #[error("Message error: {0}")]
    Message(#[from] AuraError),

    #[error("Serialization failed: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("Other error: {0}")]
    Other(String),
}
