// Core types for the CRDT ledger

use aura_crypto::Ed25519VerifyingKey;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Re-export shared types from crypto and aura-types
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
        Cid(hex::encode(blake3::hash(data).as_bytes()))
    }

    /// Create a CID from a string
    pub fn from_string(s: &str) -> Self {
        Cid(s.to_string())
    }
}

// Display for AccountId is implemented in aura-crypto crate

/// Device metadata stored in CRDT
///
/// Tracks device information, cryptographic keys, and replay protection state.
/// Reference: 080 spec Part 3: Ledger Compaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMetadata {
    /// Unique identifier for this device
    pub device_id: DeviceId,
    /// Human-readable name for the device
    pub device_name: String,
    /// Classification of device type (Native, Guardian, or Browser)
    pub device_type: DeviceType,
    /// Ed25519 public key for device signature verification
    pub public_key: Ed25519VerifyingKey,
    /// Timestamp (seconds since epoch) when device was added to account
    pub added_at: u64,
    /// Timestamp of the most recent activity from this device
    pub last_seen: u64,
    /// Merkle proofs for DKD commitments (session_id -> proof)
    /// Required for post-compaction recovery verification
    pub dkd_commitment_proofs: std::collections::BTreeMap<Uuid, Vec<u8>>,
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
    /// User's primary device with full account control
    Native,
    /// Guardian device used for account recovery
    Guardian,
    /// Browser-based device with limited capabilities
    Browser,
}

/// Guardian metadata
///
/// Tracks information about a guardian who can help recover account access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianMetadata {
    /// Unique identifier for this guardian
    pub guardian_id: GuardianId,
    /// Device ID of the guardian's device
    pub device_id: DeviceId,
    /// Email address for guardian contact
    pub email: String,
    /// Ed25519 public key for signature verification
    pub public_key: Ed25519VerifyingKey,
    /// Timestamp when this guardian was added
    pub added_at: u64,
    /// Policy controlling guardian recovery actions
    pub policy: GuardianPolicy,
}

/// Guardian policy configuration
///
/// Controls how a guardian can participate in account recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianPolicy {
    /// Whether this guardian's recovery actions require explicit approval
    pub requires_approval: bool,
    /// Cooldown period in seconds between recovery actions by this guardian
    pub cooldown_period: u64,
    /// Maximum number of recovery operations allowed per calendar day
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
    fn new_with_effects(effects: &dyn aura_crypto::effects::CryptoEffects) -> Self;
}

impl SessionIdExt for SessionId {
    fn new_with_effects(effects: &dyn aura_crypto::effects::CryptoEffects) -> Self {
        // Generate random bytes for UUID v4 using crypto effects
        let random_bytes: Vec<u8> = (0..16).map(|_| effects.random_byte()).collect();

        // Create UUID v4 from random bytes
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&random_bytes);

        // Set version (4) and variant bits for UUID v4
        uuid_bytes[6] = (uuid_bytes[6] & 0x0f) | 0x40; // Version 4
        uuid_bytes[8] = (uuid_bytes[8] & 0x3f) | 0x80; // Variant 10

        let uuid = uuid::Uuid::from_bytes(uuid_bytes);
        SessionId::from_uuid(uuid)
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
///
/// Prevents concurrent execution of the same operation type across devices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationLock {
    /// Type of operation being locked
    pub operation_type: OperationType,
    /// Session ID associated with this lock
    pub session_id: SessionId,
    /// Timestamp when lock was acquired
    pub acquired_at: u64,
    /// Timestamp when lock expires (auto-release)
    pub expires_at: u64,
    /// Participant ID holding the lock
    pub holder: ParticipantId,
    /// Device ID of the lock holder
    pub holder_device_id: DeviceId,
    /// Epoch during which lock was granted
    pub granted_at_epoch: u64,
    /// Lottery ticket for fair lock acquisition
    pub lottery_ticket: [u8; 32],
}

/// DKD commitment root for verification
///
/// Stores the root hash of DKD commitments for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdCommitmentRoot {
    /// Session ID for which commitments were made
    pub session_id: SessionId,
    /// Blake3 hash of all commitments (32 bytes)
    pub root_hash: [u8; 32],
    /// Number of individual commitments included in this root
    pub commitment_count: u32,
    /// Timestamp when root was created
    pub created_at: u64,
}

