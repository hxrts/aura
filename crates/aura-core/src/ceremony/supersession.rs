//! Ceremony Supersession Types
//!
//! When a new ceremony logically replaces an older one, supersession facts
//! are emitted to signal participants to stop working on the old ceremony.
//! These facts propagate via existing anti-entropy - no special protocol needed.
//!
//! # Design Rationale
//!
//! Supersession is explicit rather than implicit because:
//! 1. Prestate binding alone causes old ceremonies to silently fail on commit
//! 2. Explicit supersession provides audit trail for debugging
//! 3. Participants can immediately stop wasted work on superseded ceremonies
//! 4. UI can show clear "superseded by newer request" messaging
//!
//! # Supersession vs Abort
//!
//! - **Abort**: Ceremony failed/cancelled, no replacement exists
//! - **Supersession**: Ceremony replaced by a newer ceremony (includes link)

use crate::domain::content::Hash32;
use serde::{Deserialize, Serialize};

/// Reason why a ceremony was superseded by another.
///
/// This enum captures the semantic reason for supersession, enabling
/// appropriate UI messaging and audit logging.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SupersessionReason {
    /// The ceremony's prestate no longer matches current state.
    ///
    /// This happens when another ceremony (or other state change) committed
    /// while this ceremony was in progress, invalidating its prestate binding.
    PrestateStale,

    /// The same initiator explicitly started a newer ceremony.
    ///
    /// Common when a user cancels and restarts, or when retry logic
    /// creates a fresh ceremony after detecting issues.
    NewerRequest,

    /// Manual cancellation that triggers supersession.
    ///
    /// The user or system explicitly cancelled this ceremony in favor
    /// of a new one (as opposed to a simple abort with no replacement).
    ExplicitCancel,

    /// Timeout-triggered supersession.
    ///
    /// The ceremony timed out and a new attempt was automatically started.
    /// The superseding ceremony is the retry attempt.
    Timeout,

    /// A concurrent ceremony with higher precedence won.
    ///
    /// When multiple ceremonies compete for the same state transition,
    /// one wins based on precedence rules (typically first-to-commit).
    Precedence {
        /// The ceremony ID that won the race
        winner: String,
    },
}

impl SupersessionReason {
    /// Returns a human-readable description of the reason.
    pub fn description(&self) -> &'static str {
        match self {
            SupersessionReason::PrestateStale => "prestate no longer matches current state",
            SupersessionReason::NewerRequest => "replaced by newer request from same initiator",
            SupersessionReason::ExplicitCancel => "explicitly cancelled in favor of new ceremony",
            SupersessionReason::Timeout => "timed out, retry started",
            SupersessionReason::Precedence { .. } => "superseded by concurrent ceremony",
        }
    }

    /// Returns a short code for the reason (for fact keys, logging).
    pub fn code(&self) -> &'static str {
        match self {
            SupersessionReason::PrestateStale => "prestate_stale",
            SupersessionReason::NewerRequest => "newer_request",
            SupersessionReason::ExplicitCancel => "explicit_cancel",
            SupersessionReason::Timeout => "timeout",
            SupersessionReason::Precedence { .. } => "precedence",
        }
    }
}

impl std::fmt::Display for SupersessionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

/// Record of a ceremony supersession for audit trail.
///
/// Supersession records are stored in the journal to maintain a complete
/// history of ceremony lifecycle, enabling debugging and forensics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupersessionRecord {
    /// The ceremony that was superseded (the old ceremony)
    pub superseded_id: Hash32,

    /// The ceremony that supersedes it (the new ceremony)
    pub superseding_id: Hash32,

    /// Why the supersession occurred
    pub reason: SupersessionReason,

    /// When the supersession was recorded (milliseconds since epoch)
    pub timestamp_ms: u64,
}

impl SupersessionRecord {
    /// Create a new supersession record.
    pub fn new(
        superseded_id: Hash32,
        superseding_id: Hash32,
        reason: SupersessionReason,
        timestamp_ms: u64,
    ) -> Self {
        Self {
            superseded_id,
            superseding_id,
            reason,
            timestamp_ms,
        }
    }

    /// Generate a fact key for this supersession record.
    ///
    /// Format: `supersession:{superseded_id_hex}:{superseding_id_hex}`
    pub fn fact_key(&self) -> String {
        format!(
            "supersession:{}:{}",
            hex::encode(&self.superseded_id.0[..8]),
            hex::encode(&self.superseding_id.0[..8])
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supersession_reason_display() {
        assert_eq!(
            SupersessionReason::PrestateStale.to_string(),
            "prestate no longer matches current state"
        );
        assert_eq!(
            SupersessionReason::NewerRequest.to_string(),
            "replaced by newer request from same initiator"
        );
    }

    #[test]
    fn test_supersession_reason_code() {
        assert_eq!(SupersessionReason::PrestateStale.code(), "prestate_stale");
        assert_eq!(SupersessionReason::Timeout.code(), "timeout");
        assert_eq!(
            SupersessionReason::Precedence {
                winner: "abc".into()
            }
            .code(),
            "precedence"
        );
    }

    #[test]
    fn test_supersession_record_fact_key() {
        let record = SupersessionRecord::new(
            Hash32([1u8; 32]),
            Hash32([2u8; 32]),
            SupersessionReason::NewerRequest,
            1234567890,
        );

        let key = record.fact_key();
        assert!(key.starts_with("supersession:"));
        assert!(key.contains(":"));
    }

    #[test]
    fn test_supersession_record_serialization() {
        let record = SupersessionRecord::new(
            Hash32([1u8; 32]),
            Hash32([2u8; 32]),
            SupersessionReason::Precedence {
                winner: "winner_ceremony_id".into(),
            },
            1234567890,
        );

        let serialized = serde_json::to_string(&record).unwrap();
        let deserialized: SupersessionRecord = serde_json::from_str(&serialized).unwrap();

        assert_eq!(record, deserialized);
    }
}
