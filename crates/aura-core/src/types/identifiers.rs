//! Core identifier types used across the Aura platform
//!
//! This module provides the fundamental identifier types that uniquely identify
//! various entities and concepts within the Aura system.
//!
//! # Identifier Patterns
//!
//! Identifiers are generated using declarative macros to reduce boilerplate:
//! - `uuid_id!`: UUID-backed identifiers with standard traits
//! - `hash_id!`: Hash32-backed identifiers for content-addressed data
//! - `string_id!`: String-backed identifiers for human-readable names

use crate::{crypto::hash, AuraError, Hash32};
use hex;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

fn derived_uuid(label: &[u8]) -> Uuid {
    let digest = hash::hash(label);
    let mut uuid_bytes = [0u8; 16];
    uuid_bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(uuid_bytes)
}

// ============================================================================
// Identifier Generation Macros
// ============================================================================

/// Generate a UUID-backed identifier type with standard traits and methods.
///
/// # Generated Methods
/// - `new_from_entropy(entropy: [u8; 32])`: Create from caller-provided entropy
/// - `from_entropy(entropy: [u8; 32])`: Create from caller-provided entropy (alias)
/// - `from_uuid(uuid: Uuid)`: Create from UUID
/// - `uuid()`: Get inner UUID
///
/// # Generated Traits
/// - Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize
/// - No Default implementation (avoids deterministic sentinels)
/// - Display (with optional prefix)
/// - FromStr (parses UUID, optionally with prefix)
/// - From<Uuid>, From<Self> for Uuid
macro_rules! uuid_id {
    (
        $(#[$meta:meta])*
        $name:ident,
        label: $label:expr,
        prefix: $prefix:expr
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub Uuid);

        impl $name {
            /// Create from caller-provided entropy
            pub fn new_from_entropy(entropy: [u8; 32]) -> Self {
                let mut uuid_bytes = [0u8; 16];
                uuid_bytes.copy_from_slice(&entropy[..16]);
                Self(Uuid::from_bytes(uuid_bytes))
            }

            /// Create from caller-provided entropy (alias)
            pub fn from_entropy(entropy: [u8; 32]) -> Self {
                Self::new_from_entropy(entropy)
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

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                if $prefix.is_empty() {
                    write!(f, "{}", self.0)
                } else {
                    write!(f, "{}{}", $prefix, self.0)
                }
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let uuid_str = if $prefix.is_empty() {
                    s
                } else {
                    s.strip_prefix($prefix).unwrap_or(s)
                };
                Ok(Self(Uuid::parse_str(uuid_str)?))
            }
        }

        impl From<Uuid> for $name {
            fn from(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }
    };
}

/// Generate a string-backed identifier type with standard traits.
///
/// # Generated Methods
/// - `new(id: impl Into<String>)`: Create from string
/// - `as_str()`: Get inner string reference
///
/// # Generated Traits
/// - Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize
/// - Display (with prefix)
/// - From<String>, From<&str>
macro_rules! string_id {
    (
        $(#[$meta:meta])*
        $name:ident,
        prefix: $prefix:expr
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub String);

        impl $name {
            /// Create a new identifier
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            /// Get the inner string
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}{}", $prefix, self.0)
            }
        }

        impl From<String> for $name {
            fn from(id: String) -> Self {
                Self(id)
            }
        }

        impl From<&str> for $name {
            fn from(id: &str) -> Self {
                Self(id.to_string())
            }
        }
    };
}

/// Generate a Hash32-backed identifier type with standard traits.
///
/// # Generated Methods
/// - `new(id: Hash32)`: Create from Hash32
/// - `from_bytes(bytes: [u8; 32])`: Create from raw bytes
/// - `as_bytes()`: Get raw bytes reference
///
/// # Generated Traits
/// - Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default
/// - Display (with prefix, hex-encoded)
/// - FromStr (parses hex, optionally with prefix)
/// - From<[u8; 32]>
macro_rules! hash_id {
    (
        $(#[$meta:meta])*
        $name:ident,
        prefix: $prefix:expr
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
        pub struct $name(pub Hash32);

        impl $name {
            /// Create from a Hash32
            pub fn new(id: Hash32) -> Self {
                Self(id)
            }

            /// Create from raw bytes
            pub fn from_bytes(bytes: [u8; 32]) -> Self {
                Self(Hash32::new(bytes))
            }

            /// Get the raw bytes
            pub fn as_bytes(&self) -> &[u8; 32] {
                self.0.as_bytes()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}:{}", $prefix, hex::encode(self.0.as_bytes()))
            }
        }

        impl FromStr for $name {
            type Err = hex::FromHexError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let hex_str = s.strip_prefix(concat!($prefix, ":")).unwrap_or(s);
                let bytes = hex::decode(hex_str)?;
                if bytes.len() != 32 {
                    return Err(hex::FromHexError::InvalidStringLength);
                }
                let mut array = [0u8; 32];
                array.copy_from_slice(&bytes);
                Ok(Self::from_bytes(array))
            }
        }

        impl From<[u8; 32]> for $name {
            fn from(bytes: [u8; 32]) -> Self {
                Self::from_bytes(bytes)
            }
        }
    };
}

