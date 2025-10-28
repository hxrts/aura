//! Protocol Results with Ledger Mutations and Capability Verification
//!
//! This module defines the result types returned by protocols that include
//! the canonical commit payloads, ledger mutations, and threshold capability
//! proofs for authorization.

use crate::ThresholdSignature;
use aura_types::{AuraError, AuraResult as Result};
use aura_journal::capability::{Epoch, GroupRoster, KeyhiveCgkaOperation};
use aura_journal::{
    capability::{unified_manager::VerificationContext, ThresholdCapability},
    events::RelationshipId,
    Event, OperationType as JournalOperationType, SessionId,
};
use aura_types::{DeviceId, GuardianId};
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};

/// Capability proof for protocol authorization
///
/// Contains threshold-signed capability tokens proving that protocol
/// participants have proper authorization for the operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityProof {
    /// Primary capability authorizing the operation
    pub primary_capability: ThresholdCapability,
    /// Additional capabilities from other participants
    pub participant_capabilities: Vec<ThresholdCapability>,
    /// Verification context from capability system
    pub verification_context: VerificationContext,
    /// Administrative operation flag
    pub requires_admin: bool,
}

impl CapabilityProof {
    /// Create new capability proof
    pub fn new(
        primary_capability: ThresholdCapability,
        participant_capabilities: Vec<ThresholdCapability>,
        verification_context: VerificationContext,
        requires_admin: bool,
    ) -> Self {
        Self {
            primary_capability,
            participant_capabilities,
            verification_context,
            requires_admin,
        }
    }

    /// Get all capabilities in the proof
    pub fn all_capabilities(&self) -> Vec<&ThresholdCapability> {
        let mut caps = vec![&self.primary_capability];
        caps.extend(self.participant_capabilities.iter());
        caps
    }

    /// Get total authority level across all capabilities
    pub fn total_authority(&self) -> u32 {
        self.all_capabilities()
            .iter()
            .map(|cap| cap.authority_level() as u32)
            .sum()
    }

    /// Check if proof meets minimum authority requirements
    pub fn meets_authority_threshold(&self, minimum: u32) -> bool {
        self.verification_context.authority_level >= minimum
    }
}

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
    /// Capability proof authorizing this operation
    pub capability_proof: CapabilityProof,
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
            capability_proof: self.capability_proof.clone(),
        }
    }

    /// Verify authorization for this protocol result
    pub fn verify_authorization(&self) -> Result<()> {
        // Verify primary capability signature
        self.capability_proof
            .primary_capability
            .verify_signature()
            .map_err(|e| {
                AuraError::insufficient_capability(format!(
                    "Primary capability verification failed: {}",
                    e
                ))
            })?;

        // Verify participant capabilities
        for cap in &self.capability_proof.participant_capabilities {
            cap.verify_signature().map_err(|e| {
                AuraError::insufficient_capability(format!(
                    "Participant capability verification failed: {}",
                    e
                ))
            })?;
        }

        // Check authority threshold for admin operations
        if self.capability_proof.requires_admin
            && !self.capability_proof.meets_authority_threshold(2)
        {
            return Err(AuraError::insufficient_capability(
                "Insufficient authority for administrative DKD operation",
            ));
        }

        Ok(())
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
    pub capability_proof: CapabilityProof,
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
    /// Capability proof authorizing this resharing operation
    pub capability_proof: CapabilityProof,
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
            capability_proof: self.capability_proof.clone(),
        }
    }

    /// Verify authorization for this resharing operation
    pub fn verify_authorization(&self) -> Result<()> {
        // Resharing is always administrative - requires high authority
        if !self.capability_proof.meets_authority_threshold(3) {
            return Err(AuraError::insufficient_capability(
                "Insufficient authority for resharing operation - requires administrative privileges"
            ));
        }

        // Verify all capability signatures
        for cap in self.capability_proof.all_capabilities() {
            cap.verify_signature().map_err(|e| {
                AuraError::insufficient_capability(format!("Capability verification failed: {}", e))
            })?;
        }

        Ok(())
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
    pub capability_proof: CapabilityProof,
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
    /// Capability proof authorizing this recovery operation
    pub capability_proof: CapabilityProof,
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
            capability_proof: self.capability_proof.clone(),
        }
    }

    /// Verify authorization for this recovery operation
    pub fn verify_authorization(&self) -> Result<()> {
        // Recovery is administrative - requires high authority from guardians
        if !self.capability_proof.meets_authority_threshold(2) {
            return Err(AuraError::insufficient_capability(
                "Insufficient authority for recovery operation",
            ));
        }

        // Verify all capability signatures
        for cap in self.capability_proof.all_capabilities() {
            cap.verify_signature().map_err(|e| {
                AuraError::insufficient_capability(format!("Capability verification failed: {}", e))
            })?;
        }

        Ok(())
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
    pub capability_proof: CapabilityProof,
}

