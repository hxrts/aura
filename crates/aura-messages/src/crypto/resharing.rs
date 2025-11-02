//! Key Resharing Protocol Messages
//!
//! Messages used in threshold key resharing for updating participant sets
//! and threshold configurations.

use aura_types::{DeviceId, SessionId};
use serde::{Deserialize, Serialize};

/// Resharing protocol message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResharingMessage {
    /// Initiate key resharing
    InitiateResharing(InitiateResharingMessage),
    /// Distribute sub-share to new participant
    DistributeSubShare(DistributeSubShareMessage),
    /// Acknowledge receipt of sub-share
    AcknowledgeSubShare(AcknowledgeSubShareMessage),
    /// Finalize resharing with new threshold key
    FinalizeResharing(FinalizeResharingMessage),
    /// Abort resharing due to failure
    AbortResharing(AbortResharingMessage),
    /// Rollback failed resharing to previous state
    RollbackResharing(RollbackResharingMessage),
}

/// Initiate resharing protocol message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitiateResharingMessage {
    pub session_id: SessionId,
    pub old_threshold: u16,
    pub new_threshold: u16,
    pub old_participants: Vec<DeviceId>,
    pub new_participants: Vec<DeviceId>,
    pub start_epoch: u64,
    pub ttl_in_epochs: u64,
    pub resharing_context: Vec<u8>,
}

/// Sub-share distribution message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributeSubShareMessage {
    pub session_id: SessionId,
    pub from_device_id: DeviceId,
    pub to_device_id: DeviceId,
    pub encrypted_sub_share: Vec<u8>, // HPKE ciphertext
    pub share_index: u16,
    pub commitment_proof: Vec<u8>,
}

/// Sub-share acknowledgment message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeSubShareMessage {
    pub session_id: SessionId,
    pub from_device_id: DeviceId,
    pub to_device_id: DeviceId,
    pub ack_signature: Vec<u8>,
    pub share_verification: bool,
    pub error_message: Option<String>,
}

/// Resharing finalization message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalizeResharingMessage {
    pub session_id: SessionId,
    pub new_group_public_key: Vec<u8>,
    pub new_threshold: u16,
    pub test_signature: Vec<u8>, // Proof that new shares work
    pub participant_commitments: Vec<(DeviceId, Vec<u8>)>,
    pub verification_data: ResharingVerification,
}

/// Resharing verification data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResharingVerification {
    pub all_shares_received: bool,
    pub test_signature_valid: bool,
    pub participant_verifications: Vec<ParticipantResharingVerification>,
    pub new_threshold_achieved: bool,
}

/// Per-participant resharing verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantResharingVerification {
    pub device_id: DeviceId,
    pub shares_sent: u16,
    pub shares_received: u16,
    pub verification_successful: bool,
    pub error_details: Option<String>,
}

/// Resharing abort message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbortResharingMessage {
    pub session_id: SessionId,
    pub reason: ResharingAbortReason,
    pub failed_participants: Vec<DeviceId>,
    pub error_details: Option<String>,
}

/// Reasons for resharing abort
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResharingAbortReason {
    Timeout,
    DeliveryFailure {
        missing_acks: Vec<(DeviceId, DeviceId)>,
    },
    TestSignatureFailed,
    InsufficientParticipants,
    InvalidShares,
    CommunicationFailure,
    ByzantineBehavior,
}

/// Resharing rollback message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackResharingMessage {
    pub session_id: SessionId,
    pub rollback_to_epoch: u64,
    pub reason: String,
    pub affected_participants: Vec<DeviceId>,
}

/// Resharing protocol result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResharingProtocolResult {
    pub session_id: SessionId,
    pub success: bool,
    pub new_group_public_key: Option<Vec<u8>>,
    pub new_threshold: Option<u16>,
    pub new_participants: Vec<DeviceId>,
    pub verification: Option<ResharingVerification>,
}

/// Encrypted share data for resharing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedShare {
    pub recipient_device_id: DeviceId,
    pub encrypted_data: Vec<u8>, // HPKE encrypted share
    pub sender_proof: Vec<u8>,
    pub share_commitment: Vec<u8>,
}

// All message types use standard serde traits for serialization
