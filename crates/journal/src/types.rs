// Core types for the CRDT ledger

use ed25519_dalek::{VerifyingKey, Signature};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Device identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct DeviceId(pub Uuid);

impl DeviceId {
    pub fn new() -> Self {
        DeviceId(Uuid::new_v4())
    }
}

impl Default for DeviceId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Guardian identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct GuardianId(pub Uuid);

impl GuardianId {
    pub fn new() -> Self {
        GuardianId(Uuid::new_v4())
    }
}

impl Default for GuardianId {
    fn default() -> Self {
        Self::new()
    }
}

/// Account identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AccountId(pub Uuid);

impl AccountId {
    pub fn new() -> Self {
        AccountId(Uuid::new_v4())
    }
}

impl Default for AccountId {
    fn default() -> Self {
        Self::new()
    }
}

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
        VerifyingKey::from_bytes(bytes.as_slice().try_into().map_err(serde::de::Error::custom)?)
            .map_err(serde::de::Error::custom)
    }
}

/// Device type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceType {
    Native,      // User's primary device
    Guardian,    // Guardian device
    Browser,     // Browser-based device
}

/// Guardian metadata stored in CRDT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianMetadata {
    pub guardian_id: GuardianId,
    pub account_id: AccountId,
    pub contact_info: ContactInfo,
    pub added_at: u64,
    pub share_envelope_cid: Option<String>, // CID of encrypted recovery share
}

/// Contact information for guardians
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactInfo {
    pub name: String,
    pub contact_method: ContactMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContactMethod {
    Signal(String),
    Email(String),
    Other(String),
}

/// Session epoch - monotonically increasing counter
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SessionEpoch(pub u64);

impl SessionEpoch {
    pub fn initial() -> Self {
        SessionEpoch(1)
    }
    
    pub fn increment(&self) -> Self {
        SessionEpoch(self.0 + 1)
    }
}

impl Default for SessionEpoch {
    fn default() -> Self {
        Self::initial()
    }
}

/// Presence ticket metadata cached in CRDT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceTicketCache {
    pub device_id: DeviceId,
    pub issued_at: u64,
    pub expires_at: u64,
    pub ticket_digest: [u8; 32], // BLAKE3 hash of ticket for verification
}

/// Policy reference stored in CRDT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyReference {
    pub policy_cid: String,
    pub version: u64,
    pub updated_at: u64,
}

/// Cooldown counter for recovery operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CooldownCounter {
    pub operation_id: Uuid,
    pub started_at: u64,
    pub duration_seconds: u64,
    pub can_cancel: bool,
}

impl CooldownCounter {
    pub fn is_complete(&self, current_time: u64) -> bool {
        current_time >= self.started_at + self.duration_seconds
    }
    
    pub fn remaining_seconds(&self, current_time: u64) -> u64 {
        let end_time = self.started_at + self.duration_seconds;
        end_time.saturating_sub(current_time)
    }
}

/// Content identifier (simplified for MVP - would use actual CID library)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cid(pub String);

impl Cid {
    pub fn from_bytes(data: &[u8]) -> Self {
        let hash = blake3::hash(data);
        Cid(format!("blake3:{}", hash.to_hex()))
    }
}

/// Threshold signature wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdSig {
    #[serde(with = "signature_serde")]
    pub signature: Signature,
    pub signers: Vec<u16>, // Participant IDs
}

/// DKD commitment root for post-compaction verification
///
/// Reference: 080 spec Part 3: Ledger Compaction
/// When a DKD session is compacted, we persist the Merkle root of all commitments
/// so that recovery can verify guardian shares even after the original commitment
/// events have been pruned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdCommitmentRoot {
    pub session_id: Uuid,
    pub merkle_root: [u8; 32],
    pub created_at: u64,
}

/// Merkle proof for a guardian share commitment
///
/// Reference: 080 spec Part 3: Ledger Compaction
/// Guardians must store their Merkle proof so they can prove their
/// commitment was included in the DKD session after compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    pub commitment_hash: [u8; 32],
    pub siblings: Vec<[u8; 32]>,
    pub path_indices: Vec<bool>, // true = right, false = left
}

impl MerkleProof {
    /// Verify this proof against a Merkle root
    pub fn verify(&self, root: &[u8; 32]) -> bool {
        let mut current_hash = self.commitment_hash;
        
        for (sibling, is_right) in self.siblings.iter().zip(self.path_indices.iter()) {
            current_hash = if *is_right {
                // Current is left child
                compute_parent_hash(&current_hash, sibling)
            } else {
                // Current is right child
                compute_parent_hash(sibling, &current_hash)
            };
        }
        
        &current_hash == root
    }
}

/// Compute parent hash in Merkle tree
fn compute_parent_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(left);
    hasher.update(right);
    *hasher.finalize().as_bytes()
}

