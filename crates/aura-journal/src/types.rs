// Core types for the CRDT ledger

use aura_crypto::{verifying_key_serde, Ed25519VerifyingKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Re-export shared types from crypto and aura-types
use aura_crypto::MerkleProof;
use aura_types::{DeviceId, GuardianId};

// Import authentication types (ThresholdSig is imported where needed)

// Re-export consolidated types from aura-types
pub use aura_types::{
    Cid as AuraTypesCid, Epoch, EventNonce, OperationType, ParticipantId, ProtocolType,
    SessionEpoch, SessionId, SessionOutcome, SessionStatus,
};

/// Content Identifier for storage operations
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Cid(pub String);

impl Cid {
    /// Create a CID from bytes using Blake3 hash
    pub fn from_bytes(data: &[u8]) -> Self {
        use aura_crypto::blake3_hash;
        Cid(hex::encode(blake3_hash(data)))
    }

    /// Create a CID from a string
    pub fn from_string(s: &str) -> Self {
        Cid(s.to_string())
    }
}

// Display for AccountId is implemented in aura-crypto crate

/// Device metadata stored in CRDT
///
/// Reference: 080 spec Part 3: Ledger Compaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMetadata {
    pub device_id: DeviceId,
    pub device_name: String,
    pub device_type: DeviceType,
    #[serde(with = "verifying_key_serde")]
    pub public_key: Ed25519VerifyingKey,
    pub added_at: u64,
    pub last_seen: u64,
    /// Merkle proofs for DKD commitments (session_id -> proof)
    /// Required for post-compaction recovery verification
    pub dkd_commitment_proofs: std::collections::BTreeMap<Uuid, MerkleProof>,
    /// Next nonce for this device (monotonic counter)
    /// Used for device-specific replay protection
    pub next_nonce: u64,
    /// Recently used nonces for replay protection (bounded set)
    /// Maintains a sliding window of recent nonces to prevent replay attacks
    pub used_nonces: std::collections::BTreeSet<u64>,
    /// Current key share epoch for this device
    /// Updated during resharing operations to track key rotation
    pub key_share_epoch: u64,
}

/// Device type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceType {
    Native,   // User's primary device
    Guardian, // Guardian device
    Browser,  // Browser-based device
}

/// Guardian metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianMetadata {
    pub guardian_id: GuardianId,
    pub device_id: DeviceId,
    pub email: String,
    #[serde(with = "verifying_key_serde")]
    pub public_key: Ed25519VerifyingKey,
    pub added_at: u64,
    pub policy: GuardianPolicy,
}

/// Guardian policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianPolicy {
    pub requires_approval: bool,
    pub cooldown_period: u64, // seconds
    pub max_recoveries_per_day: u32,
}

impl Default for GuardianPolicy {
    fn default() -> Self {
        Self {
            requires_approval: true,
            cooldown_period: 86400, // 24 hours
            max_recoveries_per_day: 1,
        }
    }
}

// ParticipantId is now imported from aura-types

// SessionId is now imported from aura-types
// Extensions for journal-specific functionality
pub trait SessionIdExt {
    fn new_with_effects(effects: &aura_crypto::Effects) -> Self;
}

impl SessionIdExt for SessionId {
    fn new_with_effects(effects: &aura_crypto::Effects) -> Self {
        SessionId::from_uuid(effects.gen_uuid())
    }
}

// OperationType is now imported from aura-types

// ProtocolType is now imported from aura-types

/// Comprehensive audit trail for signature share verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureShareAuditTrail {
    /// Hash of the message that was signed
    pub message_hash: Vec<u8>,
    /// Hash of the aggregated signature
    pub signature_hash: Vec<u8>,
    /// Total number of signature shares provided
    pub total_shares: usize,
    /// Details of valid signature shares
    pub valid_shares: Vec<ValidShareDetail>,
    /// Details of invalid signature shares
    pub invalid_shares: Vec<InvalidShareDetail>,
    /// All verification details for comprehensive audit
    pub verification_details: Vec<ValidShareDetail>,
    /// Calculated authority level based on valid shares
    pub authority_level: f64,
    /// Timestamp when verification was performed
    pub verification_timestamp: u64,
}

