// Core types for the CRDT ledger

use aura_crypto::Ed25519VerifyingKey;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Re-export shared types from crypto and aura-core
use aura_core::{DeviceId, GuardianId};

// Import authentication types (ThresholdSig is imported where needed)

// Re-export consolidated types from aura-core
pub use aura_core::{
    OperationType, ParticipantId, ProtocolType, SessionEpoch, SessionId, SessionOutcome,
    SessionStatus,
};

// Use ContentId from aura-core

// Display for AccountId is implemented in aura-crypto crate

/// Device metadata stored in CRDT
///
/// Tracks device information, cryptographic keys, and replay protection state.
/// Reference: 080 spec Part 3: Ledger Compaction
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

// ParticipantId is now imported from aura-core

// SessionId is now imported from aura-core
// Extensions for journal-specific functionality
/// Provides effects-based session ID generation for journal operations
pub trait SessionIdExt {
    async fn new_with_effects(effects: &dyn aura_core::effects::CryptoEffects) -> Self;
}

impl SessionIdExt for SessionId {
    async fn new_with_effects(effects: &dyn aura_core::effects::CryptoEffects) -> Self {
        // Generate random bytes for UUID v4 using crypto effects
        let random_bytes = effects.random_bytes(16).await;

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

// OperationType is now imported from aura-core

// ProtocolType is now imported from aura-core

// EventNonce is now imported from aura-core

/// Operation lock for distributed coordination
///
/// Prevents concurrent execution of the same operation type across devices.
/// Core to distributed locking protocol documented in 500_distributed_*.md specs.
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

// SessionStatus is now imported from aura-core

// SessionOutcome is now imported from aura-core

/// Ed25519 signature share for threshold signatures
///
/// Represents one participant's contribution to a threshold signature.
/// Actively used in FROST crypto implementation across aura-crypto and aura-protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureShare {
    /// ID of the participant who created this share
    pub participant_id: ParticipantId,
    /// The signature share bytes
    pub signature_share: Vec<u8>,
    /// Commitment value for this share
    pub commitment: Vec<u8>,
}

/// Presence ticket cache
///
/// Caches presence tickets for efficient session participation.
/// Part of session management documented in 001_identity_spec.md.
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

// ========== Storage Types ==========

/// Storage quota tracking for a device
///
/// Integrated with error handling system (StorageQuotaExceeded).
/// Referenced in group storage documentation as foundational infrastructure.
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

/// Key derivation specification for encryption
///
/// Specifies parameters for deriving encryption keys for storage blobs.
/// Extensively used in crypto tests and core security infrastructure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyDerivationSpec {
    /// Context string for key derivation (e.g., blob ID, device ID)
    pub context: String,
    /// Key derivation algorithm (e.g., "HKDF-SHA256")
    pub algorithm: String,
    /// Additional algorithm-specific parameters
    pub params: std::collections::BTreeMap<String, String>,
}
