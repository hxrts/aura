//! FROST (Flexible Round-Optimized Schnorr Threshold) Protocol Messages
//!
//! Messages used in threshold signature protocols for distributed signing.

use aura_core::{DeviceId, SessionId};
use serde::{Deserialize, Serialize};

/// FROST protocol message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FrostMessage {
    /// Initiate FROST DKG (Distributed Key Generation)
    InitiateDkg(FrostDkgInitMessage),
    /// DKG commitment message
    DkgCommitment(FrostDkgCommitmentMessage),
    /// DKG share distribution
    DkgShareDistribution(FrostDkgShareMessage),
    /// DKG finalization
    DkgFinalize(FrostDkgFinalizeMessage),
    /// Initiate threshold signing
    InitiateSigning(FrostSigningInitMessage),
    /// Signing commitment (Round 1)
    SigningCommitment(FrostSigningCommitmentMessage),
    /// Signature share (Round 2)
    SignatureShare(FrostSignatureShareMessage),
    /// Aggregate signature
    AggregateSignature(FrostAggregateSignatureMessage),
    /// Protocol abort
    Abort(FrostAbortMessage),
}

/// FROST DKG initiation message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostDkgInitMessage {
    pub session_id: SessionId,
    pub threshold: u16,
    pub participants: Vec<DeviceId>,
    pub group_context: Vec<u8>,
}

/// FROST DKG commitment message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostDkgCommitmentMessage {
    pub session_id: SessionId,
    pub participant_id: DeviceId,
    pub commitments: Vec<u8>, // Polynomial commitments
    pub proof_of_possession: Vec<u8>,
}

/// FROST DKG share distribution message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostDkgShareMessage {
    pub session_id: SessionId,
    pub from_participant: DeviceId,
    pub to_participant: DeviceId,
    pub encrypted_share: Vec<u8>, // HPKE encrypted share
    pub share_proof: Vec<u8>,
}

/// FROST DKG finalization message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostDkgFinalizeMessage {
    pub session_id: SessionId,
    pub group_public_key: Vec<u8>,
    pub verification_key_shares: Vec<(DeviceId, Vec<u8>)>,
    pub threshold_public_key: Vec<u8>,
}

/// FROST signing initiation message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostSigningInitMessage {
    pub session_id: SessionId,
    pub message_to_sign: Vec<u8>,
    pub signing_participants: Vec<DeviceId>,
    pub context: Option<Vec<u8>>,
}

/// FROST signing commitment message (Round 1)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostSigningCommitmentMessage {
    pub session_id: SessionId,
    pub participant_id: DeviceId,
    pub hiding_commitment: Vec<u8>,
    pub binding_commitment: Vec<u8>,
}

/// FROST signature share message (Round 2)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostSignatureShareMessage {
    pub session_id: SessionId,
    pub participant_id: DeviceId,
    pub signature_share: Vec<u8>,
    pub commitment_proof: Vec<u8>,
}

/// FROST aggregate signature message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostAggregateSignatureMessage {
    pub session_id: SessionId,
    pub aggregated_signature: Vec<u8>,
    pub signing_participants: Vec<DeviceId>,
    pub signature_verification: FrostSignatureVerification,
}

/// FROST signature verification data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostSignatureVerification {
    pub is_valid: bool,
    pub verification_details: Vec<ParticipantVerification>,
    pub group_verification: bool,
}

/// Per-participant verification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantVerification {
    pub participant_id: DeviceId,
    pub share_valid: bool,
    pub commitment_valid: bool,
    pub error_message: Option<String>,
}

/// FROST protocol abort message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostAbortMessage {
    pub session_id: SessionId,
    pub reason: FrostAbortReason,
    pub failed_participant: Option<DeviceId>,
    pub error_details: Option<String>,
}

/// Reasons for FROST protocol abort
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FrostAbortReason {
    InvalidCommitment,
    InvalidShare,
    InvalidSignature,
    InsufficientParticipants,
    CommunicationFailure,
    Timeout,
    ByzantineBehavior,
}

/// FROST protocol result for successful operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostDkgResult {
    pub session_id: SessionId,
    pub group_public_key: Vec<u8>,
    pub participant_shares: Vec<(DeviceId, Vec<u8>)>,
    pub threshold: u16,
    pub success: bool,
}

/// FROST signing result for successful signatures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostSigningResult {
    pub session_id: SessionId,
    pub signature: Vec<u8>,
    pub message: Vec<u8>,
    pub signing_participants: Vec<DeviceId>,
    pub verification: FrostSignatureVerification,
}

// All message types use standard serde traits for serialization
