//! TreeSession coordination and intent ranking
//!
//! Implements the Intent Pool pattern for coordinator-free tree mutations.
//! Devices independently compute the same instigator using deterministic ranking.

use aura_protocol::effects::choreographic::{ChoreographicRole, ChoreographyError};
use aura_types::{Commitment, DeviceId, Intent, IntentId, SessionId};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// Configuration for TreeSession coordination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeSessionConfig {
    /// Minimum number of participants required (threshold)
    pub threshold: usize,
    /// Total number of participants
    pub total_participants: usize,
    /// Timeout for each phase in seconds
    pub phase_timeout_seconds: u64,
}

/// TreeSession orchestrates a tree mutation from intent to committed TreeOp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeSession {
    /// Unique identifier for this session
    pub session_id: SessionId,
    /// The intent being executed
    pub intent: Intent,
    /// Devices participating in this session
    pub participants: Vec<ChoreographicRole>,
    /// Minimum signatures required
    pub threshold: usize,
}

/// Lifecycle phases for a TreeSession
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TreeSessionLifecycle {
    /// Ranking intents to select instigator
    Proposal,
    /// Validating snapshot commitments via Prepare/ACK
    Prepare,
    /// Exchanging threshold shares (off-CRDT)
    ShareExchange,
    /// Computing final tree operation
    Finalize,
    /// Creating threshold signature
    Attest,
    /// Writing to journal and tombstoning intent
    Commit,
    /// Session aborted due to conflict or failure
    Aborted,
    /// Session successfully completed
    Completed,
}

/// Errors that can occur during TreeSession execution
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum TreeSessionError {
    /// Snapshot commitment mismatch (CAS failure)
    #[error("Snapshot mismatch: expected {expected}, got {actual}")]
    SnapshotMismatch {
        /// Expected snapshot value
        expected: String,
        /// Actual snapshot value
        actual: String,
    },

    /// Insufficient participants for threshold
    #[error("Insufficient participants: need {threshold}, have {available}")]
    InsufficientParticipants {
        /// Required threshold
        threshold: usize,
        /// Available participants
        available: usize,
    },

    /// Invalid share data received
    #[error("Invalid share from {device_id}: {reason}")]
    InvalidShare {
        /// Device that sent invalid share
        device_id: DeviceId,
        /// Reason for invalidity
        reason: String,
    },

    /// Threshold signature verification failed
    #[error("Threshold signature verification failed: {0}")]
    SignatureVerificationFailed(String),

    /// Choreography execution error
    #[error("Choreography error: {0}")]
    ChoreographyError(String),

    /// Session timed out
    #[error("Session timed out in phase {phase:?}")]
    Timeout {
        /// The phase in which timeout occurred
        phase: TreeSessionLifecycle,
    },
}

impl From<ChoreographyError> for TreeSessionError {
    fn from(e: ChoreographyError) -> Self {
        TreeSessionError::ChoreographyError(e.to_string())
    }
}

/// Deterministic intent ranking for instigator selection
///
/// Ranking tuple: (snapshot_commitment, priority, intent_id)
/// - All replicas compute the same ranking deterministically
/// - Highest-ranked intent's author becomes instigator
/// - CAS-style: intent's snapshot must match current tree commitment
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentRank {
    /// Snapshot commitment from intent (for CAS check)
    pub snapshot_commitment: Commitment,
    /// Priority level (higher = more urgent)
    pub priority: u32,
    /// Intent identifier (tie-breaker)
    pub intent_id: IntentId,
}

impl IntentRank {
    /// Create ranking tuple from intent
    pub fn from_intent(intent: &Intent) -> Self {
        Self {
            snapshot_commitment: intent.snapshot_commitment,
            priority: intent.priority.0 as u32, // Extract u64 from Priority, convert to u32
            intent_id: intent.intent_id,
        }
    }
}

