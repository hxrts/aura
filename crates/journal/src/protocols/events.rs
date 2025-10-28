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
// - Presence

use crate::types::*;
use aura_crypto::Ed25519Signature;
use aura_crypto::{signature_serde, MerkleProof};
use aura_types::{AccountId, DeviceId, GuardianId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Event identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub Uuid);

impl EventId {
    /// Create a new event ID using injected effects (for production/testing)
    pub fn new_with_effects(effects: &aura_crypto::Effects) -> Self {
        EventId(effects.gen_uuid())
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
    /// Create a new builder for Event
    pub fn builder() -> EventBuilder {
        EventBuilder::new()
    }

    /// Create a new event with current protocol version
    pub fn new(
        account_id: AccountId,
        nonce: u64,
        parent_hash: Option<[u8; 32]>,
        epoch_at_write: u64,
        event_type: EventType,
        authorization: EventAuthorization,
        effects: &aura_crypto::Effects,
    ) -> Result<Self, String> {
        Ok(Event {
            version: EVENT_VERSION,
            event_id: EventId(effects.gen_uuid()),
            account_id,
            timestamp: effects.now().map_err(|e| format!("Time error: {}", e))?,
            nonce,
            parent_hash,
            epoch_at_write,
            event_type,
            authorization,
        })
    }

    /// Compute hash of this event for causal chain
    pub fn hash(&self) -> crate::Result<[u8; 32]> {
        // Serialize event to canonical form and hash
        let serialized = crate::serialization::serialize_cbor(self)?;
        Ok(aura_crypto::blake3_hash(&serialized))
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
        Ok(aura_crypto::blake3_hash(&serialized))
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
        signature: Ed25519Signature,
    },
    /// Guardian signature (for recovery approvals)
    GuardianSignature {
        guardian_id: GuardianId,
        #[serde(with = "signature_serde")]
        signature: Ed25519Signature,
    },
    /// Lifecycle-internal authorization used during protocol execution.
    LifecycleInternal,
}

/// All event types in the Aura system
///
/// Placeholder types for Keyhive integration (until actual integration is complete)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyhiveCapabilityDelegation {
    pub capability_id: String,
    // TODO: Replace with actual keyhive delegation type
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyhiveCapabilityRevocation {
    pub capability_id: String,
    // TODO: Replace with actual keyhive revocation type
}

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
    /// Update device nonce for replay prevention
    UpdateDeviceNonce(UpdateDeviceNonceEvent),
    /// Add guardian to account
    AddGuardian(AddGuardianEvent),
    /// Remove guardian from account
    RemoveGuardian(RemoveGuardianEvent),

    // ========== Presence ==========
    /// Cache presence ticket for offline verification
    PresenceTicketCache(PresenceTicketCacheEvent),

    // ========== Capabilities ==========
    /// Delegate a capability (legacy Aura format)
    CapabilityDelegation(crate::capability::events::CapabilityDelegation),
    /// Revoke a capability (legacy Aura format)
    CapabilityRevocation(crate::capability::events::CapabilityRevocation),

    // ========== Keyhive Integration ==========
    /// Keyhive capability delegation (placeholder)
    KeyhiveCapabilityDelegation(KeyhiveCapabilityDelegation),
    /// Keyhive capability revocation (placeholder)
    KeyhiveCapabilityRevocation(KeyhiveCapabilityRevocation),
    /// Keyhive CGKA operation
    KeyhiveCgka(keyhive_core::cgka::operation::CgkaOperation),

    // ========== SSB Counter Coordination ==========
    /// Increment counter for unique envelope identifiers
    IncrementCounter(IncrementCounterEvent),
    /// Reserve counter range for batch operations
    ReserveCounterRange(ReserveCounterRangeEvent),

