// Event types for authenticated CRDT operations
//
// Reference: 080_architecture_protocol_integration.md - Part 3: CRDT Choreography
//
// This module defines all 50+ event types required by the 080 specification,
// organized by protocol:
// - Epoch/Clock Management
// - Distributed Locking
// - P2P DKD Protocol
// - P2P Resharing Protocol
// - Recovery Protocol
// - Compaction Protocol
// - Device/Guardian Management
// - Presence & Policy

use crate::types::*;
use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Event identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub Uuid);

impl EventId {
    pub fn new() -> Self {
        EventId(Uuid::new_v4())
    }
}

impl Default for EventId {
    fn default() -> Self {
        Self::new()
    }
}

/// Protocol version for events
pub const EVENT_VERSION: u16 = 1;

/// Base event structure
///
/// All CRDT events share this structure with a type-specific payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Protocol version (for forward compatibility)
    pub version: u16,
    pub event_id: EventId,
    pub account_id: AccountId,
    pub timestamp: u64,
    /// Nonce to prevent replay attacks (must be unique per account)
    pub nonce: u64,
    /// Parent event hash for causal ordering (None for genesis event)
    pub parent_hash: Option<[u8; 32]>,
    /// Epoch at which this event was written (for logical clock)
    pub epoch_at_write: u64,
    pub event_type: EventType,
    pub authorization: EventAuthorization,
}

impl Event {
    /// Create a new event with current protocol version
    pub fn new(
        account_id: AccountId,
        nonce: u64,
        parent_hash: Option<[u8; 32]>,
        epoch_at_write: u64,
        event_type: EventType,
        authorization: EventAuthorization,
    ) -> Self {
        Event {
            version: EVENT_VERSION,
            event_id: EventId::new(),
            account_id,
            timestamp: crate::state::current_timestamp(),
            nonce,
            parent_hash,
            epoch_at_write,
            event_type,
            authorization,
        }
    }
    
    /// Compute hash of this event for causal chain
    pub fn hash(&self) -> crate::Result<[u8; 32]> {
        // Serialize event to canonical form and hash
        let serialized = crate::serialization::serialize_cbor(self)?;
        Ok(*blake3::hash(&serialized).as_bytes())
    }
    
    /// Compute signable hash (excludes authorization for signing)
    ///
    /// This computes the hash of the event content that should be signed.
    /// The authorization field is excluded to avoid circular dependency.
    pub fn signable_hash(&self) -> crate::Result<[u8; 32]> {
        // Create a struct with all fields except authorization
        use serde::Serialize;
        
        #[derive(Serialize)]
        struct SignableEvent<'a> {
            version: u16,
            event_id: EventId,
            account_id: AccountId,
            timestamp: u64,
            nonce: u64,
            parent_hash: Option<[u8; 32]>,
            epoch_at_write: u64,
            event_type: &'a EventType,
        }
        
        let signable = SignableEvent {
            version: self.version,
            event_id: self.event_id,
            account_id: self.account_id,
            timestamp: self.timestamp,
            nonce: self.nonce,
            parent_hash: self.parent_hash,
            epoch_at_write: self.epoch_at_write,
            event_type: &self.event_type,
        };
        
        let serialized = crate::serialization::serialize_cbor(&signable)?;
        Ok(*blake3::hash(&serialized).as_bytes())
    }
    
    /// Validate event version is supported
    pub fn validate_version(&self) -> Result<(), String> {
        if self.version > EVENT_VERSION {
            return Err(format!(
                "Unsupported event version {} (max supported: {})",
                self.version, EVENT_VERSION
            ));
        }
        Ok(())
    }
    
    /// Validate causal ordering (parent hash matches last event)
    pub fn validate_parent(&self, expected_parent: Option<[u8; 32]>) -> Result<(), String> {
        if self.parent_hash != expected_parent {
            return Err(format!(
                "Invalid parent hash: expected {:?}, got {:?}",
                expected_parent, self.parent_hash
            ));
        }
        Ok(())
    }
}

/// Authorization for an event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventAuthorization {
    /// Threshold signature from M-of-N participants
    ThresholdSignature(ThresholdSig),
    /// Single device certificate signature
    DeviceCertificate {
        device_id: DeviceId,
        #[serde(with = "signature_serde")]
        signature: Signature,
    },
    /// Guardian signature (for recovery approvals)
    GuardianSignature {
        guardian_id: GuardianId,
        #[serde(with = "signature_serde")]
        signature: Signature,
    },
}

mod signature_serde {
    use ed25519_dalek::Signature;
    use serde::{Deserialize, Deserializer, Serializer};
    
    pub fn serialize<S>(sig: &Signature, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&sig.to_bytes())
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Signature, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        Signature::from_slice(&bytes).map_err(serde::de::Error::custom)
    }
}