/// Details of a valid signature share
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidShareDetail {
    /// ID of the participant who created this share
    pub signer_id: u8,
    /// Index of this share in the share list
    pub share_index: usize,
    /// Hash of the signature share bytes
    pub share_hash: Vec<u8>,
    /// Verification status of this share
    pub verification_status: ShareVerificationStatus,
    /// Weight of this share's contribution to authority
    pub contribution_weight: f64,
    /// Timestamp when this share was verified
    pub timestamp: u64,
}

/// Details of an invalid signature share
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidShareDetail {
    /// ID of the participant who created this share
    pub signer_id: u8,
    /// Index of this share in the share list
    pub share_index: usize,
    /// Reason why this share failed verification
    pub error_reason: String,
    /// Timestamp when verification failure was detected
    pub timestamp: u64,
}

/// Status of signature share verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShareVerificationStatus {
    /// Share is structurally valid but not cryptographically verified
    StructurallyValid,
    /// Share is fully cryptographically verified
    CryptographicallyVerified,
    /// Share failed structural validation
    StructurallyInvalid,
    /// Share failed cryptographic verification
    CryptographicallyInvalid,
}

// EventNonce is now imported from aura-types

/// Operation lock for distributed coordination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationLock {
    pub operation_type: OperationType,
    pub session_id: SessionId,
    pub acquired_at: u64,
    pub expires_at: u64,
    pub holder: ParticipantId,
    pub holder_device_id: DeviceId,
    pub granted_at_epoch: u64,
    pub lottery_ticket: [u8; 32],
}

/// DKD commitment root for verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdCommitmentRoot {
    pub session_id: SessionId,
    pub root_hash: [u8; 32],
    pub commitment_count: u32,
    pub created_at: u64,
}

/// Contact information for guardians
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactInfo {
    pub email: String,
    pub phone: Option<String>,
    pub backup_email: Option<String>,
    pub notification_preferences: NotificationPreferences,
}

/// Notification preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPreferences {
    pub email_enabled: bool,
    pub phone_enabled: bool,
    pub urgency_threshold: UrgencyLevel,
}

/// Urgency level for notifications
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UrgencyLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl Default for NotificationPreferences {
    fn default() -> Self {
        Self {
            email_enabled: true,
            phone_enabled: false,
            urgency_threshold: UrgencyLevel::Medium,
        }
    }
}

/// Session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_id: SessionId,
    pub protocol_type: ProtocolType,
    pub participants: Vec<ParticipantId>,
    pub started_at: u64,
    pub expires_at: u64,
    pub status: SessionStatus,
    pub metadata: std::collections::BTreeMap<String, String>,
}

impl Session {
    pub fn new(
        session_id: SessionId,
        protocol_type: ProtocolType,
        participants: Vec<ParticipantId>,
        started_at: u64,
        ttl_in_epochs: u64,
        _timestamp: u64,
    ) -> Self {
        Self {
            session_id,
            protocol_type,
            participants,
            started_at,
            expires_at: started_at + ttl_in_epochs,
            status: SessionStatus::Active,
            metadata: std::collections::BTreeMap::new(),
        }
    }

    pub fn update_status(&mut self, status: SessionStatus, _timestamp: u64) {
        self.status = status;
    }

    pub fn complete(&mut self, _outcome: SessionOutcome, timestamp: u64) {
        self.update_status(SessionStatus::Completed, timestamp);
    }

    pub fn abort(&mut self, _reason: &str, _blamed_party: Option<ParticipantId>, timestamp: u64) {
        self.update_status(SessionStatus::Failed, timestamp);
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            SessionStatus::Completed
                | SessionStatus::Failed
                | SessionStatus::Expired
                | SessionStatus::TimedOut
        )
    }

    pub fn is_timed_out(&self, current_epoch: u64) -> bool {
        current_epoch > self.expires_at
    }

    pub fn is_expired(&self, current_epoch: u64) -> bool {
        self.is_timed_out(current_epoch)
    }
}

// SessionStatus is now imported from aura-types

// SessionOutcome is now imported from aura-types

/// Ed25519Signature share for threshold signatures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureShare {
    pub participant_id: ParticipantId,
    pub signature_share: Vec<u8>,
    pub commitment: Vec<u8>,
}

/// Cooldown counter for rate limiting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CooldownCounter {
    pub participant_id: ParticipantId,
    pub operation_type: OperationType,
    pub count: u32,
    pub reset_at: u64,
}

