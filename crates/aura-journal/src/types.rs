//! Guardian metadata and policy types for recovery workflows

use aura_core::time::TimeStamp;
use aura_core::Ed25519VerifyingKey;
use serde::{Deserialize, Serialize};

// Re-export shared types from crypto and aura-core
use aura_core::identifiers::DeviceId;
use aura_core::GuardianId;

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