/// Result of Locking protocol execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockingProtocolResult {
    /// Session ID for this locking operation
    pub session_id: SessionId,
    /// Operation type governed by the lock
    pub operation_type: JournalOperationType,
    /// Winner of the deterministic lottery
    pub winner: DeviceId,
    /// Whether the lock was granted
    pub granted: bool,
    /// Threshold signature attesting to quorum approval
    pub threshold_signature: ThresholdSignature,
    /// Ledger events to persist as part of the lock
    pub ledger_events: Vec<Event>,
    /// Participants that contributed to the decision
    pub participants: Vec<DeviceId>,
    /// Capability proof authorizing this locking operation
    pub capability_proof: CapabilityProof,
}

impl LockingProtocolResult {
    /// Produce the canonical commit payload for ledger persistence.
    pub fn commit_payload(&self) -> LockingCommitPayload {
        LockingCommitPayload {
            session_id: self.session_id,
            operation_type: self.operation_type,
            winner: self.winner,
            participants: self.participants.clone(),
            capability_proof: self.capability_proof.clone(),
        }
    }

    /// Verify authorization for this locking operation
    pub fn verify_authorization(&self) -> Result<()> {
        // Verify capability signatures
        for cap in self.capability_proof.all_capabilities() {
            cap.verify_signature().map_err(|e| {
                AuraError::insufficient_capability(format!("Capability verification failed: {}", e))
            })?
        }

        Ok(())
    }
}

/// Canonical locking commit payload for ledger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockingCommitPayload {
    pub session_id: SessionId,
    pub operation_type: JournalOperationType,
    pub winner: DeviceId,
    pub participants: Vec<DeviceId>,
    pub capability_proof: CapabilityProof,
}

/// Result of Counter reservation execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CounterProtocolResult {
    /// Session ID for the reservation
    pub session_id: SessionId,
    /// Relationship the counter belongs to
    pub relationship_id: RelationshipId,
    /// Device requesting the reservation
    pub requesting_device: DeviceId,
    /// Reserved counter values (single or range)
    pub reserved_values: Vec<u64>,
    /// TTL for the reservation in epochs
    pub ttl_epochs: u64,
    /// Ledger events to persist
    pub ledger_events: Vec<Event>,
    /// Participants involved in authorization
    pub participants: Vec<DeviceId>,
    /// Capability proof authorizing this counter operation
    pub capability_proof: CapabilityProof,
}

impl CounterProtocolResult {
    /// Return the first reserved value when a single count is requested.
    pub fn primary_value(&self) -> Option<u64> {
        self.reserved_values.first().copied()
    }
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

    /// Write group result to ledger
    async fn write_group_result(&mut self, result: &GroupProtocolResult) -> Result<()>;
}

/// Result of Group protocol execution including CGKA operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupProtocolResult {
    /// Session ID for this group operation
    pub session_id: SessionId,
    /// Group identifier
    pub group_id: String,
    /// Current group epoch after operation
    pub epoch: Epoch,
    /// Current group roster after operation
    pub roster: GroupRoster,
    /// CGKA operations performed
    pub cgka_operations: Vec<KeyhiveCgkaOperation>,
    /// Events to be written to ledger
    pub ledger_events: Vec<Event>,
    /// Participants who contributed
    pub participants: Vec<DeviceId>,
    /// Capability proof authorizing this group operation
    pub capability_proof: CapabilityProof,
}

impl GroupProtocolResult {
    /// Get the canonical commit payload for ledger
    pub fn commit_payload(&self) -> GroupCommitPayload {
        GroupCommitPayload {
            session_id: self.session_id,
            group_id: self.group_id.clone(),
            epoch: self.epoch,
            roster: self.roster.clone(),
            cgka_operations: self.cgka_operations.clone(),
            participants: self.participants.clone(),
            capability_proof: self.capability_proof.clone(),
        }
    }

    /// Verify authorization for this group operation
    pub fn verify_authorization(&self) -> Result<()> {
        // Group operations require authentication
        for cap in self.capability_proof.all_capabilities() {
            cap.verify_signature().map_err(|e| {
                AuraError::insufficient_capability(format!("Capability verification failed: {}", e))
            })?
        }

        Ok(())
    }
}

/// Canonical Group commit payload for ledger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupCommitPayload {
    pub session_id: SessionId,
    pub group_id: String,
    pub epoch: Epoch,
    pub roster: GroupRoster,
    pub cgka_operations: Vec<KeyhiveCgkaOperation>,
    pub participants: Vec<DeviceId>,
    pub capability_proof: CapabilityProof,
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_dkd_commit_payload() {
        use aura_journal::capability::unified_manager::VerificationContext;
        use aura_journal::capability::ThresholdCapability;
        
        let capability_proof = CapabilityProof {
            primary_capability: ThresholdCapability::new_test_capability(),
            participant_capabilities: vec![],
            verification_context: VerificationContext::new_test_context(),
            requires_admin: false,
        };
        
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
            capability_proof,
        };

        let payload = result.commit_payload();
        assert_eq!(payload.session_id, result.session_id);
        assert_eq!(payload.derived_public_key, result.derived_public_key);
    }
}