/// Presence ticket cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceTicketCache {
    pub device_id: DeviceId,
    pub session_epoch: SessionEpoch,
    pub ticket: Vec<u8>,
    pub expires_at: u64,
    pub issued_at: u64,
    pub ticket_digest: [u8; 32],
}

/// Evidence of Byzantine behavior for logging and analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ByzantineEvidence {
    /// Resource exhaustion attack detected
    ResourceExhaustion {
        /// Number of excessive requests
        request_count: u64,
        /// Time window of the attack
        window_ms: u64,
    },
    /// Invalid protocol behavior
    InvalidBehavior {
        /// Description of the invalid behavior
        description: String,
        /// Raw evidence data
        evidence: Vec<u8>,
    },
    /// Protocol deviation detected
    ProtocolDeviation {
        /// Expected protocol step
        expected: String,
        /// Actual behavior observed
        actual: String,
    },
}

/// Severity level for Byzantine behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ByzantineSeverity {
    /// Low impact, monitoring only
    Low,
    /// Medium impact, affects local operations
    Medium,
    /// High impact, affects protocol security
    High,
    /// Critical impact, system compromise
    Critical,
}

// ========== Storage Types ==========

/// Storage metadata for blobs managed by the system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageMetadata {
    /// Content identifier
    pub cid: Cid,
    /// Size in bytes
    pub size_bytes: u64,
    /// Required capabilities for access
    pub required_capabilities: Vec<String>,
    /// Number of replicas to maintain
    pub replication_factor: u8,
    /// Encryption key derivation spec
    pub encryption_key_spec: KeyDerivationSpec,
    /// When the blob was stored
    pub stored_at: u64,
    /// Device that initiated storage
    pub stored_by: DeviceId,
    /// Current replication status
    pub replication_status: ReplicationStatus,
    /// Access control policy
    pub access_policy: AccessPolicy,
}

/// Storage quota tracking for a device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageQuota {
    /// Device this quota applies to
    pub device_id: DeviceId,
    /// Maximum storage allowed in bytes
    pub max_bytes: u64,
    /// Currently used storage in bytes
    pub used_bytes: u64,
    /// Number of blobs stored
    pub blob_count: u64,
    /// Last updated timestamp
    pub updated_at: u64,
}

/// Replication status for a blob
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplicationStatus {
    /// Still replicating to reach target factor
    Replicating { current: u8, target: u8 },
    /// Successfully replicated
    Complete { replica_count: u8 },
    /// Replication degraded (lost replicas)
    Degraded { current: u8, target: u8 },
    /// Replication failed
    Failed { reason: String },
}

/// Access control policy for storage blobs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessPolicy {
    /// Read access requirements
    pub read_permissions: Vec<String>,
    /// Write access requirements
    pub write_permissions: Vec<String>,
    /// Delete access requirements
    pub delete_permissions: Vec<String>,
    /// Time-based access restrictions
    pub time_restrictions: Option<TimeRestrictions>,
}

/// Time-based access restrictions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRestrictions {
    /// Access not allowed before this timestamp
    pub not_before: Option<u64>,
    /// Access not allowed after this timestamp
    pub not_after: Option<u64>,
}

/// Access audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessAuditEntry {
    /// Unique audit entry ID
    pub entry_id: Uuid,
    /// Content that was accessed
    pub cid: Cid,
    /// Type of access operation
    pub operation: AccessOperation,
    /// Device that performed the access
    pub device_id: DeviceId,
    /// Capability token used for access
    pub capability_token_id: String,
    /// Timestamp of access
    pub accessed_at: u64,
    /// Result of the access attempt
    pub result: AccessResult,
    /// Additional context
    pub context: Option<String>,
}

/// Type of storage access operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccessOperation {
    /// Read operation
    Read,
    /// Write/store operation
    Write,
    /// Delete operation
    Delete,
    /// Metadata query
    Metadata,
}

/// Result of an access attempt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccessResult {
    /// Access granted successfully
    Granted,
    /// Access denied due to insufficient permissions
    Denied { reason: String },
    /// Access failed due to technical error
    Failed { error: String },
}

/// Key derivation specification for encryption
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyDerivationSpec {
    /// Context for key derivation
    pub context: String,
    /// Algorithm used for key derivation
    pub algorithm: String,
    /// Additional parameters
    pub params: std::collections::BTreeMap<String, String>,
}
