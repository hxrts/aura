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

    /// Create from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
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

// Extension traits for Effects-based ID generation (for deterministic testing)
// These traits allow IDs to be generated using injected randomness via Effects

/// Extension trait for DeviceId with Effects support
pub trait DeviceIdExt {
    /// Create a new device ID using Effects for deterministic randomness
    fn new_with_effects(effects: &impl crate::EffectsLike) -> Self;
    /// Create from a string identifier using Effects
    fn from_string_with_effects(id_str: &str, effects: &impl crate::EffectsLike) -> Self;
}

/// Extension trait for GuardianId with Effects support
pub trait GuardianIdExt {
    /// Create a new guardian ID using Effects for deterministic randomness
    fn new_with_effects(effects: &impl crate::EffectsLike) -> Self;
    /// Create from a string identifier using Effects
    fn from_string_with_effects(id_str: &str, effects: &impl crate::EffectsLike) -> Self;
}

/// Extension trait for AccountId with Effects support
pub trait AccountIdExt {
    /// Create a new account ID using Effects for deterministic randomness
    fn new_with_effects(effects: &impl crate::EffectsLike) -> Self;
    /// Create from a string identifier using Effects
    fn from_string_with_effects(id_str: &str, effects: &impl crate::EffectsLike) -> Self;
}

/// Trait for Effects-like objects that support UUID generation
/// Used to abstract over different Effects implementations for ID generation
pub trait EffectsLike {
    /// Generate a deterministic UUID
    fn gen_uuid(&self) -> Uuid;
}

impl DeviceIdExt for DeviceId {
    fn new_with_effects(effects: &impl EffectsLike) -> Self {
        DeviceId(effects.gen_uuid())
    }

    fn from_string_with_effects(id_str: &str, _effects: &impl EffectsLike) -> Self {
        // Create a deterministic UUID from the string
        let namespace = Uuid::NAMESPACE_DNS;
        DeviceId(Uuid::new_v5(&namespace, id_str.as_bytes()))
    }
}

impl GuardianIdExt for GuardianId {
    fn new_with_effects(effects: &impl EffectsLike) -> Self {
        GuardianId(effects.gen_uuid())
    }

    fn from_string_with_effects(id_str: &str, _effects: &impl EffectsLike) -> Self {
        // Create a deterministic UUID from the string
        let namespace = Uuid::NAMESPACE_DNS;
        GuardianId(Uuid::new_v5(&namespace, id_str.as_bytes()))
    }
}

impl AccountIdExt for AccountId {
    fn new_with_effects(effects: &impl EffectsLike) -> Self {
        AccountId(effects.gen_uuid())
    }

    fn from_string_with_effects(id_str: &str, _effects: &impl EffectsLike) -> Self {
        // Create a deterministic UUID from the string
        let namespace = Uuid::NAMESPACE_DNS;
        AccountId(Uuid::new_v5(&namespace, id_str.as_bytes()))
    }
}
