// Core types for the CRDT ledger

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Re-export shared types from crypto
pub use aura_crypto::{AccountId, DeviceId, GuardianId, MerkleProof};

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
/// Reference: 080 spec Part 3: Ledger Compaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMetadata {
    pub device_id: DeviceId,
    pub device_name: String,
    pub device_type: DeviceType,
    #[serde(with = "verifying_key_serde")]
    pub public_key: VerifyingKey,
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
}

mod verifying_key_serde {
    use ed25519_dalek::VerifyingKey;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(key: &VerifyingKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(key.as_bytes())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<VerifyingKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        VerifyingKey::from_bytes(
            bytes
                .as_slice()
                .try_into()
                .map_err(serde::de::Error::custom)?,
        )
        .map_err(serde::de::Error::custom)
    }
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
    pub public_key: VerifyingKey,
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

/// Session epoch for replay protection
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SessionEpoch(pub u64);

impl SessionEpoch {
    pub fn initial() -> Self {
        SessionEpoch(0)
    }

    pub fn next(self) -> Self {
        SessionEpoch(self.0 + 1)
    }
}

impl Default for SessionEpoch {
    fn default() -> Self {
        Self::initial()
    }
}

impl std::fmt::Display for SessionEpoch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Participant identifier (device ID or guardian ID)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ParticipantId {
    Device(DeviceId),
    Guardian(GuardianId),
}

impl std::fmt::Display for ParticipantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParticipantId::Device(id) => write!(f, "device:{}", id),
            ParticipantId::Guardian(id) => write!(f, "guardian:{}", id),
        }
    }
}

impl From<DeviceId> for ParticipantId {
    fn from(id: DeviceId) -> Self {
        ParticipantId::Device(id)
    }
}

impl From<GuardianId> for ParticipantId {
    fn from(id: GuardianId) -> Self {
        ParticipantId::Guardian(id)
    }
}

/// Session identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new_with_effects(effects: &aura_crypto::Effects) -> Self {
        SessionId(effects.gen_uuid())
    }
}


impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Operation type for protocol classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OperationType {
    Dkd,        // Key derivation
    Resharing,  // Key resharing
    Recovery,   // Guardian recovery
    Locking,    // Distributed locking
}

impl std::fmt::Display for OperationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationType::Dkd => write!(f, "dkd"),
            OperationType::Resharing => write!(f, "resharing"),
            OperationType::Recovery => write!(f, "recovery"),
            OperationType::Locking => write!(f, "locking"),
        }
    }
}

/// Protocol type for session classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProtocolType {
    Dkd,            // Key derivation
    Resharing,      // Key resharing
    Recovery,       // Guardian recovery
    Locking,        // Distributed locking
    LockAcquisition, // Lock acquisition
}

impl From<OperationType> for ProtocolType {
    fn from(op: OperationType) -> Self {
        match op {
            OperationType::Dkd => ProtocolType::Dkd,
            OperationType::Resharing => ProtocolType::Resharing,
            OperationType::Recovery => ProtocolType::Recovery,
            OperationType::Locking => ProtocolType::Locking,
        }
    }
}

impl std::fmt::Display for ProtocolType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtocolType::Dkd => write!(f, "dkd"),
            ProtocolType::Resharing => write!(f, "resharing"),
            ProtocolType::Recovery => write!(f, "recovery"),
            ProtocolType::Locking => write!(f, "locking"),
            ProtocolType::LockAcquisition => write!(f, "lock_acquisition"),
        }
    }
}

/// Threshold signature with participant tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdSig {
    #[serde(with = "signature_serde")]
    pub signature: Signature,
    /// Indices of signers who contributed to this signature
    pub signers: Vec<u8>,
    /// Individual signature shares (for verification)
    pub signature_shares: Vec<Vec<u8>>,
}

mod signature_serde {
    use ed25519_dalek::Signature;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(signature: &Signature, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&signature.to_bytes())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Signature, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        let bytes_array: [u8; 64] = bytes
            .as_slice()
            .try_into()
            .map_err(serde::de::Error::custom)?;
        Ok(Signature::from_bytes(&bytes_array))
    }
}

/// Event nonce for replay protection
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EventNonce(pub u64);

impl EventNonce {
    pub fn new(value: u64) -> Self {
        EventNonce(value)
    }
}

impl std::fmt::Display for EventNonce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

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
            SessionStatus::Completed | SessionStatus::Failed | SessionStatus::Expired | SessionStatus::TimedOut
        )
    }

    pub fn is_timed_out(&self, current_epoch: u64) -> bool {
        current_epoch > self.expires_at
    }

    pub fn is_expired(&self, current_epoch: u64) -> bool {
        self.is_timed_out(current_epoch)
    }
}

/// Session status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    Active,
    Completed,
    Failed,
    Expired,
    TimedOut,
}

/// Session outcome
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionOutcome {
    Success,
    Failed,
    Aborted,
}

/// Signature share for threshold signatures
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