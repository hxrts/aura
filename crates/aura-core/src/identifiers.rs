//! Core identifier types used across the Aura platform
//!
//! This module provides the fundamental identifier types that uniquely identify
//! various entities and concepts within the Aura system.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

/// Session identifier for protocol sessions and coordination
///
/// Used to uniquely identify sessions across all protocol types (DKD, resharing,
/// recovery, locking, etc.) and ensure session-specific operations are isolated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    /// Create a new random session ID
    #[allow(clippy::disallowed_methods)]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Get the inner UUID
    pub fn uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "session-{}", self.0)
    }
}

impl From<Uuid> for SessionId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<SessionId> for Uuid {
    fn from(session_id: SessionId) -> Self {
        session_id.0
    }
}

/// Event identifier for journal events
///
/// Uniquely identifies events within the journal/ledger system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EventId(pub Uuid);

impl EventId {
    /// Create a new random event ID
    #[allow(clippy::disallowed_methods)]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Get the inner UUID
    pub fn uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for EventId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "event-{}", self.0)
    }
}

impl From<Uuid> for EventId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<EventId> for Uuid {
    fn from(event_id: EventId) -> Self {
        event_id.0
    }
}

// EventIdExt moved to aura-effects to maintain clean interface layer

/// Event nonce for ordering and uniqueness
///
/// Provides ordering guarantees for events within sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EventNonce(pub u64);

impl EventNonce {
    /// Create a new event nonce
    pub fn new(nonce: u64) -> Self {
        Self(nonce)
    }

    /// Get the inner nonce value
    pub fn value(&self) -> u64 {
        self.0
    }

    /// Get the next nonce in sequence
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

impl fmt::Display for EventNonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "nonce-{}", self.0)
    }
}

impl From<u64> for EventNonce {
    fn from(nonce: u64) -> Self {
        Self(nonce)
    }
}

impl From<EventNonce> for u64 {
    fn from(nonce: EventNonce) -> Self {
        nonce.0
    }
}

/// Member identifier for group membership
///
/// Identifies members within groups or organizational structures.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MemberId(pub String);

impl MemberId {
    /// Create a new member ID
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the inner string
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MemberId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "member-{}", self.0)
    }
}

impl From<String> for MemberId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for MemberId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

/// Individual identifier for identity management
///
/// Identifies individual persons or entities within the identity system.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct IndividualId(pub String);

impl IndividualId {
    /// Create a new individual ID
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the inner string
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IndividualId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "individual-{}", self.0)
    }
}

impl From<String> for IndividualId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for IndividualId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

/// Operation identifier for tracking operations
///
/// Identifies specific operations across the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OperationId(pub Uuid);

impl OperationId {
    /// Create a new random operation ID
    #[allow(clippy::disallowed_methods)]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Get the inner UUID
    pub fn uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for OperationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "operation-{}", self.0)
    }
}

impl From<Uuid> for OperationId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<OperationId> for Uuid {
    fn from(operation_id: OperationId) -> Self {
        operation_id.0
    }
}

/// Device identifier for distinguishing different devices in a threshold account
///
/// Each device in an Aura account has a unique DeviceId that identifies it within
/// the threshold scheme. Devices collectively hold shares of the root key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct DeviceId(pub Uuid);

impl DeviceId {
    /// Create a new random device ID
    #[allow(clippy::disallowed_methods)]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create a placeholder device ID for temporary use
    pub fn placeholder() -> Self {
        Self(Uuid::nil())
    }

    /// Create from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Create from 32 bytes (for testing)
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        // Take first 16 bytes for UUID
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&bytes[..16]);
        Self(Uuid::from_bytes(uuid_bytes))
    }

    /// Convert to 32 bytes (compatible with from_bytes)
    pub fn to_bytes(&self) -> Result<[u8; 32], &'static str> {
        let uuid_bytes = self.0.as_bytes();
        let mut result = [0u8; 32];
        result[..16].copy_from_slice(uuid_bytes);
        // Fill rest with zeros for consistent 32-byte format
        Ok(result)
    }
}

