// Core types for the CRDT effect_api

use aura_core::time::TimeStamp;
use aura_core::Ed25519VerifyingKey;
use serde::{Deserialize, Serialize};

// Re-export shared types from crypto and aura-core
use aura_core::identifiers::DeviceId;
use aura_core::GuardianId;

// Import authentication types (ThresholdSig is imported where needed)

// Re-export consolidated types from aura-core
// ProtocolType has moved to aura-protocol (Layer 4)
// SessionStatus and SessionOutcome are defined in aura-core
// Use ContentId from aura-core

// Display for AccountId is implemented in aura-core crate

/// Guardian metadata
///
/// Tracks information about a guardian who can help recover account access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardianMetadata {
    /// Unique identifier for this guardian
    pub guardian_id: GuardianId,
    /// Device ID of the guardian's device
    pub device_id: DeviceId,
    /// Email address for guardian contact
    pub email: String,
    /// Ed25519 public key for signature verification
    pub public_key: Ed25519VerifyingKey,
    /// Time when this guardian was added (using unified time system)
    pub added_at: TimeStamp,
    /// Policy controlling guardian recovery actions
    pub policy: GuardianPolicy,
}

/// Guardian policy configuration
///
/// Controls how a guardian can participate in account recovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardianPolicy {
    /// Whether this guardian's recovery actions require explicit approval
    pub requires_approval: bool,
    /// Cooldown period in seconds between recovery actions by this guardian
    pub cooldown_period: u64,
    /// Maximum number of recovery operations allowed per calendar day
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

// ParticipantId is now imported from aura-core

// SessionId is now imported from aura-core

// ProtocolType is now imported from aura-core

// EventNonce is now imported from aura-core
//
// Note: Session type was removed - session tracking now happens in aura-protocol (Layer 4)

// SessionStatus is now imported from aura-core

// SessionOutcome is now imported from aura-core