/// Distributed operation lock
///
/// Reference: 080 spec Part 3: Distributed Locking
/// Only one critical operation (DKD, Resharing, Recovery) can run at a time
/// to prevent concurrent protocol execution and ensure consistency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationLock {
    pub operation_type: OperationType,
    pub session_id: Uuid,
    pub holder_device_id: DeviceId,
    pub granted_at_epoch: u64,
    pub lottery_ticket: [u8; 32], // Hash used for deterministic lottery
}

/// Type of critical operation that requires distributed locking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationType {
    Dkd,
    Resharing,
    Recovery,
    Compaction,
}

/// Participant identifier for protocols (can be device or guardian)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum ParticipantId {
    Device(DeviceId),
    Guardian(GuardianId),
}

impl From<DeviceId> for ParticipantId {
    fn from(device_id: DeviceId) -> Self {
        ParticipantId::Device(device_id)
    }
}

impl From<GuardianId> for ParticipantId {
    fn from(guardian_id: GuardianId) -> Self {
        ParticipantId::Guardian(guardian_id)
    }
}

impl std::fmt::Display for ParticipantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParticipantId::Device(id) => write!(f, "device:{}", id.0),
            ParticipantId::Guardian(id) => write!(f, "guardian:{}", id.0),
        }
    }
}

/// A generic representation of any long-running, multi-party choreography
///
/// This struct provides unified state management for all distributed protocols
/// in the Aura system, enabling consistent monitoring, recovery, and UX.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier
    pub session_id: Uuid,
    
    /// Type of protocol being executed
    pub protocol_type: ProtocolType,
    
    /// Current lifecycle status
    pub status: SessionStatus,
    
    /// Participants involved in this session
    pub participants: Vec<ParticipantId>,
    
    /// Epoch when session was initiated
    pub start_epoch: u64,
    
    /// Maximum epochs this session can run before timing out
    pub ttl_in_epochs: u64,
    
    /// Final outcome once session reaches terminal state
    pub outcome: Option<SessionOutcome>,
    
    /// Timestamp when session was created
    pub created_at: u64,
    
    /// Timestamp when session was last updated
    pub updated_at: u64,
}

impl Session {
    /// Create a new session
    pub fn new(
        session_id: Uuid,
        protocol_type: ProtocolType,
        participants: Vec<ParticipantId>,
        start_epoch: u64,
        ttl_in_epochs: u64,
        timestamp: u64,
    ) -> Self {
        Self {
            session_id,
            protocol_type,
            status: SessionStatus::Pending,
            participants,
            start_epoch,
            ttl_in_epochs,
            outcome: None,
            created_at: timestamp,
            updated_at: timestamp,
        }
    }
    
    /// Check if session has timed out
    pub fn is_timed_out(&self, current_epoch: u64) -> bool {
        current_epoch > self.start_epoch + self.ttl_in_epochs
    }
    
    /// Check if session is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            SessionStatus::Aborted | SessionStatus::Completed | SessionStatus::TimedOut
        )
    }
    
    /// Update session status
    pub fn update_status(&mut self, status: SessionStatus, timestamp: u64) {
        self.status = status;
        self.updated_at = timestamp;
    }
    
    /// Complete session with outcome
    pub fn complete(&mut self, outcome: SessionOutcome, timestamp: u64) {
        self.status = SessionStatus::Completed;
        self.outcome = Some(outcome);
        self.updated_at = timestamp;
    }
    
    /// Abort session with failure outcome
    pub fn abort(&mut self, reason: String, blamed_party: Option<ParticipantId>, timestamp: u64) {
        self.status = SessionStatus::Aborted;
        self.outcome = Some(SessionOutcome::Failure { reason, blamed_party });
        self.updated_at = timestamp;
    }
}

/// The specific protocol this session is executing
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ProtocolType {
    /// Genesis distributed key generation (account creation)
    GenesisDkg,
    
    /// Key resharing to change threshold or participants
    Resharing,
    
    /// Guardian-based account recovery
    GuardianRecovery,
    
    /// Distributed lock acquisition for critical operations
    LockAcquisition,
    
    /// Standard DKD (Distributed Key Derivation) session
    Dkd,
    
    /// Account compaction to reduce storage
    Compaction,
}

/// The current lifecycle status of the session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    /// Waiting for participants to begin
    Pending,
    
    /// Protocol actively in progress
    Active,
    
    /// In a mandatory waiting period (e.g., guardian recovery cooldown)
    Cooldown,
    
    /// Terminated due to an error or fault
    Aborted,
    
    /// Successfully finished
    Completed,
    
    /// Terminated due to inactivity
    TimedOut,
}

/// The final result of a terminal session
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionOutcome {
    /// Session completed successfully
    Success,
    
    /// Session failed with details
    Failure {
        /// Human-readable reason for failure
        reason: String,
        
        /// Optional participant to blame for the failure
        blamed_party: Option<ParticipantId>,
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

