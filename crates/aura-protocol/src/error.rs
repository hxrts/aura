#![allow(
    missing_docs,
    unused_variables,
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    clippy::match_like_matches_macro,
    clippy::type_complexity,
    clippy::while_let_loop,
    clippy::redundant_closure,
    clippy::large_enum_variant,
    clippy::unused_unit,
    clippy::get_first,
    clippy::single_range_in_vec_init,
    clippy::disallowed_methods, // Orchestration layer coordinates time/random effects
    deprecated // Deprecated time/random functions used intentionally for effect coordination
)]
//! Unified protocol error type for aura-protocol.
//!
//! This consolidates overlapping error enums (consensus, guards, messaging)
//! and provides straightforward conversions between them.

use aura_consensus::ConsensusError;
use aura_guards::GuardError;
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

impl aura_core::ProtocolErrorCode for ProtocolError {
    fn code(&self) -> &'static str {
        match self {
            ProtocolError::Consensus(err) => err.code(),
            ProtocolError::Guard(err) => err.code(),
            ProtocolError::Message(err) => err.code(),
            ProtocolError::Serialization(_) => "serialization",
            ProtocolError::Other(_) => "other",
        }
    }
}
