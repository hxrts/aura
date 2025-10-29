//! Protocol metadata and descriptors

use aura_types::AuraError;
use frost_ed25519 as frost;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU16;

// Import shared types from aura-types
pub use aura_types::SessionId;

/// Unique identifier for a participant in the threshold signing protocol
///
/// ParticipantId must be non-zero for FROST compatibility.
/// Use `new()` or `try_from()` to create validated instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParticipantId(NonZeroU16);

impl ParticipantId {
    /// Create a new ParticipantId from a non-zero value
    pub fn new(id: NonZeroU16) -> Self {
        ParticipantId(id)
    }

    /// Get the inner value as u16
    pub fn as_u16(&self) -> u16 {
        self.0.get()
    }

    /// Create a ParticipantId from a u16, panicking if zero
    ///
    /// **WARNING**: This method panics if id is zero. Only use in tests!
    /// Use `try_from()` for fallible conversion in production code.
    pub fn from_u16_unchecked(id: u16) -> Self {
        Self::try_from(id).expect("ParticipantId must be non-zero")
    }
}

impl TryFrom<u16> for ParticipantId {
    type Error = AuraError;

    fn try_from(id: u16) -> std::result::Result<Self, Self::Error> {
        NonZeroU16::new(id)
            .map(ParticipantId)
            .ok_or_else(|| AuraError::coordination_failed("Participant ID must be non-zero"))
    }
}

// Add trait implementations for ParticipantId
impl From<ParticipantId> for u16 {
    fn from(id: ParticipantId) -> Self {
        id.as_u16()
    }
}

impl From<ParticipantId> for frost::Identifier {
    fn from(id: ParticipantId) -> Self {
        // FROST identifiers must be non-zero - this is now guaranteed by type system
        // ParticipantId ensures the value is non-zero, so this conversion is infallible
        frost::Identifier::try_from(id.as_u16()).expect("ParticipantId guarantees non-zero value")
    }
}

/// Configuration for threshold signing setup
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThresholdConfig {
    /// Minimum number of participants required (M in M-of-N)
    pub threshold: u16,
    /// Total number of participants (N in M-of-N)
    pub total_participants: u16,
}

impl ThresholdConfig {
    pub fn new(threshold: u16, total_participants: u16) -> Result<Self, AuraError> {
        if threshold == 0 || threshold > total_participants {
            return Err(AuraError::coordination_failed(format!(
                "Invalid threshold: {} must be between 1 and {}",
                threshold, total_participants
            )));
        }
        Ok(ThresholdConfig {
            threshold,
            total_participants,
        })
    }

    /// Default 2-of-3 configuration for MVP
    pub fn default_2_of_3() -> Self {
        ThresholdConfig {
            threshold: 2,
            total_participants: 3,
        }
    }
}