/// All event types in the Aura system
///
/// Reference: 080 spec - Part 3: CRDT Choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    // ========== Epoch/Clock Management (080 Part 1: Logical Clock) ==========
    /// Advance logical epoch when ledger is idle
    EpochTick(EpochTickEvent),
    
    // ========== Distributed Locking (080 Part 3: Distributed Locking) ==========
    /// Request to acquire distributed lock for critical operation
    RequestOperationLock(RequestOperationLockEvent),
    /// Threshold-granted lock acquisition (M-of-N devices agree)
    GrantOperationLock(GrantOperationLockEvent),
    /// Release distributed lock after operation completes
    ReleaseOperationLock(ReleaseOperationLockEvent),
    
    // ========== P2P DKD Protocol (080 Part 1: P2P DKD Integration) ==========
    /// Initiate new DKD session
    InitiateDkdSession(InitiateDkdSessionEvent),
    /// Record commitment in Phase 1 (before revealing points)
    RecordDkdCommitment(RecordDkdCommitmentEvent),
    /// Reveal point in Phase 2 (after all commitments collected)
    RevealDkdPoint(RevealDkdPointEvent),
    /// Finalize DKD session with derived identity
    FinalizeDkdSession(FinalizeDkdSessionEvent),
    /// Abort DKD session due to timeout or Byzantine behavior
    AbortDkdSession(AbortDkdSessionEvent),
    /// Health check request for stuck participants
    HealthCheckRequest(HealthCheckRequestEvent),
    /// Health check response from participant
    HealthCheckResponse(HealthCheckResponseEvent),
    
    // ========== P2P Resharing Protocol (080 Part 4: P2P Resharing) ==========
    /// Initiate resharing protocol
    InitiateResharing(InitiateResharingEvent),
    /// Distribute encrypted sub-share to new participant
    DistributeSubShare(DistributeSubShareEvent),
    /// Acknowledge receipt of sub-share
    AcknowledgeSubShare(AcknowledgeSubShareEvent),
    /// Finalize resharing with new threshold key
    FinalizeResharing(FinalizeResharingEvent),
    /// Abort resharing due to failure
    AbortResharing(AbortResharingEvent),
    /// Rollback failed resharing to previous state
    ResharingRollback(ResharingRollbackEvent),
    
    // ========== Recovery Protocol (080 Part 2: Recovery Protocol) ==========
    /// Initiate account recovery
    InitiateRecovery(InitiateRecoveryEvent),
    /// Guardian approval for recovery request
    CollectGuardianApproval(CollectGuardianApprovalEvent),
    /// Guardian submits encrypted recovery share
    SubmitRecoveryShare(SubmitRecoveryShareEvent),
    /// Complete recovery with reconstructed identity
    CompleteRecovery(CompleteRecoveryEvent),
    /// Abort recovery (timeout or cancellation)
    AbortRecovery(AbortRecoveryEvent),
    /// Nudge guardian to respond to recovery request
    NudgeGuardian(NudgeGuardianEvent),
    
    // ========== Compaction Protocol (080 Part 3: Ledger Compaction) ==========
    /// Propose ledger compaction with epoch range
    ProposeCompaction(ProposeCompactionEvent),
    /// Acknowledge compaction proposal
    AcknowledgeCompaction(AcknowledgeCompactionEvent),
    /// Commit compaction (threshold-authorized)
    CommitCompaction(CommitCompactionEvent),
    
    // ========== Device/Guardian Management ==========
    /// Add new device to account
    AddDevice(AddDeviceEvent),
    /// Remove device from account
    RemoveDevice(RemoveDeviceEvent),
    /// Add guardian to account
    AddGuardian(AddGuardianEvent),
    /// Remove guardian from account
    RemoveGuardian(RemoveGuardianEvent),
    
    // ========== Presence & Policy ==========
    /// Cache presence ticket for offline verification
    PresenceTicketCache(PresenceTicketCacheEvent),
    /// Update account policy
    PolicyUpdate(PolicyUpdateEvent),
}

// ==================== Event Payload Structures ====================

// ========== Epoch/Clock Management ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochTickEvent {
    pub new_epoch: u64,
    pub evidence_hash: [u8; 32], // Hash of latest CRDT snapshot
}

// ========== Distributed Locking ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestOperationLockEvent {
    pub operation_type: OperationType,
    pub session_id: Uuid,
    pub device_id: DeviceId,
    pub lottery_ticket: [u8; 32], // Hash(device_id || last_event_hash)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrantOperationLockEvent {
    pub operation_type: OperationType,
    pub session_id: Uuid,
    pub winner_device_id: DeviceId,
    pub granted_at_epoch: u64,
    /// Threshold signature from M-of-N devices
    pub threshold_signature: ThresholdSig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseOperationLockEvent {
    pub operation_type: OperationType,
    pub session_id: Uuid,
    pub device_id: DeviceId,
}

