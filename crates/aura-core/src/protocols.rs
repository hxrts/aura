//! Protocol types and session metadata
//!
//! This module provides enumerations and types for different protocols supported
//! by the Aura platform, including threshold cryptography configuration and
//! protocol session coordination.

use crate::AuraError;
use frost_ed25519 as frost;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::num::NonZeroU16;

/// Protocol type enumeration
///
/// Identifies the type of protocol being executed in a session and is used for
/// dispatch/analytics across the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtocolType {
    /// Deterministic Key Derivation protocol
    Dkd,
    /// Counter reservation protocol
    Counter,
    /// Key resharing protocol for threshold updates
    Resharing,
    /// Resource locking protocol
    Locking,
    /// Lock acquisition protocol
    LockAcquisition,
    /// Effect API compaction protocol for state optimization
    Compaction,
}

impl fmt::Display for ProtocolType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolType::Dkd => write!(f, "dkd"),
            ProtocolType::Counter => write!(f, "counter"),
            ProtocolType::Resharing => write!(f, "resharing"),
            ProtocolType::Locking => write!(f, "locking"),
            ProtocolType::LockAcquisition => write!(f, "lock-acquisition"),
            ProtocolType::Compaction => write!(f, "compaction"),
        }
    }
}

impl ProtocolType {
    /// Get all protocol types
    pub fn all() -> &'static [ProtocolType] {
        &[
            ProtocolType::Dkd,
            ProtocolType::Counter,
            ProtocolType::Resharing,
            ProtocolType::Locking,
            ProtocolType::LockAcquisition,
            ProtocolType::Compaction,
        ]
    }

    /// Check if this protocol supports threshold operations
    pub fn supports_threshold(&self) -> bool {
        matches!(
            self,
            ProtocolType::Dkd | ProtocolType::Counter | ProtocolType::Resharing
        )
    }

    /// Check if this protocol modifies account state
    pub fn modifies_account_state(&self) -> bool {
        matches!(
            self,
            ProtocolType::Dkd
                | ProtocolType::Counter
                | ProtocolType::Resharing
                | ProtocolType::Compaction
        )
    }

    /// Get the typical duration category for this protocol
    pub fn duration_category(&self) -> ProtocolDuration {
        match self {
            ProtocolType::Dkd => ProtocolDuration::Short,
            ProtocolType::Counter => ProtocolDuration::Short,
            ProtocolType::Resharing => ProtocolDuration::Medium,
            ProtocolType::Locking => ProtocolDuration::Short,
            ProtocolType::LockAcquisition => ProtocolDuration::Short,
            ProtocolType::Compaction => ProtocolDuration::Medium,
        }
    }
}

/// Protocol duration categories
///
/// Indicates the expected duration category for different protocols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtocolDuration {
    /// Short duration (seconds to minutes)
    Short,
    /// Medium duration (minutes to hours)
    Medium,
    /// Long duration (hours to days)
    Long,
}

impl fmt::Display for ProtocolDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolDuration::Short => write!(f, "short"),
            ProtocolDuration::Medium => write!(f, "medium"),
            ProtocolDuration::Long => write!(f, "long"),
        }
    }
}

/// Protocol priority levels
///
/// Indicates the priority level for protocol execution and resource allocation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
pub enum ProtocolPriority {
    /// Low priority - background operations
    Low,
    /// Normal priority - standard operations
    #[default]
    Normal,
    /// High priority - important operations
    High,
    /// Critical priority - security or recovery operations
    Critical,
}

impl fmt::Display for ProtocolPriority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolPriority::Low => write!(f, "low"),
            ProtocolPriority::Normal => write!(f, "normal"),
            ProtocolPriority::High => write!(f, "high"),
            ProtocolPriority::Critical => write!(f, "critical"),
        }
    }
}

/// Protocol execution mode
///
/// Indicates how a protocol should be executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ProtocolMode {
    /// Synchronous execution - wait for completion
    Synchronous,
    /// Asynchronous execution - run in background
    #[default]
    Asynchronous,
    /// Interactive execution - requires user interaction
    Interactive,
}

impl fmt::Display for ProtocolMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolMode::Synchronous => write!(f, "synchronous"),
            ProtocolMode::Asynchronous => write!(f, "asynchronous"),
            ProtocolMode::Interactive => write!(f, "interactive"),
        }
    }
}

// ============================================================================
// Threshold Cryptography Types
// ============================================================================

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
            // This is unreachable because FrostParticipantId guarantees non-zero
            unreachable!("FrostParticipantId guarantees non-zero value, conversion must succeed")
        })
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

/// Protocol session status enumeration for coordination
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtocolSessionStatus {
    /// Session is initializing
    Initializing,
    /// Session is actively running
    Active,
    /// Session completed successfully
    Completed,
    /// Session failed with error
    Failed(String),
    /// Session was terminated
    Terminated,
}
