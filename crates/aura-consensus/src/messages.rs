//! Consensus protocol messages
//!
//! This module defines the messages exchanged during the consensus protocol.

use super::types::{CommitFact, ConsensusId};
use aura_core::{
    epochs::Epoch,
    frost::{NonceCommitment, PartialSignature},
    Hash32,
};
use serde::{Deserialize, Serialize};

/// Messages exchanged during consensus protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusMessage {
    /// Phase 1: Execute request from coordinator to witnesses
    Execute {
        consensus_id: ConsensusId,
        prestate_hash: Hash32,
        operation_hash: Hash32,
        operation_bytes: Vec<u8>,
        /// Optional cached commitments for fast path (1 RTT)
        cached_commitments: Option<Vec<NonceCommitment>>,
    },

    /// Phase 2a: Nonce commitment from witness (slow path only)
    NonceCommit {
        consensus_id: ConsensusId,
        commitment: NonceCommitment,
    },

    /// Phase 2b: Aggregated nonces for signing (slow path only)
    SignRequest {
        consensus_id: ConsensusId,
        aggregated_nonces: Vec<NonceCommitment>,
    },

    /// Phase 3: Partial signature from witness
    SignShare {
        consensus_id: ConsensusId,
        share: PartialSignature,
        /// Optional commitment for the next consensus round (pipelining optimization)
        next_commitment: Option<NonceCommitment>,
        /// Epoch for commitment validation
        epoch: Epoch,
    },

    /// Phase 4: Final consensus result broadcast
    ConsensusResult { commit_fact: CommitFact },

    /// Conflict detected during consensus
    Conflict {
        consensus_id: ConsensusId,
        conflicts: Vec<Hash32>,
    },

    /// Acknowledgment message (for reliable delivery)
    Ack {
        consensus_id: ConsensusId,
        phase: ConsensusPhase,
    },
}

/// Consensus protocol phases for acknowledgment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsensusPhase {
    /// Execute phase
    Execute,
    /// Nonce commitment phase
    NonceCommit,
    /// Signing phase
    Sign,
    /// Result phase
    Result,
}

impl ConsensusMessage {
    /// Get the consensus ID for this message
    pub fn consensus_id(&self) -> ConsensusId {
        match self {
            ConsensusMessage::Execute { consensus_id, .. } => *consensus_id,
            ConsensusMessage::NonceCommit { consensus_id, .. } => *consensus_id,
            ConsensusMessage::SignRequest { consensus_id, .. } => *consensus_id,
            ConsensusMessage::SignShare { consensus_id, .. } => *consensus_id,
            ConsensusMessage::ConsensusResult { commit_fact } => commit_fact.consensus_id,
            ConsensusMessage::Conflict { consensus_id, .. } => *consensus_id,
            ConsensusMessage::Ack { consensus_id, .. } => *consensus_id,
        }
    }

    /// Get the phase of this message
    pub fn phase(&self) -> ConsensusPhase {
        match self {
            ConsensusMessage::Execute { .. } => ConsensusPhase::Execute,
            ConsensusMessage::NonceCommit { .. } => ConsensusPhase::NonceCommit,
            ConsensusMessage::SignRequest { .. } => ConsensusPhase::Sign,
            ConsensusMessage::SignShare { .. } => ConsensusPhase::Sign,
            ConsensusMessage::ConsensusResult { .. } => ConsensusPhase::Result,
            ConsensusMessage::Conflict { .. } => ConsensusPhase::Result,
            ConsensusMessage::Ack { phase, .. } => *phase,
        }
    }

    /// Check if this message is for fast path (1 RTT)
    pub fn is_fast_path(&self) -> bool {
        match self {
            ConsensusMessage::Execute {
                cached_commitments, ..
            } => cached_commitments.is_some(),
            ConsensusMessage::SignShare {
                next_commitment, ..
            } => next_commitment.is_some(),
            _ => false,
        }
    }
}

/// Request to run consensus
#[derive(Debug, Clone)]
pub struct ConsensusRequest {
    /// Hash of the current state
    pub prestate_hash: Hash32,
    /// Operation to reach consensus on
    pub operation_bytes: Vec<u8>,
    /// Hash of the operation
    pub operation_hash: Hash32,
    /// Optional timeout override (milliseconds)
    pub timeout_ms: Option<u64>,
}

/// Response from consensus operation
#[derive(Debug, Clone)]
pub struct ConsensusResponse {
    /// The consensus ID that was used
    pub consensus_id: ConsensusId,
    /// Result of the consensus
    pub result: Result<CommitFact, ConsensusError>,
    /// Time taken (milliseconds)
    pub duration_ms: u64,
    /// Whether fast path was used
    pub fast_path: bool,
}

/// Errors that can occur during consensus
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConsensusError {
    #[error("Timeout after {0}ms")]
    Timeout(u64),

    #[error("Conflict detected: {0}")]
    Conflict(String),

    #[error("Insufficient witnesses: have {have}, need {need}")]
    InsufficientWitnesses { have: u32, need: u32 },

    #[error("Invalid prestate: {0}")]
    InvalidPrestate(String),

    #[error("Cryptographic error: {0}")]
    Crypto(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl aura_core::ProtocolErrorCode for ConsensusError {
    fn code(&self) -> &'static str {
        match self {
            ConsensusError::Timeout(_) => "timeout",
            ConsensusError::Conflict(_) => "conflict",
            ConsensusError::InsufficientWitnesses { .. } => "insufficient_witnesses",
            ConsensusError::InvalidPrestate(_) => "invalid_prestate",
            ConsensusError::Crypto(_) => "crypto",
            ConsensusError::Network(_) => "network",
            ConsensusError::Internal(_) => "internal",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_consensus_id() {
        let id = ConsensusId::new(Hash32::default(), Hash32([1u8; 32]), 42);

        let msg = ConsensusMessage::Execute {
            consensus_id: id,
            prestate_hash: Hash32::default(),
            operation_hash: Hash32([1u8; 32]),
            operation_bytes: vec![],
            cached_commitments: None,
        };

        assert_eq!(msg.consensus_id(), id);
        assert_eq!(msg.phase(), ConsensusPhase::Execute);
        assert!(!msg.is_fast_path());
    }

    #[test]
    fn test_fast_path_detection() {
        let id = ConsensusId::new(Hash32::default(), Hash32([1u8; 32]), 42);

        // Fast path execute with cached commitments
        let fast_execute = ConsensusMessage::Execute {
            consensus_id: id,
            prestate_hash: Hash32::default(),
            operation_hash: Hash32([1u8; 32]),
            operation_bytes: vec![],
            cached_commitments: Some(vec![]),
        };
        assert!(fast_execute.is_fast_path());

        // Fast path sign share with next commitment
        let fast_sign = ConsensusMessage::SignShare {
            consensus_id: id,
            share: PartialSignature {
                signer: 1,
                signature: vec![],
            },
            next_commitment: Some(NonceCommitment {
                signer: 1,
                commitment: vec![],
            }),
            epoch: Epoch::from(1),
        };
        assert!(fast_sign.is_fast_path());
    }
}