impl Default for DeviceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for DeviceId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(DeviceId(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for DeviceId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<DeviceId> for Uuid {
    fn from(device_id: DeviceId) -> Self {
        device_id.0
    }
}

impl From<&str> for DeviceId {
    fn from(s: &str) -> Self {
        DeviceId::from_str(s).unwrap_or_else(|_| {
            // Create a deterministic UUID from the string if parsing fails
            let namespace = Uuid::NAMESPACE_DNS;
            DeviceId(Uuid::new_v5(&namespace, s.as_bytes()))
        })
    }
}

/// Guardian identifier for social recovery guardians
///
/// Guardians are trusted third parties that can help recover account access
/// if the user loses their devices. Each guardian has a unique GuardianId.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct GuardianId(pub Uuid);

impl GuardianId {
    /// Create a new random guardian ID
    #[allow(clippy::disallowed_methods)]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl Default for GuardianId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for GuardianId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for GuardianId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(GuardianId(Uuid::parse_str(s)?))
    }
}

/// Account identifier for distinguishing different Aura accounts
///
/// Each Aura account has a unique AccountId. Users may have multiple accounts,
/// and this ID uniquely identifies each one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AccountId(pub Uuid);

impl AccountId {
    /// Create a new random account ID
    #[allow(clippy::disallowed_methods)]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Create from 32 bytes (for testing)
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        // Take first 16 bytes for UUID
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&bytes[..16]);
        Self(Uuid::from_bytes(uuid_bytes))
    }
}

impl Default for AccountId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for AccountId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(AccountId(Uuid::parse_str(s)?))
    }
}

// Extension traits for Effects-based ID generation moved to aura-effects
// to maintain clean interface layer separation

/// Extension trait for IndividualId with additional utility methods
pub trait IndividualIdExt {
    /// Create from device ID (device-specific identity)
    fn from_device(device_id: &DeviceId) -> Self;
    /// Create from DKD context (derived identity)
    fn from_dkd_context(context: &str, fingerprint: &[u8; 32]) -> Self;
}

impl IndividualIdExt for IndividualId {
    fn from_device(device_id: &DeviceId) -> Self {
        Self(format!("device:{}", device_id.0))
    }

    fn from_dkd_context(context: &str, fingerprint: &[u8; 32]) -> Self {
        let fingerprint_hex = hex::encode(fingerprint);
        Self(format!("dkd:{}:{}", context, fingerprint_hex))
    }
}

/// Data identifier for stored data chunks
///
/// Identifies data stored in the Aura storage system.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DataId(pub String);

impl DataId {
    /// Create a new random data ID
    #[allow(clippy::disallowed_methods)]
    pub fn new() -> Self {
        Self(format!("data:{}", Uuid::new_v4()))
    }

    /// Create an encrypted data ID
    #[allow(clippy::disallowed_methods)]
    pub fn new_encrypted() -> Self {
        Self(format!("encrypted:{}", Uuid::new_v4()))
    }

    // Effects-based methods moved to aura-effects

    /// Get the inner string
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for DataId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for DataId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for DataId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for DataId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

/// Relay identifier for pairwise communication contexts
///
/// Identifies a pairwise communication context between two parties using X25519-derived keys.
/// Forms the foundation for RID (Relay ID) message contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RelayId(pub [u8; 32]);

impl RelayId {
    /// Create a new relay ID from 32 bytes
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Create from two device IDs (deterministic)
    pub fn from_devices(device_a: &DeviceId, device_b: &DeviceId) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"AURA_RELAY_ID");

        // Ensure deterministic ordering
        if device_a < device_b {
            hasher.update(device_a.0.as_bytes());
            hasher.update(device_b.0.as_bytes());
        } else {
            hasher.update(device_b.0.as_bytes());
            hasher.update(device_a.0.as_bytes());
        }

        Self(*hasher.finalize().as_bytes())
    }

    /// Get the raw bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for RelayId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "relay:{}", hex::encode(self.0))
    }
}

impl From<[u8; 32]> for RelayId {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

/// Group identifier for threshold group contexts
///
/// Identifies a threshold group communication context derived from group membership.
/// Forms the foundation for GID (Group ID) message contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GroupId(pub [u8; 32]);

impl GroupId {
    /// Create a new group ID from 32 bytes
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Create from group members and threshold (deterministic)
    pub fn from_threshold_config(members: &[DeviceId], threshold: u16) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"AURA_GROUP_ID");
        hasher.update(&threshold.to_le_bytes());

