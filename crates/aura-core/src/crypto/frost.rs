//! FROST threshold cryptography primitives
//!
//! This module provides types for FROST (Flexible Round-Optimized Schnorr Threshold)
//! signatures, including participant identifiers and threshold configuration.

use crate::threshold::AgreementMode;
use crate::types::participants::ParticipantIdentity;
use crate::AuraError;
use frost_ed25519 as frost;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU16;

// Re-export tree signing primitives (actual FROST implementation)
pub use crate::crypto::tree_signing::*;

/// Configuration for threshold signing setup
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThresholdConfig {
    /// Minimum number of participants required (M in M-of-N)
    pub threshold: u16,
    /// Total number of participants (N in M-of-N)
    pub total_participants: u16,
}

impl ThresholdConfig {
    /// Create a new threshold configuration with validation
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

/// Full threshold state including epoch and guardian information
///
/// This extends `ThresholdConfig` with the epoch number and list of guardian
/// authority IDs. Used by the recovery system to understand the current
/// guardian configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThresholdState {
    /// Current epoch for this configuration
    pub epoch: u64,
    /// Minimum number of participants required (k in k-of-n)
    pub threshold: u16,
    /// Total number of participants (n in k-of-n)
    pub total_participants: u16,
    /// Participants (in protocol participant order)
    pub participants: Vec<ParticipantIdentity>,
    /// Agreement mode (A1/A2/A3) for current keying state
    #[serde(default)]
    pub agreement_mode: AgreementMode,
}

impl ThresholdState {
    /// Create an empty state for when no guardians are configured
    pub fn empty() -> Self {
        ThresholdState {
            epoch: 0,
            threshold: 0,
            total_participants: 0,
            participants: Vec::new(),
            agreement_mode: AgreementMode::default(),
        }
    }

    /// Extract just the threshold configuration
    pub fn config(&self) -> ThresholdConfig {
        ThresholdConfig {
            threshold: self.threshold,
            total_participants: self.total_participants,
        }
    }
}

/// Unique identifier for a participant in the threshold signing protocol
///
/// FrostParticipantId must be non-zero for FROST compatibility.
/// Use `new()` or `try_from()` to create validated instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FrostParticipantId(NonZeroU16);

impl FrostParticipantId {
    /// Create a new FrostParticipantId from a non-zero value
    pub fn new(id: NonZeroU16) -> Self {
        FrostParticipantId(id)
    }

    /// Get the inner value as u16
    pub fn as_u16(&self) -> u16 {
        self.0.get()
    }

    /// Create a FrostParticipantId from a u16, panicking if zero
    ///
    /// **WARNING**: This method panics if id is zero. Only use in tests!
    /// Use `try_from()` for fallible conversion in production code.
    pub fn from_u16_unchecked(id: u16) -> Self {
        Self::try_from(id)
            .unwrap_or_else(|_| panic!("FrostParticipantId must be non-zero, got {}", id))
    }
}

impl TryFrom<u16> for FrostParticipantId {
    type Error = AuraError;

    fn try_from(id: u16) -> std::result::Result<Self, Self::Error> {
        NonZeroU16::new(id)
            .map(FrostParticipantId)
            .ok_or_else(|| AuraError::coordination_failed("Participant ID must be non-zero"))
    }
}

// Add trait implementations for FrostParticipantId
impl From<FrostParticipantId> for u16 {
    fn from(id: FrostParticipantId) -> Self {
        id.as_u16()
    }
}

impl From<FrostParticipantId> for frost::Identifier {
    fn from(id: FrostParticipantId) -> Self {
        // FROST identifiers must be non-zero - this is now guaranteed by type system
        // FrostParticipantId ensures the value is non-zero, so this conversion is infallible
        frost::Identifier::try_from(id.as_u16()).unwrap_or_else(|_| {
            // This is unreachable because FrostParticipantId guarantees non-zero value
            unreachable!("FrostParticipantId guarantees non-zero value, conversion must succeed")
        })
    }
}