/// Contact information for guardians
///
/// Stores communication details for reaching a guardian.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactInfo {
    /// Primary email address for guardian contact
    pub email: String,
    /// Optional phone number for guardian contact
    pub phone: Option<String>,
    /// Optional backup email address
    pub backup_email: Option<String>,
    /// Guardian's notification preferences
    pub notification_preferences: NotificationPreferences,
}

/// Notification preferences
///
/// Controls how and when guardians receive notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPreferences {
    /// Whether email notifications are enabled
    pub email_enabled: bool,
    /// Whether phone/SMS notifications are enabled
    pub phone_enabled: bool,
    /// Minimum urgency level required to send notifications
    pub urgency_threshold: UrgencyLevel,
}

/// Urgency level for notifications
///
/// Categorizes operational events by their importance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UrgencyLevel {
    /// Low priority - routine updates
    Low,
    /// Medium priority - important updates requiring attention
    Medium,
    /// High priority - urgent security events
    High,
    /// Critical priority - immediate action required
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
///
/// Represents an active or completed protocol session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique identifier for this session
    pub session_id: SessionId,
    /// Type of protocol being executed in this session
    pub protocol_type: ProtocolType,
    /// List of participants in this session
    pub participants: Vec<ParticipantId>,
    /// Timestamp when session was started
    pub started_at: u64,
    /// Timestamp when session will expire
    pub expires_at: u64,
    /// Current status of the session
    pub status: SessionStatus,
    /// Additional metadata stored with the session
    pub metadata: std::collections::BTreeMap<String, String>,
}

impl Session {
    /// Create a new session
    ///
    /// # Arguments
    /// * `session_id` - Unique identifier for the session
    /// * `protocol_type` - Type of protocol being executed
    /// * `participants` - List of participating device IDs
    /// * `started_at` - Timestamp when session starts
    /// * `ttl_in_epochs` - Time-to-live in epochs
    /// * `_timestamp` - Current timestamp (unused)
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

    /// Update the session status
    ///
    /// # Arguments
    /// * `status` - New status for the session
    /// * `_timestamp` - Current timestamp (unused)
    pub fn update_status(&mut self, status: SessionStatus, _timestamp: u64) {
        self.status = status;
    }

    /// Mark session as completed
    ///
    /// # Arguments
    /// * `_outcome` - Protocol outcome (unused)
    /// * `timestamp` - Timestamp of completion
    pub fn complete(&mut self, _outcome: SessionOutcome, timestamp: u64) {
        self.update_status(SessionStatus::Completed, timestamp);
    }

    /// Abort the session due to failure
    ///
    /// # Arguments
    /// * `_reason` - Reason for abort (unused)
    /// * `_blamed_party` - Party responsible for failure (unused)
    /// * `timestamp` - Timestamp of abort
    pub fn abort(&mut self, _reason: &str, _blamed_party: Option<ParticipantId>, timestamp: u64) {
        self.update_status(SessionStatus::Failed, timestamp);
    }

    /// Check if session is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            SessionStatus::Completed
                | SessionStatus::Failed
                | SessionStatus::Expired
                | SessionStatus::TimedOut
        )
    }

    /// Check if session has timed out
    ///
    /// # Arguments
    /// * `current_epoch` - Current epoch for comparison
    pub fn is_timed_out(&self, current_epoch: u64) -> bool {
        current_epoch > self.expires_at
    }

    /// Check if session has expired
    ///
    /// # Arguments
    /// * `current_epoch` - Current epoch for comparison
    pub fn is_expired(&self, current_epoch: u64) -> bool {
        self.is_timed_out(current_epoch)
    }
}

// SessionStatus is now imported from aura-types

// SessionOutcome is now imported from aura-types

/// Ed25519 signature share for threshold signatures
///
/// Represents one participant's contribution to a threshold signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureShare {
    /// ID of the participant who created this share
    pub participant_id: ParticipantId,
    /// The signature share bytes
    pub signature_share: Vec<u8>,
    /// Commitment value for this share
    pub commitment: Vec<u8>,
}

