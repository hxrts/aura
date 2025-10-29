//! Account Recovery Protocol Messages
//!
//! Messages used in social recovery protocols for account restoration
//! through guardian approval and encrypted share reconstruction.

use crate::serialization::WireSerializable;
use aura_types::{DeviceId, GuardianId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Recovery protocol message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryMessage {
    /// Initiate account recovery
    InitiateRecovery(InitiateRecoveryMessage),
    /// Guardian approval for recovery request
    GuardianApproval(GuardianApprovalMessage),
    /// Submit encrypted recovery share
    SubmitRecoveryShare(SubmitRecoveryShareMessage),
    /// Complete recovery with reconstructed identity
    CompleteRecovery(CompleteRecoveryMessage),
    /// Abort recovery (timeout or cancellation)
    AbortRecovery(AbortRecoveryMessage),
    /// Nudge guardian to respond to recovery request
    NudgeGuardian(NudgeGuardianMessage),
}

/// Recovery initiation message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitiateRecoveryMessage {
    pub recovery_id: Uuid,
    pub new_device_id: DeviceId,
    pub new_device_pk: Vec<u8>,
    pub required_guardians: Vec<GuardianId>,
    pub quorum_threshold: u16,
    pub cooldown_seconds: u64,
    pub recovery_context: Vec<u8>,
    pub identity_proof: Vec<u8>,
}

/// Guardian approval message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianApprovalMessage {
    pub recovery_id: Uuid,
    pub guardian_id: GuardianId,
    pub approved: bool,
    pub approval_signature: Vec<u8>,
    pub approval_timestamp: u64,
    pub guardian_notes: Option<String>,
}

/// Recovery share submission message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitRecoveryShareMessage {
    pub recovery_id: Uuid,
    pub guardian_id: GuardianId,
    pub encrypted_share: Vec<u8>, // HPKE with AAD
    pub merkle_proof: aura_crypto::MerkleProof,
    pub dkd_session_id: Uuid,
    pub share_verification: RecoveryShareVerification,
}

/// Recovery share verification data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryShareVerification {
    pub share_valid: bool,
    pub merkle_proof_valid: bool,
    pub encryption_verified: bool,
    pub guardian_signature_valid: bool,
    pub error_details: Option<String>,
}

/// Recovery completion message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteRecoveryMessage {
    pub recovery_id: Uuid,
    pub new_device_id: DeviceId,
    pub test_signature: Vec<u8>, // Proof that recovered identity works
    pub recovered_shares: Vec<RecoveredShare>,
    pub verification_data: RecoveryVerification,
}

/// Recovered share information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveredShare {
    pub guardian_id: GuardianId,
    pub share_index: u16,
    pub verification_successful: bool,
    pub contribution_weight: f64,
}

/// Recovery verification data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryVerification {
    pub quorum_achieved: bool,
    pub shares_reconstructed: u16,
    pub test_signature_valid: bool,
    pub guardian_approvals: Vec<GuardianApproval>,
    pub recovery_successful: bool,
}

/// Guardian approval record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianApproval {
    pub guardian_id: GuardianId,
    pub approved: bool,
    pub approval_timestamp: u64,
    pub signature_valid: bool,
}

/// Recovery abort message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbortRecoveryMessage {
    pub recovery_id: Uuid,
    pub reason: RecoveryAbortReason,
    pub aborted_by: Option<DeviceId>,
    pub error_details: Option<String>,
}

/// Reasons for recovery abort
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryAbortReason {
    Timeout,
    InsufficientApprovals,
    VerificationFailed,
    UserCancelled,
    GuardianRefusal,
    InvalidShares,
    CommunicationFailure,
}

/// Guardian nudge message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NudgeGuardianMessage {
    pub recovery_id: Uuid,
    pub guardian_id: GuardianId,
    pub nudge_count: u16,
    pub urgency_level: NudgeUrgency,
    pub message: Option<String>,
}

/// Nudge urgency levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NudgeUrgency {
    Low,
    Normal,
    High,
    Critical,
}

/// Recovery protocol result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryProtocolResult {
    pub recovery_id: Uuid,
    pub success: bool,
    pub new_device_id: Option<DeviceId>,
    pub recovered_identity: Option<Vec<u8>>,
    pub guardian_participation: Vec<GuardianParticipation>,
    pub verification: Option<RecoveryVerification>,
}

/// Guardian participation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianParticipation {
    pub guardian_id: GuardianId,
    pub responded: bool,
    pub approved: Option<bool>,
    pub share_provided: bool,
    pub response_time_seconds: Option<u64>,
}

/// Guardian signature for recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianSignature {
    pub guardian_id: GuardianId,
    pub signature: Vec<u8>,
    pub signed_data_hash: [u8; 32],
    pub timestamp: u64,
}

// Implement wire serialization for all message types
impl WireSerializable for RecoveryMessage {}
impl WireSerializable for InitiateRecoveryMessage {}
impl WireSerializable for GuardianApprovalMessage {}
impl WireSerializable for SubmitRecoveryShareMessage {}
impl WireSerializable for RecoveryShareVerification {}
impl WireSerializable for CompleteRecoveryMessage {}
impl WireSerializable for RecoveredShare {}
impl WireSerializable for RecoveryVerification {}
impl WireSerializable for GuardianApproval {}
impl WireSerializable for AbortRecoveryMessage {}
impl WireSerializable for NudgeGuardianMessage {}
impl WireSerializable for RecoveryProtocolResult {}
impl WireSerializable for GuardianParticipation {}
impl WireSerializable for GuardianSignature {}
