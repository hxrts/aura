//! Session epochs and participant IDs
//!
//! This module provides foundation types for managing session epochs and protocol participants.
//! Session orchestration types (SessionStatus, SessionOutcome) have moved to aura-protocol.
//! Session type system types (LocalSessionType) have moved to aura-mpst.

use crate::types::identifiers::DeviceId;
use crate::{AuraError, GuardianId};
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
/// (effect API session epochs, logical clocks, etc.) should alias this struct
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
    pub fn next(self) -> Result<Self, AuraError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| AuraError::invalid("Epoch overflow"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_next_increments() {
        let epoch = Epoch::new(41);
        let next = epoch.next().expect("epoch increment should succeed");
        assert_eq!(next.value(), 42);
    }

    #[test]
    fn epoch_next_overflow() {
        let epoch = Epoch::new(u64::MAX);
        let err = epoch.next().unwrap_err();
        assert!(err.to_string().contains("Epoch overflow"));
    }
}

/// Session-specific epoch counter.
///
/// This is an alias for [`Epoch`] that callers can use when they specifically
/// mean the session epoch maintained by the effect API. Retaining the alias keeps
/// call sites readable without introducing divergent implementations.
pub type SessionEpoch = Epoch;