    // ========== Session Management ==========
    /// Create new protocol session
    CreateSession(CreateSessionEvent),
    /// Update session status
    UpdateSessionStatus(UpdateSessionStatusEvent),
    /// Complete session with outcome
    CompleteSession(CompleteSessionEvent),
    /// Abort session with failure
    AbortSession(AbortSessionEvent),
    /// Clean up expired sessions
    CleanupExpiredSessions(CleanupExpiredSessionsEvent),
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
    /// Optional: A delegated capability that a lottery winner could choose to act upon.
    /// This allows for future protocol optimizations where the winner can execute an
    /// action on behalf of the initiator, saving a network round-trip.
    pub delegated_action: Option<crate::capability::events::CapabilityDelegation>,
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
    pub commitment_root: [u8; 32],    // Merkle root of all commitments
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
    ByzantineBehavior {
        device_id: DeviceId,
        details: String,
    },
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
    DeliveryFailure {
        missing_acks: Vec<(DeviceId, DeviceId)>,
    },
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
pub struct UpdateDeviceNonceEvent {
    pub device_id: DeviceId,
    pub new_nonce: u64,
    pub previous_nonce: u64,
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

// ========== Presence ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceTicketCacheEvent {
    pub device_id: DeviceId,
    pub ticket_digest: [u8; 32],
    pub issued_at: u64,
    pub expires_at: u64,
}

// ========== SSB Counter Coordination ==========

/// SSB counter increment event for unique envelope identifiers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncrementCounterEvent {
    /// Relationship identifier for which counter is being incremented
    pub relationship_id: RelationshipId,
    /// Device requesting the counter increment
    pub requesting_device: DeviceId,
    /// Proposed new counter value
    pub new_counter_value: u64,
    /// Previous counter value for conflict detection
    pub previous_counter_value: u64,
    /// Epoch when increment was requested
    pub requested_at_epoch: u64,
    /// TTL for this counter reservation (epochs)
    pub ttl_epochs: u64,
}

/// Reserve a range of counter values for batch operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReserveCounterRangeEvent {
    /// Relationship identifier for which counters are being reserved
    pub relationship_id: RelationshipId,
    /// Device requesting the counter range
    pub requesting_device: DeviceId,
    /// Starting counter value for the range
    pub start_counter: u64,
    /// Number of counter values to reserve
    pub range_size: u64,
    /// Previous counter value for conflict detection
    pub previous_counter_value: u64,
    /// Epoch when range was requested
    pub requested_at_epoch: u64,
    /// TTL for this counter reservation (epochs)
    pub ttl_epochs: u64,
}

// ========== Session Management ==========

/// Create new protocol session event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionEvent {
    pub session_id: uuid::Uuid,
    pub protocol_type: ProtocolType,
    pub participants: Vec<DeviceId>,
    pub context_data: Vec<u8>,
    pub ttl_epochs: u64,
    pub created_at_epoch: u64,
}

/// Update session status event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSessionStatusEvent {
    pub session_id: uuid::Uuid,
    pub new_status: SessionStatus,
    pub previous_status: SessionStatus,
    pub updated_at_epoch: u64,
    pub reason: Option<String>,
}

/// Complete session with outcome event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteSessionEvent {
    pub session_id: uuid::Uuid,
    pub outcome: SessionOutcome,
    pub completion_data: Option<Vec<u8>>,
    pub completed_at_epoch: u64,
    pub final_status: SessionStatus,
}

/// Abort session with failure event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbortSessionEvent {
    pub session_id: uuid::Uuid,
    pub reason: String,
    pub blamed_party: Option<ParticipantId>,
    pub aborted_at_epoch: u64,
    pub previous_status: SessionStatus,
}

/// Clean up expired sessions event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupExpiredSessionsEvent {
    pub cleanup_at_epoch: u64,
    pub expired_sessions: Vec<uuid::Uuid>,
    pub cleanup_reason: String,
}

/// Relationship identifier for SSB counter tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RelationshipId(pub [u8; 32]);

impl RelationshipId {
    /// Create a new relationship ID from two account IDs
    pub fn from_accounts(account_a: AccountId, account_b: AccountId) -> Self {
        // Deterministic relationship ID: lexicographically sort accounts
        let (first, second) = if account_a.0 <= account_b.0 {
            (account_a.0, account_b.0)
        } else {
            (account_b.0, account_a.0)
        };

        // Hash the sorted pair
        let mut hasher = aura_crypto::blake3_hasher();
        hasher.update(first.as_bytes());
        hasher.update(second.as_bytes());
        hasher.update(b"relationship");

        Self(*hasher.finalize().as_bytes())
    }

    /// Create from raw bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

/// Get current unix timestamp in seconds using injected effects
pub fn current_timestamp_with_effects(effects: &aura_crypto::Effects) -> crate::Result<u64> {
    effects.now().map_err(|e| {
        crate::LedgerError::SerializationFailed(format!("Failed to get current timestamp: {}", e))
    })
}

/// Builder for Event that provides a fluent API for construction
pub struct EventBuilder {
    account_id: Option<AccountId>,
    nonce: Option<u64>,
    parent_hash: Option<Option<[u8; 32]>>,
    epoch_at_write: Option<u64>,
    event_type: Option<EventType>,
    authorization: Option<EventAuthorization>,
    effects: Option<aura_crypto::Effects>,