// ============================================================================
// UUID-backed Identifiers
// ============================================================================

uuid_id!(
    /// Session identifier for protocol sessions and coordination
    ///
    /// Used to uniquely identify sessions across all protocol types (DKD, resharing,
    /// recovery, locking, etc.) and ensure session-specific operations are isolated.
    SessionId,
    label: b"session-id",
    prefix: "session-"
);

uuid_id!(
    /// Event identifier for journal events
    ///
    /// Uniquely identifies events within the journal/effect API system.
    EventId,
    label: b"event-id",
    prefix: "event-"
);

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
    pub fn next(self) -> Result<Self, AuraError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| AuraError::invalid("EventNonce overflow"))
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

// ============================================================================
// String-backed Identifiers
// ============================================================================

string_id!(
    /// Member identifier for group membership
    ///
    /// Identifies members within groups or organizational structures.
    MemberId,
    prefix: "member-"
);

string_id!(
    /// Individual identifier for identity management
    ///
    /// Identifies individual persons or entities within the identity system.
    IndividualId,
    prefix: "individual-"
);

string_id!(
    /// Invitation identifier for relationship and enrollment invites
    ///
    /// Stored as a string to preserve existing prefixed formats (e.g. "inv-...").
    InvitationId,
    prefix: ""
);

string_id!(
    /// Recovery ceremony identifier for guardian-based recovery flows
    ///
    /// Stored as a string to preserve existing prefixed formats (e.g. "recovery-...").
    RecoveryId,
    prefix: ""
);

string_id!(
    /// Guardian ceremony identifier for key-rotation and enrollment flows
    ///
    /// Stored as a string to preserve existing prefixed formats.
    CeremonyId,
    prefix: ""
);

uuid_id!(
    /// Operation identifier for tracking operations
    ///
    /// Identifies specific operations across the system.
    OperationId,
    label: b"operation-id",
    prefix: "operation-"
);

/// Device identifier for distinguishing different devices in a threshold account
///
/// Each device in an Aura account has a unique DeviceId that identifies it within
/// the threshold scheme. Devices collectively hold shares of the root key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct DeviceId(pub Uuid);

impl DeviceId {
    /// Create a device ID from 32 bytes of caller-provided entropy (effect-injected).
    pub fn new_from_entropy(entropy: [u8; 32]) -> Self {
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&entropy[..16]);
        Self(Uuid::from_bytes(uuid_bytes))
    }
    /// Create from caller-provided entropy (alias)
    pub fn from_entropy(entropy: [u8; 32]) -> Self {
        Self::new_from_entropy(entropy)
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

    /// Convert to hex string
    ///
    /// Returns the UUID as a hexadecimal string (without hyphens).
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.as_bytes())
    }

    /// Get the inner UUID
    pub fn uuid(&self) -> Uuid {
        self.0
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

impl From<[u8; 32]> for DeviceId {
    fn from(bytes: [u8; 32]) -> Self {
        Self::from_bytes(bytes)
    }
}

uuid_id!(
    /// Guardian identifier for social recovery guardians
    ///
    /// Guardians are trusted third parties that can help recover account access
    /// if the user loses their devices. Each guardian has a unique GuardianId.
    GuardianId,
    label: b"guardian-id",
    prefix: ""
);

uuid_id!(
    /// Account identifier for distinguishing different Aura accounts
    ///
    /// Each Aura account has a unique AccountId. Users may have multiple accounts,
    /// and this ID uniquely identifies each one.
    AccountId,
    label: b"account-id",
    prefix: ""
);

impl AccountId {
    /// Create from 32 bytes (for testing)
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&bytes[..16]);
        Self(Uuid::from_bytes(uuid_bytes))
    }
}