// ========== P2P DKD Protocol ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitiateDkdSessionEvent {
    pub session_id: Uuid,
    pub context_id: Vec<u8>,
    pub threshold: u16,
    pub participants: Vec<DeviceId>,
    pub start_epoch: u64,
    pub ttl_in_epochs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordDkdCommitmentEvent {
    pub session_id: Uuid,
    pub device_id: DeviceId,
    pub commitment: [u8; 32], // blake3(Point)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevealDkdPointEvent {
    pub session_id: Uuid,
    pub device_id: DeviceId,
    pub point: Vec<u8>, // Compressed Edwards point (32 bytes)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalizeDkdSessionEvent {
    pub session_id: Uuid,
    pub seed_fingerprint: [u8; 32],
    pub commitment_root: [u8; 32], // Merkle root of all commitments
    pub derived_identity_pk: Vec<u8>, // Public key derived from seed
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbortDkdSessionEvent {
    pub session_id: Uuid,
    pub reason: DkdAbortReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DkdAbortReason {
    Timeout,
    ByzantineBehavior { device_id: DeviceId, details: String },
    CollisionDetected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckRequestEvent {
    pub session_id: Uuid,
    pub target_device_id: DeviceId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResponseEvent {
    pub session_id: Uuid,
    pub device_id: DeviceId,
    pub status: HealthStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded { reason: String },
    Offline,
}

// ========== P2P Resharing Protocol ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitiateResharingEvent {
    pub session_id: Uuid,
    pub old_threshold: u16,
    pub new_threshold: u16,
    pub old_participants: Vec<DeviceId>,
    pub new_participants: Vec<DeviceId>,
    pub start_epoch: u64,
    pub ttl_in_epochs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributeSubShareEvent {
    pub session_id: Uuid,
    pub from_device_id: DeviceId,
    pub to_device_id: DeviceId,
    pub encrypted_sub_share: Vec<u8>, // HPKE ciphertext
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeSubShareEvent {
    pub session_id: Uuid,
    pub from_device_id: DeviceId,
    pub to_device_id: DeviceId,
    pub ack_signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalizeResharingEvent {
    pub session_id: Uuid,
    pub new_group_public_key: Vec<u8>,
    pub new_threshold: u16,
    pub test_signature: Vec<u8>, // Proof that new shares work
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbortResharingEvent {
    pub session_id: Uuid,
    pub reason: ResharingAbortReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResharingAbortReason {
    Timeout,
    DeliveryFailure { missing_acks: Vec<(DeviceId, DeviceId)> },
    TestSignatureFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResharingRollbackEvent {
    pub session_id: Uuid,
    pub rollback_to_epoch: u64,
}

// ========== Recovery Protocol ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitiateRecoveryEvent {
    pub recovery_id: Uuid,
    pub new_device_id: DeviceId,
    pub new_device_pk: Vec<u8>,
    pub required_guardians: Vec<GuardianId>,
    pub quorum_threshold: u16,
    pub cooldown_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectGuardianApprovalEvent {
    pub recovery_id: Uuid,
    pub guardian_id: GuardianId,
    pub approved: bool,
    pub approval_signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitRecoveryShareEvent {
    pub recovery_id: Uuid,
    pub guardian_id: GuardianId,
    pub encrypted_share: Vec<u8>, // HPKE with AAD
    pub merkle_proof: MerkleProof,
    pub dkd_session_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteRecoveryEvent {
    pub recovery_id: Uuid,
    pub new_device_id: DeviceId,
    pub test_signature: Vec<u8>, // Proof that recovered identity works
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbortRecoveryEvent {
    pub recovery_id: Uuid,
    pub reason: RecoveryAbortReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryAbortReason {
    Timeout,
    InsufficientApprovals,
    VerificationFailed,
    UserCancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NudgeGuardianEvent {
    pub recovery_id: Uuid,
    pub guardian_id: GuardianId,
}

// ========== Compaction Protocol ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposeCompactionEvent {
    pub compaction_id: Uuid,
    pub compact_before_epoch: u64,
    pub commitment_roots_to_preserve: Vec<Uuid>, // DKD session IDs
    pub proposer_device_id: DeviceId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeCompactionEvent {
    pub compaction_id: Uuid,
    pub device_id: DeviceId,
    pub ack_signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitCompactionEvent {
    pub compaction_id: Uuid,
    pub compacted_before_epoch: u64,
    pub preserved_commitment_roots: Vec<DkdCommitmentRoot>,
    pub threshold_signature: ThresholdSig,
}

// ========== Device/Guardian Management ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddDeviceEvent {
    pub device_id: DeviceId,
    pub device_name: String,
    pub device_type: DeviceType,
    pub public_key: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveDeviceEvent {
    pub device_id: DeviceId,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddGuardianEvent {
    pub guardian_id: GuardianId,
    pub contact_info: ContactInfo,
    pub encrypted_share_cid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveGuardianEvent {
    pub guardian_id: GuardianId,
    pub reason: String,
}

// ========== Presence & Policy ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceTicketCacheEvent {
    pub device_id: DeviceId,
    pub ticket_digest: [u8; 32],
    pub issued_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyUpdateEvent {
    pub policy_cid: Cid,
    pub version: u64,
}

/// Get current unix timestamp in seconds
pub fn current_timestamp() -> crate::Result<u64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| crate::LedgerError::SerializationFailed(format!(
            "System time is before UNIX epoch: {}",
            e
        )))
}