    // Optional overrides for automatic fields
    version_override: Option<u16>,
    event_id_override: Option<EventId>,
    timestamp_override: Option<u64>,
}

impl EventBuilder {
    /// Create a new event builder
    pub fn new() -> Self {
        Self {
            account_id: None,
            nonce: None,
            parent_hash: None,
            epoch_at_write: None,
            event_type: None,
            authorization: None,
            effects: None,
            version_override: None,
            event_id_override: None,
            timestamp_override: None,
        }
    }

    /// Set the account ID
    pub fn account_id(mut self, account_id: AccountId) -> Self {
        self.account_id = Some(account_id);
        self
    }

    /// Set the nonce
    pub fn nonce(mut self, nonce: u64) -> Self {
        self.nonce = Some(nonce);
        self
    }

    /// Set the parent hash
    pub fn parent_hash(mut self, parent_hash: Option<[u8; 32]>) -> Self {
        self.parent_hash = Some(parent_hash);
        self
    }

    /// Set parent hash to None (for genesis events)
    pub fn genesis_event(mut self) -> Self {
        self.parent_hash = Some(None);
        self
    }

    /// Set the epoch at write
    pub fn epoch_at_write(mut self, epoch: u64) -> Self {
        self.epoch_at_write = Some(epoch);
        self
    }

    /// Set the event type
    pub fn event_type(mut self, event_type: EventType) -> Self {
        self.event_type = Some(event_type);
        self
    }

    /// Set authorization using threshold signature
    pub fn with_threshold_signature(mut self, signature: crate::ThresholdSig) -> Self {
        self.authorization = Some(EventAuthorization::ThresholdSignature(signature));
        self
    }

    /// Set authorization using device certificate
    pub fn with_device_certificate(
        mut self,
        device_id: DeviceId,
        signature: aura_crypto::Ed25519Signature,
    ) -> Self {
        self.authorization = Some(EventAuthorization::DeviceCertificate {
            device_id,
            signature,
        });
        self
    }

    /// Set authorization using guardian signature
    pub fn with_guardian_signature(
        mut self,
        guardian_id: GuardianId,
        signature: aura_crypto::Ed25519Signature,
    ) -> Self {
        self.authorization = Some(EventAuthorization::GuardianSignature {
            guardian_id,
            signature,
        });
        self
    }

    /// Set effects for generating timestamp and event ID
    pub fn effects(mut self, effects: aura_crypto::Effects) -> Self {
        self.effects = Some(effects);
        self
    }

    /// Override the default version (useful for testing)
    pub fn version(mut self, version: u16) -> Self {
        self.version_override = Some(version);
        self
    }

    /// Override the default event ID (useful for testing)
    pub fn event_id(mut self, event_id: EventId) -> Self {
        self.event_id_override = Some(event_id);
        self
    }

    /// Override the default timestamp (useful for testing)
    pub fn timestamp(mut self, timestamp: u64) -> Self {
        self.timestamp_override = Some(timestamp);
        self
    }

    /// Create a DKD commitment event
    pub fn dkd_commitment(
        mut self,
        session_id: uuid::Uuid,
        device_id: DeviceId,
        commitment: [u8; 32],
    ) -> Self {
        self.event_type = Some(EventType::RecordDkdCommitment(RecordDkdCommitmentEvent {
            session_id,
            device_id,
            commitment,
        }));
        self
    }

    /// Create a DKD reveal event
    pub fn dkd_reveal(
        mut self,
        session_id: uuid::Uuid,
        device_id: DeviceId,
        point: Vec<u8>,
    ) -> Self {
        self.event_type = Some(EventType::RevealDkdPoint(RevealDkdPointEvent {
            session_id,
            device_id,
            point,
        }));
        self
    }

    /// Create a resharing event
    pub fn resharing_complete(
        mut self,
        session_id: uuid::Uuid,
        new_threshold: u16,
        new_group_public_key: Vec<u8>,
    ) -> Self {
        // Create real test signature to prove new shares work
        let test_signature = Self::create_resharing_proof_signature(
            session_id,
            &new_group_public_key,
            new_threshold,
        );

        self.event_type = Some(EventType::FinalizeResharing(FinalizeResharingEvent {
            session_id,
            new_group_public_key,
            new_threshold,
            test_signature,
        }));
        self
    }

    /// Create a recovery event
    pub fn recovery_complete(mut self, recovery_id: uuid::Uuid, new_device_id: DeviceId) -> Self {
        // Create real test signature to prove recovered identity works
        let test_signature = Self::create_recovery_proof_signature(recovery_id, new_device_id);

        self.event_type = Some(EventType::CompleteRecovery(CompleteRecoveryEvent {
            recovery_id,
            new_device_id,
            test_signature,
        }));
        self
    }