uuid_id!(
    /// Authority identifier - primary identifier for authorities in the new model
    ///
    /// Represents an opaque cryptographic authority that can sign operations and
    /// hold state. Replaces AccountId in the authority-centric architecture.
    /// Authorities are self-contained entities with internal device structure
    /// that is not exposed externally.
    AuthorityId,
    label: b"authority-id",
    prefix: "authority-"
);

impl AuthorityId {
    /// Convert to bytes
    pub fn to_bytes(&self) -> [u8; 16] {
        self.0.into_bytes()
    }
}

// ============================================================================
// Hash32-backed Identifiers
// ============================================================================

hash_id!(
    /// Channel identifier for AMP messaging substreams
    ///
    /// Channels are scoped under a RelationalContext. The identifier is opaque and
    /// does not reveal membership or topology.
    ChannelId,
    prefix: "channel"
);

hash_id!(
    /// Home identifier for social topology homes
    ///
    /// Homes are storage/relay containers in the social architecture. Each home
    /// has storage limits, residents, and neighborhood memberships.
    HomeId,
    prefix: "home"
);

hash_id!(
    /// Neighborhood identifier for inter-home connections
    ///
    /// Neighborhoods connect multiple homes for relay and storage sharing.
    NeighborhoodId,
    prefix: "neighborhood"
);

uuid_id!(
    /// Context identifier for RelationalContexts
    ///
    /// Identifies a RelationalContext that manages cross-authority relationships.
    /// ContextIds are opaque and never encode participant data or authority structure.
    ContextId,
    label: b"context-id",
    prefix: "context-"
);

impl ContextId {
    /// Get bytes representation
    pub fn to_bytes(&self) -> [u8; 16] {
        *self.0.as_bytes()
    }

    /// Get bytes as slice
    pub fn as_bytes(&self) -> &[u8; 16] {
        self.0.as_bytes()
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
        Self(format!("dkd:{context}:{fingerprint_hex}"))
    }
}

/// Data identifier for stored data chunks
///
/// Identifies data stored in the Aura storage system.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DataId(pub String);

impl DataId {
    /// Create a new data ID (deterministic hash-derived)
    pub fn new() -> Self {
        Self(Self::derive_tagged("data:"))
    }

    /// Create an encrypted data ID (deterministic hash-derived)
    pub fn new_encrypted() -> Self {
        Self(Self::derive_tagged("encrypted:"))
    }

    // Effects-based methods moved to aura-effects

    fn derive_tagged(prefix: &str) -> String {
        let uuid = derived_uuid(prefix.as_bytes());
        format!("{prefix}{uuid}")
    }

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
        let mut h = hash::hasher();
        h.update(b"AURA_RELAY_ID");

        // Ensure deterministic ordering
        if device_a < device_b {
            h.update(device_a.0.as_bytes());
            h.update(device_b.0.as_bytes());
        } else {
            h.update(device_b.0.as_bytes());
            h.update(device_a.0.as_bytes());
        }

        Self(h.finalize())
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
        let mut h = hash::hasher();
        h.update(b"AURA_GROUP_ID");
        h.update(&threshold.to_le_bytes());

        // Sort members for deterministic ordering
        let mut sorted_members = members.to_vec();
        sorted_members.sort();

        for member in sorted_members {
            h.update(member.0.as_bytes());
        }

        Self(h.finalize())
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
        let mut h = hash::hasher();
        match self {
            MessageContext::Relay(relay_id) => {
                h.update(b"RELAY");
                h.update(relay_id.as_bytes());
            }
            MessageContext::Group(group_id) => {
                h.update(b"GROUP");
                h.update(group_id.as_bytes());
            }
            MessageContext::DkdContext(dkd_id) => {
                h.update(b"DKD");
                h.update(dkd_id.app_label.as_bytes());
                h.update(&dkd_id.fingerprint);
            }
        }
        h.finalize()
    }
}

impl fmt::Display for MessageContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessageContext::Relay(relay_id) => write!(f, "{relay_id}"),
            MessageContext::Group(group_id) => write!(f, "{group_id}"),
            MessageContext::DkdContext(dkd_id) => write!(f, "{dkd_id}"),
        }
    }
}
