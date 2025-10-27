//! Protocol Results with Ledger Mutations
//!
//! This module defines the result types returned by protocols that include
//! the canonical commit payloads and ledger mutations.

use crate::ThresholdSignature;
use aura_errors::Result;
use aura_types::{DeviceId, GuardianId};
use aura_journal::{Event, SessionId};
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};

/// Result of DKD protocol execution including commit payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdProtocolResult {
    /// Session ID for this DKD execution
    pub session_id: SessionId,
    /// Derived key bytes
    pub derived_key: Vec<u8>,
    /// Derived public key
    pub derived_public_key: VerifyingKey,
    /// Session transcript hash for verification
    pub transcript_hash: [u8; 32],
    /// Threshold signature over the derived key
    pub threshold_signature: ThresholdSignature,
    /// Events to be written to ledger
    pub ledger_events: Vec<Event>,
    /// Participants who contributed
    pub participants: Vec<DeviceId>,
}

impl DkdProtocolResult {
    /// Get the canonical commit payload for ledger
    pub fn commit_payload(&self) -> DkdCommitPayload {
        DkdCommitPayload {
            session_id: self.session_id,
            derived_public_key: self.derived_public_key,
            transcript_hash: self.transcript_hash,
            threshold_signature: self.threshold_signature.clone(),
            participants: self.participants.clone(),
        }
    }
}

/// Canonical DKD commit payload for ledger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdCommitPayload {
    pub session_id: SessionId,
    pub derived_public_key: VerifyingKey,
    pub transcript_hash: [u8; 32],
    pub threshold_signature: ThresholdSignature,
    pub participants: Vec<DeviceId>,
}

/// Result of Resharing protocol execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResharingProtocolResult {
    /// Session ID for this resharing
    pub session_id: SessionId,
    /// New threshold configuration
    pub new_threshold: u16,
    /// New participant set
    pub new_participants: Vec<DeviceId>,
    /// Old participant set
    pub old_participants: Vec<DeviceId>,
    /// New encrypted shares for each participant
    pub new_shares: Vec<EncryptedShare>,
    /// Threshold signature from old participants approving resharing
    pub approval_signature: ThresholdSignature,
    /// Events to be written to ledger
    pub ledger_events: Vec<Event>,
}

impl ResharingProtocolResult {
    /// Get the canonical commit payload for ledger
    pub fn commit_payload(&self) -> ResharingCommitPayload {
        ResharingCommitPayload {
            session_id: self.session_id,
            new_threshold: self.new_threshold,
            new_participants: self.new_participants.clone(),
            old_participants: self.old_participants.clone(),
            share_commitments: self.new_shares.iter().map(|s| s.commitment).collect(),
            approval_signature: self.approval_signature.clone(),
        }
    }
}

/// Canonical resharing commit payload for ledger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResharingCommitPayload {
    pub session_id: SessionId,
    pub new_threshold: u16,
    pub new_participants: Vec<DeviceId>,
    pub old_participants: Vec<DeviceId>,
    pub share_commitments: Vec<[u8; 32]>,
    pub approval_signature: ThresholdSignature,
}

/// Encrypted share for a participant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedShare {
    pub recipient: DeviceId,
    pub encrypted_share: Vec<u8>,
    pub commitment: [u8; 32],
}

/// Result of Recovery protocol execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryProtocolResult {
    /// Session ID for this recovery
    pub session_id: SessionId,
    /// New device being recovered to
    pub new_device_id: DeviceId,
    /// Guardians who approved recovery
    pub approving_guardians: Vec<GuardianId>,
    /// Guardian signatures over recovery request
    pub guardian_signatures: Vec<GuardianSignature>,
    /// Reconstructed key share for new device
    pub recovered_share: Vec<u8>,
    /// Revocation proof for old devices (if applicable)
    pub revocation_proof: Option<RevocationProof>,
    /// Events to be written to ledger
    pub ledger_events: Vec<Event>,
}

impl RecoveryProtocolResult {
    /// Get the canonical commit payload for ledger
    pub fn commit_payload(&self) -> RecoveryCommitPayload {
        RecoveryCommitPayload {
            session_id: self.session_id,
            new_device_id: self.new_device_id,
            approving_guardians: self.approving_guardians.clone(),
            guardian_signatures: self.guardian_signatures.clone(),
            revocation_proof: self.revocation_proof.clone(),
        }
    }
}

/// Canonical recovery commit payload for ledger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryCommitPayload {
    pub session_id: SessionId,
    pub new_device_id: DeviceId,
    pub approving_guardians: Vec<GuardianId>,
    pub guardian_signatures: Vec<GuardianSignature>,
    pub revocation_proof: Option<RevocationProof>,
}

/// Guardian signature over recovery request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianSignature {
    pub guardian_id: GuardianId,
    pub signature: Vec<u8>,
    pub signed_at: u64,
}

/// Proof that old devices have been revoked
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevocationProof {
    pub revoked_devices: Vec<DeviceId>,
    pub threshold_signature: ThresholdSignature,
    pub revoked_at: u64,
}

/// Extension trait for writing protocol results to ledger
#[async_trait::async_trait]
pub trait ProtocolResultWriter {
    /// Write DKD result to ledger
    async fn write_dkd_result(&mut self, result: &DkdProtocolResult) -> Result<()>;

    /// Write resharing result to ledger
    async fn write_resharing_result(&mut self, result: &ResharingProtocolResult) -> Result<()>;

    /// Write recovery result to ledger
    async fn write_recovery_result(&mut self, result: &RecoveryProtocolResult) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_dkd_commit_payload() {
        let result = DkdProtocolResult {
            session_id: SessionId(Uuid::new_v4()),
            derived_key: vec![1, 2, 3, 4],
            derived_public_key: VerifyingKey::from_bytes(&[1u8; 32]).unwrap(),
            transcript_hash: [0u8; 32],
            threshold_signature: ThresholdSignature {
                signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                signers: vec![],
            },
            ledger_events: vec![],
            participants: vec![],
        };

        let payload = result.commit_payload();
        assert_eq!(payload.session_id, result.session_id);
        assert_eq!(payload.derived_public_key, result.derived_public_key);
    }
}