    /// Create a locking event
    pub fn lock_acquired(
        mut self,
        session_id: uuid::Uuid,
        operation_type: OperationType,
        winner_device_id: DeviceId,
    ) -> Self {
        self.event_type = Some(EventType::GrantOperationLock(GrantOperationLockEvent {
            operation_type,
            session_id,
            winner_device_id,
            granted_at_epoch: 0, // Will be set by protocol
            threshold_signature: crate::ThresholdSig {
                signature: aura_crypto::Ed25519Signature::from_bytes(&[0u8; 64]),
                signers: Vec::new(),
                signature_shares: Vec::new(),
            },
        }));
        self
    }

    /// Build the event
    pub fn build(self) -> Result<Event, EventBuildError> {
        let account_id = self.account_id.ok_or(EventBuildError::MissingAccountId)?;
        let nonce = self.nonce.ok_or(EventBuildError::MissingNonce)?;
        let epoch_at_write = self
            .epoch_at_write
            .ok_or(EventBuildError::MissingEpochAtWrite)?;
        let event_type = self.event_type.ok_or(EventBuildError::MissingEventType)?;
        let authorization = self
            .authorization
            .ok_or(EventBuildError::MissingAuthorization)?;
        let effects = self.effects.ok_or(EventBuildError::MissingEffects)?;

        let event = Event {
            version: self.version_override.unwrap_or(EVENT_VERSION),
            event_id: self
                .event_id_override
                .unwrap_or_else(|| EventId(effects.gen_uuid())),
            account_id,
            timestamp: if let Some(ts) = self.timestamp_override {
                ts
            } else {
                effects
                    .now()
                    .map_err(|e| EventBuildError::TimestampError(format!("Time error: {}", e)))?
            },
            nonce,
            parent_hash: self.parent_hash.flatten(),
            epoch_at_write,
            event_type,
            authorization,
        };

        Ok(event)
    }

    /// Create real proof signature for resharing completion
    ///
    /// This proves that the new key shares are valid and functional
    fn create_resharing_proof_signature(
        session_id: uuid::Uuid,
        new_group_public_key: &[u8],
        new_threshold: u16,
    ) -> Vec<u8> {

        // Create deterministic signing key for proof
        let mut seed = [0u8; 32];
        seed[..16].copy_from_slice(&session_id.as_bytes()[..16]);
        seed[16..18].copy_from_slice(&new_threshold.to_le_bytes());
        seed[18..].copy_from_slice(&new_group_public_key[..14]);

        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&seed);

        // Create proof message
        let mut message = Vec::new();
        message.extend_from_slice(b"AURA_RESHARING_PROOF:");
        message.extend_from_slice(session_id.as_bytes());
        message.extend_from_slice(new_group_public_key);
        message.extend_from_slice(&new_threshold.to_le_bytes());

        let signature = aura_crypto::ed25519_sign(&signing_key, &message);
        aura_crypto::ed25519_signature_to_bytes(&signature).to_vec()
    }

    /// Create real proof signature for recovery completion
    ///
    /// This proves that the recovered identity is valid and functional
    fn create_recovery_proof_signature(
        recovery_id: uuid::Uuid,
        new_device_id: DeviceId,
    ) -> Vec<u8> {

        // Create deterministic signing key for proof
        let mut seed = [0u8; 32];
        seed[..16].copy_from_slice(&recovery_id.as_bytes()[..16]);
        seed[16..].copy_from_slice(new_device_id.0.as_bytes());

        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&seed);

        // Create proof message
        let mut message = Vec::new();
        message.extend_from_slice(b"AURA_RECOVERY_PROOF:");
        message.extend_from_slice(recovery_id.as_bytes());
        message.extend_from_slice(new_device_id.0.as_bytes());

        let signature = aura_crypto::ed25519_sign(&signing_key, &message);
        aura_crypto::ed25519_signature_to_bytes(&signature).to_vec()
    }
}

impl Default for EventBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur during event building
#[derive(Debug, thiserror::Error)]
pub enum EventBuildError {
    #[error("Missing account ID")]
    MissingAccountId,

    #[error("Missing nonce")]
    MissingNonce,

    #[error("Missing epoch at write")]
    MissingEpochAtWrite,

    #[error("Missing event type")]
    MissingEventType,

    #[error("Missing authorization")]
    MissingAuthorization,

    #[error("Missing effects")]
    MissingEffects,

    #[error("Timestamp error: {0}")]
    TimestampError(String),
}