        // Sort members for deterministic ordering
        let mut sorted_members = members.to_vec();
        sorted_members.sort();

        for member in sorted_members {
            hasher.update(member.0.as_bytes());
        }

        Self(*hasher.finalize().as_bytes())
    }

    /// Get the raw bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "group:{}", hex::encode(self.0))
    }
}

impl From<[u8; 32]> for GroupId {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

/// DKD context identifier for deterministic key derivation contexts
///
/// Identifies a DKD (Deterministic Key Derivation) context with application label and fingerprint.
/// Used for privacy-preserving key derivation across different application contexts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DkdContextId {
    /// Application label (e.g., "messaging", "storage", "identity")
    pub app_label: String,
    /// Fingerprint for the specific context instance
    pub fingerprint: [u8; 32],
}

impl DkdContextId {
    /// Create a new DKD context ID
    pub fn new(app_label: impl Into<String>, fingerprint: [u8; 32]) -> Self {
        Self {
            app_label: app_label.into(),
            fingerprint,
        }
    }

    /// Create from context string and fingerprint (matching existing usage)
    pub fn from_context(context: &str, fingerprint: &[u8; 32]) -> Self {
        Self::new(context, *fingerprint)
    }

    /// Get the application label
    pub fn app_label(&self) -> &str {
        &self.app_label
    }

    /// Get the fingerprint
    pub fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
}

impl fmt::Display for DkdContextId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "dkd:{}:{}",
            self.app_label,
            hex::encode(self.fingerprint)
        )
    }
}

/// Unified message context for privacy partitions
///
/// Represents the three types of privacy contexts in the Aura system:
/// - RID: Pairwise relay contexts for two-party communication
/// - GID: Group contexts for threshold protocols
/// - DKD: Application-scoped derived contexts
///
/// This enforces the privacy partition invariant: messages from different contexts
/// cannot flow into each other without explicit bridge protocols.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MessageContext {
    /// Pairwise relay context (X25519-derived)
    Relay(RelayId),
    /// Group threshold context (threshold-derived)
    Group(GroupId),
    /// DKD application context (deterministic derivation)
    DkdContext(DkdContextId),
}

impl MessageContext {
    /// Create a relay context from two devices
    pub fn relay_between(device_a: &DeviceId, device_b: &DeviceId) -> Self {
        Self::Relay(RelayId::from_devices(device_a, device_b))
    }

    /// Create a group context from threshold configuration
    pub fn group_threshold(members: &[DeviceId], threshold: u16) -> Self {
        Self::Group(GroupId::from_threshold_config(members, threshold))
    }

    /// Create a DKD context
    pub fn dkd_context(app_label: impl Into<String>, fingerprint: [u8; 32]) -> Self {
        Self::DkdContext(DkdContextId::new(app_label, fingerprint))
    }

    /// Check if this context is compatible with another for message flow
    ///
    /// Returns true only if contexts are identical. Cross-context message flow
    /// requires explicit bridge protocols.
    pub fn is_compatible_with(&self, other: &MessageContext) -> bool {
        self == other
    }

    /// Get a unique identifier for this context (for routing/indexing)
    pub fn context_hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        match self {
            MessageContext::Relay(relay_id) => {
                hasher.update(b"RELAY");
                hasher.update(relay_id.as_bytes());
            }
            MessageContext::Group(group_id) => {
                hasher.update(b"GROUP");
                hasher.update(group_id.as_bytes());
            }
            MessageContext::DkdContext(dkd_id) => {
                hasher.update(b"DKD");
                hasher.update(dkd_id.app_label.as_bytes());
                hasher.update(&dkd_id.fingerprint);
            }
        }
        *hasher.finalize().as_bytes()
    }
}

impl fmt::Display for MessageContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessageContext::Relay(relay_id) => write!(f, "{}", relay_id),
            MessageContext::Group(group_id) => write!(f, "{}", group_id),
            MessageContext::DkdContext(dkd_id) => write!(f, "{}", dkd_id),
        }
    }
}
