// Core types for the CRDT effect_api

use aura_core::time::{PhysicalTime, TimeStamp};
use aura_core::Ed25519VerifyingKey;
use serde::{Deserialize, Serialize};

// Re-export shared types from crypto and aura-core
use aura_core::identifiers::DeviceId;
use aura_core::GuardianId;

// Import authentication types (ThresholdSig is imported where needed)

// Re-export consolidated types from aura-core
pub use aura_core::{ParticipantId, ProtocolType, SessionId, SessionOutcome, SessionStatus};

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

/// Session information
///
/// Represents an active or completed protocol session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique identifier for this session
    pub session_id: SessionId,
    /// Type of protocol being executed in this session
    pub protocol_type: ProtocolType,
    /// List of participants in this session
    pub participants: Vec<ParticipantId>,
    /// Time when session was started (using unified time system)
    pub started_at: TimeStamp,
    /// Time when session will expire (using unified time system)
    pub expires_at: TimeStamp,
    /// Current status of the session
    pub status: SessionStatus,
    /// Additional metadata stored with the session
    pub metadata: std::collections::BTreeMap<String, String>,
}

impl Session {
    /// Create a new session
    ///
    /// # Arguments
    /// * `session_id` - Unique identifier for the session
    /// * `protocol_type` - Type of protocol being executed
    /// * `participants` - List of participating device IDs
    /// * `started_at` - Time when session starts (using unified time system)
    /// * `ttl_ms` - Time-to-live in milliseconds
    pub fn new(
        session_id: SessionId,
        protocol_type: ProtocolType,
        participants: Vec<ParticipantId>,
        started_at: TimeStamp,
        ttl_ms: u64,
    ) -> Self {
        // Calculate expiration time based on the type of timestamp
        let expires_at = match &started_at {
            TimeStamp::PhysicalClock(physical) => TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: physical.ts_ms + ttl_ms,
                uncertainty: physical.uncertainty,
            }),
            // For non-physical timestamps, use the same timestamp type but with a deterministic offset
            _ => started_at.clone(), // For simplicity, will implement proper offset later if needed
        };

        Self {
            session_id,
            protocol_type,
            participants,
            started_at,
            expires_at,
            status: SessionStatus::Active,
            metadata: std::collections::BTreeMap::new(),
        }
    }

    /// Update the session status
    ///
    /// # Arguments
    /// * `status` - New status for the session
    pub fn update_status(&mut self, status: SessionStatus) {
        self.status = status;
    }

    /// Mark session as completed
    ///
    /// # Arguments
    /// * `_outcome` - Protocol outcome (unused)
    pub fn complete(&mut self, _outcome: SessionOutcome) {
        self.update_status(SessionStatus::Completed);
    }

    /// Abort the session due to failure
    ///
    /// # Arguments
    /// * `_reason` - Reason for abort (unused)
    /// * `_blamed_party` - Party responsible for failure (unused)
    pub fn abort(&mut self, _reason: &str, _blamed_party: Option<ParticipantId>) {
        self.update_status(SessionStatus::Failed);
    }

    /// Check if session is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            SessionStatus::Completed
                | SessionStatus::Failed
                | SessionStatus::Expired
                | SessionStatus::TimedOut
        )
    }

    /// Check if session has timed out
    ///
    /// # Arguments
    /// * `current_time` - Current time for comparison
    pub fn is_timed_out(&self, current_time: &TimeStamp) -> bool {
        use aura_core::time::{OrderingPolicy, TimeOrdering};
        matches!(
            current_time.compare(&self.expires_at, OrderingPolicy::DeterministicTieBreak),
            TimeOrdering::After
        )
    }

    /// Check if session has expired
    ///
    /// # Arguments
    /// * `current_time` - Current time for comparison
    pub fn is_expired(&self, current_time: &TimeStamp) -> bool {
        self.is_timed_out(current_time)
    }
}

// SessionStatus is now imported from aura-core

// SessionOutcome is now imported from aura-core