impl Ord for IntentRank {
    fn cmp(&self, other: &Self) -> Ordering {
        // First compare by snapshot commitment (lexicographic on bytes)
        match self
            .snapshot_commitment
            .as_bytes()
            .cmp(other.snapshot_commitment.as_bytes())
        {
            Ordering::Equal => {
                // Then by priority (higher priority wins)
                match other.priority.cmp(&self.priority) {
                    Ordering::Equal => {
                        // Finally by intent_id (deterministic tie-breaker)
                        self.intent_id.cmp(&other.intent_id)
                    }
                    ord => ord,
                }
            }
            ord => ord,
        }
    }
}

impl PartialOrd for IntentRank {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Rank intents and select the highest-ranked one
///
/// This function implements the deterministic ranking algorithm that all
/// replicas execute independently to arrive at the same instigator selection.
///
/// # Arguments
///
/// * `intents` - List of pending intents to rank
/// * `current_snapshot` - Current tree commitment for CAS validation
///
/// # Returns
///
/// The highest-ranked intent whose snapshot matches current_snapshot,
/// or None if no valid intents exist.
pub fn rank_intents(intents: &[Intent], current_snapshot: &Commitment) -> Option<Intent> {
    if intents.is_empty() {
        return None;
    }

    // Filter to intents with matching snapshot (valid for CAS)
    let mut valid_intents: Vec<_> = intents
        .iter()
        .filter(|intent| &intent.snapshot_commitment == current_snapshot)
        .cloned()
        .collect();

    if valid_intents.is_empty() {
        return None;
    }

    // Sort by rank (highest first)
    valid_intents.sort_by(|a, b| {
        let rank_a = IntentRank::from_intent(a);
        let rank_b = IntentRank::from_intent(b);
        rank_b.cmp(&rank_a) // Reverse for descending order
    });

    // Return highest-ranked intent
    valid_intents.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::{Epoch, Priority};

    fn mock_intent(priority: u8, intent_id: u64, snapshot: &str) -> Intent {
        Intent {
            intent_id: IntentId::new(),
            op: aura_types::TreeOp::EpochBump,
            path_span: vec![],
            snapshot_commitment: Commitment::from(snapshot.as_bytes().to_vec()),
            priority: Priority::from(priority),
            author: DeviceId::new(),
            submitted_at: 0,
        }
    }

    #[test]
    fn test_rank_intents_by_priority() {
        let snapshot = Commitment::from(vec![1, 2, 3]);

        let low = mock_intent(1, 100, &hex::encode(snapshot.as_bytes()));
        let high = mock_intent(10, 200, &hex::encode(snapshot.as_bytes()));

        let intents = vec![low.clone(), high.clone()];
        let selected = rank_intents(&intents, &snapshot).unwrap();

        assert_eq!(selected.priority, Priority::from(10));
    }

    #[test]
    fn test_rank_intents_filters_stale_snapshots() {
        let current = Commitment::from(vec![1, 2, 3]);
        let stale = Commitment::from(vec![0, 0, 0]);

        let valid = mock_intent(5, 100, &hex::encode(current.as_bytes()));
        let invalid = mock_intent(10, 200, &hex::encode(stale.as_bytes()));

        let intents = vec![valid.clone(), invalid];
        let selected = rank_intents(&intents, &current).unwrap();

        // Should select the valid intent even though invalid has higher priority
        assert_eq!(selected.intent_id, valid.intent_id);
    }

    #[test]
    fn test_rank_intents_empty() {
        let snapshot = Commitment::from(vec![1, 2, 3]);
        let intents = vec![];

        assert!(rank_intents(&intents, &snapshot).is_none());
    }

    #[test]
    fn test_rank_intents_all_stale() {
        let current = Commitment::from(vec![1, 2, 3]);
        let stale = Commitment::from(vec![0, 0, 0]);

        let intent1 = mock_intent(5, 100, &hex::encode(stale.as_bytes()));
        let intent2 = mock_intent(10, 200, &hex::encode(stale.as_bytes()));

        let intents = vec![intent1, intent2];

        assert!(rank_intents(&intents, &current).is_none());
    }
}
