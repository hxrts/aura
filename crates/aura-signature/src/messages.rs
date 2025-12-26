//! Layer 2: Verification Message Types
//!
//! Message types supporting cryptographic verification operations:
//! threshold signatures (FROST), key resharing/rotation protocols.
//!
//! **Organization**:
//! - Cryptographic protocol messages (resharing, FROST, future DKD)
//!
//! All messages use aura-core message envelope (Layer 1) for versioning and serialization safety.
//!
//! **Authority Model**: Protocol participants are identified by `AuthorityId` rather than
//! device-level identifiers. This aligns with the authority-centric identity model where
//! authorities hide their internal device structure from external parties.

// ============================================================================
// Cryptographic Protocol Messages
// ============================================================================

use aura_core::identifiers::{AuthorityId, SessionId};
use serde::{Deserialize, Serialize};

/// Unified cryptographic protocol message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoMessage {
    /// Session this message belongs to
    pub session_id: SessionId,
    /// Authority that sent this message
    pub sender: AuthorityId,
    /// Message sequence number within session
    pub sequence: u64,
    /// Timestamp when message was created
    pub timestamp: u64,
    /// The actual crypto protocol payload
    pub payload: CryptoPayload,
}

/// Union of all cryptographic protocol payloads
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CryptoPayload {
    /// DKD protocol messages - Use future aura-dkd crate for DKD operations
    // Dkd(DkdMessage), // REMOVED: DKD messages moved to future aura-dkd feature crate
    /// Resharing protocol messages
    Resharing(ResharingMessage),
}

impl CryptoMessage {
    /// Create a new crypto message
    pub fn new(
        session_id: SessionId,
        sender: AuthorityId,
        sequence: u64,
        timestamp: u64,
        payload: CryptoPayload,
    ) -> Self {
        Self {
            session_id,
            sender,
            sequence,
            timestamp,
            payload,
        }
    }

    /// Get the protocol type for this message
    pub fn protocol_type(&self) -> &'static str {
        match &self.payload {
            // CryptoPayload::Dkd(_) => "dkd", // REMOVED: DKD messages moved to future aura-dkd feature crate
            CryptoPayload::Resharing(_) => "resharing",
        }
    }
}

// ============================================================================
// Key Resharing Protocol Messages (formerly crypto/resharing.rs)
// ============================================================================

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
    /// Authorities participating in the old threshold scheme
    pub old_participants: Vec<AuthorityId>,
    /// Authorities participating in the new threshold scheme
    pub new_participants: Vec<AuthorityId>,
    pub start_epoch: u64,
    pub ttl_in_epochs: u64,
    pub resharing_context: Vec<u8>,
}

/// Sub-share distribution message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributeSubShareMessage {
    pub session_id: SessionId,
    /// Sending authority
    pub from_authority: AuthorityId,
    /// Receiving authority
    pub to_authority: AuthorityId,
    pub encrypted_sub_share: Vec<u8>, // HPKE ciphertext
    pub share_index: u16,
    pub commitment_proof: Vec<u8>,
}

/// Sub-share acknowledgment message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeSubShareMessage {
    pub session_id: SessionId,
    /// Sending authority (acknowledger)
    pub from_authority: AuthorityId,
    /// Authority that sent the sub-share being acknowledged
    pub to_authority: AuthorityId,
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
    /// Commitments from each participating authority
    pub participant_commitments: Vec<(AuthorityId, Vec<u8>)>,
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
    /// Authority that participated in resharing
    pub authority_id: AuthorityId,
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
    /// Authorities that failed during the resharing protocol
    pub failed_participants: Vec<AuthorityId>,
    pub error_details: Option<String>,
}

/// Reasons for resharing abort
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResharingAbortReason {
    Timeout,
    DeliveryFailure {
        /// (sender, recipient) pairs that failed acknowledgment
        missing_acks: Vec<(AuthorityId, AuthorityId)>,
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
    /// Authorities affected by the rollback
    pub affected_participants: Vec<AuthorityId>,
}

/// Resharing protocol result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResharingProtocolResult {
    pub session_id: SessionId,
    pub success: bool,
    pub new_group_public_key: Option<Vec<u8>>,
    pub new_threshold: Option<u16>,
    /// Authorities that successfully completed resharing
    pub new_participants: Vec<AuthorityId>,
    pub verification: Option<ResharingVerification>,
}

/// Encrypted share data for resharing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedShare {
    /// Recipient authority for this share
    pub recipient_authority: AuthorityId,
    pub encrypted_data: Vec<u8>, // HPKE encrypted share
    pub sender_proof: Vec<u8>,
    pub share_commitment: Vec<u8>,
}

// All message types use standard serde traits for serialization
