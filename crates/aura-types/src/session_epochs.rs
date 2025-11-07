//! Session epochs, participant IDs, and session status enums
//!
//! This module provides types for managing session epochs, protocol participants,
//! session lifecycle status, and outcomes across distributed protocols.

// Session identifiers will be imported when needed
use crate::{DeviceId, GuardianId};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unified participant identifier that can represent different types of participants
///
/// This enum allows protocols to work with different kinds of participants
/// (devices and guardians) in a type-safe manner.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ParticipantId {
    /// A device participant (from aura-crypto DeviceId)
    Device(DeviceId),
    /// A guardian participant (from aura-crypto GuardianId)
    Guardian(GuardianId),
}

impl ParticipantId {
    /// Get the underlying UUID regardless of participant type
    pub fn uuid(&self) -> Uuid {
        match self {
            ParticipantId::Device(device_id) => device_id.0,
            ParticipantId::Guardian(guardian_id) => guardian_id.0,
        }
    }

    /// Check if this is a device participant
    pub fn is_device(&self) -> bool {
        matches!(self, ParticipantId::Device(_))
    }

    /// Check if this is a guardian participant
    pub fn is_guardian(&self) -> bool {
        matches!(self, ParticipantId::Guardian(_))
    }

    /// Get device ID if this is a device participant
    pub fn as_device(&self) -> Option<&DeviceId> {
        match self {
            ParticipantId::Device(device_id) => Some(device_id),
            ParticipantId::Guardian(_) => None,
        }
    }

    /// Get guardian ID if this is a guardian participant
    pub fn as_guardian(&self) -> Option<&GuardianId> {
        match self {
            ParticipantId::Device(_) => None,
            ParticipantId::Guardian(guardian_id) => Some(guardian_id),
        }
    }
}

impl fmt::Display for ParticipantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParticipantId::Device(device_id) => write!(f, "device-{}", device_id.0),
            ParticipantId::Guardian(guardian_id) => write!(f, "guardian-{}", guardian_id.0),
        }
    }
}

/// General-purpose epoch for versioning and coordination.
///
/// All epoch counters in Aura share the same semantics: they are monotonic
/// `u64` values that start at zero and advance by one. Subsidiary modules
/// (ledger session epochs, logical clocks, etc.) should alias this struct
/// to make their intent clear while keeping behaviour uniform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Epoch(pub u64);

impl Epoch {
    /// Create a new epoch
    pub fn new(epoch: u64) -> Self {
        Self(epoch)
    }

    /// Get the epoch value
    pub fn value(&self) -> u64 {
        self.0
    }

    /// Get the next epoch
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }

    /// Get the initial epoch (0)
    pub fn initial() -> Self {
        Self(0)
    }
}

impl fmt::Display for Epoch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "epoch-{}", self.0)
    }
}

impl From<u64> for Epoch {
    fn from(epoch: u64) -> Self {
        Self(epoch)
    }
}

impl From<Epoch> for u64 {
    fn from(epoch: Epoch) -> Self {
        epoch.0
    }
}

impl Default for Epoch {
    fn default() -> Self {
        Self::initial()
    }
}

/// Session-specific epoch counter.
///
/// This is an alias for [`Epoch`] that callers can use when they specifically
/// mean the session epoch maintained by the ledger. Retaining the alias keeps
/// call sites readable without introducing divergent implementations.
pub type SessionEpoch = Epoch;

/// Session status enumeration
///
/// Represents the current state of a protocol session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionStatus {
    /// Session is initializing (before active execution)
    Initializing,
    /// Session is currently active and executing
    Active,
    /// Session is waiting for responses from participants
    Waiting,
    /// Session completed successfully
    Completed,
    /// Session failed with an error
    Failed,
    /// Session expired due to timeout
    Expired,
    /// Session timed out during execution
    TimedOut,
    /// Session was cancelled
    Cancelled,
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionStatus::Initializing => write!(f, "initializing"),
            SessionStatus::Active => write!(f, "active"),
            SessionStatus::Waiting => write!(f, "waiting"),
            SessionStatus::Completed => write!(f, "completed"),
            SessionStatus::Failed => write!(f, "failed"),
            SessionStatus::Expired => write!(f, "expired"),
            SessionStatus::TimedOut => write!(f, "timed-out"),
            SessionStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Session outcome enumeration
///
/// Represents the final result of a protocol session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionOutcome {
    /// Session completed successfully
    Success,
    /// Session failed
    Failed,
    /// Session was aborted
    Aborted,
}

impl fmt::Display for SessionOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionOutcome::Success => write!(f, "success"),
            SessionOutcome::Failed => write!(f, "failed"),
            SessionOutcome::Aborted => write!(f, "aborted"),
        }
    }
}

/// Local session type for handler interfaces
///
/// Represents the type signature of session protocols used by handlers.
/// This is a placeholder type for compatibility with handler implementations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalSessionType {
    /// Protocol name
    pub protocol: String,
    /// Session parameters
    pub params: Vec<u8>,
}

impl LocalSessionType {
    /// Create a new local session type
    pub fn new(protocol: String, params: Vec<u8>) -> Self {
        Self { protocol, params }
    }

    /// Get the protocol name
    pub fn protocol(&self) -> &str {
        &self.protocol
    }

    /// Get the session parameters
    pub fn params(&self) -> &[u8] {
        &self.params
    }
}