/// Cooldown counter for rate limiting
///
/// Tracks how many times an operation has been performed recently.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CooldownCounter {
    /// Participant being rate-limited
    pub participant_id: ParticipantId,
    /// Type of operation being counted
    pub operation_type: OperationType,
    /// Current count in this cooldown period
    pub count: u32,
    /// Timestamp when counter resets
    pub reset_at: u64,
}

/// Presence ticket cache
///
/// Caches presence tickets for efficient session participation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceTicketCache {
    /// Device holding this ticket
    pub device_id: DeviceId,
    /// Session epoch for which ticket is valid
    pub session_epoch: SessionEpoch,
    /// The ticket bytes
    pub ticket: Vec<u8>,
    /// Timestamp when ticket expires
    pub expires_at: u64,
    /// Timestamp when ticket was issued
    pub issued_at: u64,
    /// Blake3 digest of the ticket for verification
    pub ticket_digest: [u8; 32],
}

/// Evidence of Byzantine behavior for logging and analysis
///
/// Documents detected misbehavior by system participants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ByzantineEvidence {
    /// Resource exhaustion attack detected
    ResourceExhaustion {
        /// Number of excessive requests
        request_count: u64,
        /// Time window of the attack in milliseconds
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
///
/// Indicates the operational impact of detected misbehavior.
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
///
/// Tracks the current state of blob replication across the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplicationStatus {
    /// Still replicating to reach target factor
    Replicating {
        /// Current number of replicas
        current: u8,
        /// Target number of replicas
        target: u8,
    },
    /// Successfully replicated
    Complete {
        /// Number of replicas achieved
        replica_count: u8,
    },
    /// Replication degraded (lost replicas)
    Degraded {
        /// Current number of replicas
        current: u8,
        /// Target number of replicas
        target: u8,
    },
    /// Replication failed
    Failed {
        /// Reason for replication failure
        reason: String,
    },
}

/// Access control policy for storage blobs
///
/// Specifies who can perform which operations on a blob.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessPolicy {
    /// Read access requirements (list of required capabilities)
    pub read_permissions: Vec<String>,
    /// Write access requirements (list of required capabilities)
    pub write_permissions: Vec<String>,
    /// Delete access requirements (list of required capabilities)
    pub delete_permissions: Vec<String>,
    /// Time-based access restrictions
    pub time_restrictions: Option<TimeRestrictions>,
}

/// Time-based access restrictions
///
/// Controls when a blob can be accessed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRestrictions {
    /// Access not allowed before this timestamp (seconds since epoch)
    pub not_before: Option<u64>,
    /// Access not allowed after this timestamp (seconds since epoch)
    pub not_after: Option<u64>,
}

/// Access audit log entry
///
/// Records each access attempt to a storage blob.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessAuditEntry {
    /// Unique audit entry ID
    pub entry_id: Uuid,
    /// Content identifier that was accessed
    pub cid: Cid,
    /// Type of access operation performed
    pub operation: AccessOperation,
    /// Device that performed the access
    pub device_id: DeviceId,
    /// ID of the capability token used for access
    pub capability_token_id: String,
    /// Timestamp when access occurred (seconds since epoch)
    pub accessed_at: u64,
    /// Result of the access attempt
    pub result: AccessResult,
    /// Additional context about the access
    pub context: Option<String>,
}

/// Type of storage access operation
///
/// Categorizes the different kinds of operations on storage blobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccessOperation {
    /// Read operation - retrieve blob contents
    Read,
    /// Write/store operation - create or update blob
    Write,
    /// Delete operation - remove blob
    Delete,
    /// Metadata query - read blob metadata without content
    Metadata,
}

/// Result of an access attempt
///
/// Indicates success or failure of an access operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccessResult {
    /// Access granted successfully
    Granted,
    /// Access denied due to insufficient permissions
    Denied {
        /// Reason why access was denied
        reason: String,
    },
    /// Access failed due to technical error
    Failed {
        /// Error description
        error: String,
    },
}

/// Key derivation specification for encryption
///
/// Specifies parameters for deriving encryption keys for storage blobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyDerivationSpec {
    /// Context string for key derivation (e.g., blob ID, device ID)
    pub context: String,
    /// Key derivation algorithm (e.g., "HKDF-SHA256")
    pub algorithm: String,
    /// Additional algorithm-specific parameters
    pub params: std::collections::BTreeMap<String, String>,
}
