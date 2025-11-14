//! DKD (Deterministic Key Derivation) Protocol Messages
//!
//! Messages used in the distributed key derivation protocol for generating
//! context-specific cryptographic identities.

use aura_core::{DeviceId, SessionId};
use serde::{Deserialize, Serialize};

/// DKD protocol message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DkdMessage {
    /// Initiate a new DKD session
    InitiateSession(InitiateDkdSessionMessage),
    /// Commit to a point in Phase 1
    PointCommitment(DkdPointCommitmentMessage),
    /// Reveal point in Phase 2
    PointReveal(DkdPointRevealMessage),
    /// Finalize DKD with derived identity
    Finalize(DkdFinalizeMessage),
    /// Abort DKD session
    Abort(DkdAbortMessage),
}

/// Initiate DKD session message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitiateDkdSessionMessage {
    pub session_id: SessionId,
    pub context_id: Vec<u8>,
    pub threshold: u16,
    pub participants: Vec<DeviceId>,
    pub start_epoch: u64,
    pub ttl_in_epochs: u64,
}

/// DKD point commitment message (Phase 1)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdPointCommitmentMessage {
    pub session_id: SessionId,
    pub device_id: DeviceId,
    pub commitment: [u8; 32], // hash(Point)
    pub commitment_proof: Option<Vec<u8>>,
}

/// DKD point reveal message (Phase 2)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdPointRevealMessage {
    pub session_id: SessionId,
    pub device_id: DeviceId,
    pub point: Vec<u8>, // Compressed Edwards point (32 bytes)
    pub opening_proof: Option<Vec<u8>>,
}

/// DKD finalization message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdFinalizeMessage {
    pub session_id: SessionId,
    pub seed_fingerprint: [u8; 32],
    pub commitment_root: [u8; 32],    // Merkle root of all commitments
    pub derived_identity_pk: Vec<u8>, // Public key derived from seed
    pub verification_data: Option<Vec<u8>>,
}

/// DKD session abort message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdAbortMessage {
    pub session_id: SessionId,
    pub reason: DkdAbortReason,
    pub blamed_device: Option<DeviceId>,
    pub evidence: Option<Vec<u8>>,
}

/// Reasons for DKD abort
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DkdAbortReason {
    Timeout,
    ByzantineBehavior {
        device_id: DeviceId,
        details: String,
    },
    CollisionDetected,
    InsufficientParticipants,
    InvalidCommitment,
    InvalidReveal,
}

/// DKD protocol phases
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DkdPhase {
    Initialization,
    Commitment,
    Reveal,
    Finalization,
    Aborted,
    Completed,
}

// All message types use standard serde traits for serialization
