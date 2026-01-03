//! Intent Pool Types
//!
//! Implements the staging area for proposed tree mutations using observed-remove set semantics.
//! Intents enable lock-free coordination where any online device can become the instigator
//! for executing a batch of compatible intents.

use aura_core::identifiers::DeviceId;
use aura_core::{Hash32 as Commitment, NodeIndex};
use serde::{Deserialize, Serialize};
use std::fmt;

// TreeOperation aliases aura_core::TreeOpKind until the intent system is fully
// migrated to the new tree mutation types.
use aura_core::TreeOpKind as TreeOperation;

/// Import unified time types from aura-core
use aura_core::time::TimeStamp;

/// Unique identifier for an intent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct IntentId(pub uuid::Uuid);

impl IntentId {
    /// Create a new intent ID.
    ///
    /// # Parameters
    /// - `id`: UUID for the intent (obtain from RandomEffects for testability)
    ///
    /// Note: Callers should obtain UUID from RandomEffects to maintain testability
    /// and consistency with the effect system architecture.
    pub fn new(id: uuid::Uuid) -> Self {
        Self(id)
    }

    /// Create from a UUID (alias for new)
    pub fn from_uuid(uuid: uuid::Uuid) -> Self {
        Self::new(uuid)
    }
}

impl fmt::Display for IntentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "intent-{}", self.0)
    }
}

/// Priority for intent execution
///
/// Higher priorities are executed first when ranking intents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Priority(pub u64);

impl Priority {
    /// Create a new priority
    pub fn new(priority: u64) -> Self {
        Self(priority)
    }

    /// Default priority
    pub fn default_priority() -> Self {
        Self(100)
    }

    /// High priority (for urgent operations)
    pub fn high() -> Self {
        Self(1000)
    }

    /// Low priority (for background operations)
    pub fn low() -> Self {
        Self(10)
    }

    /// Get the numeric value
    pub fn value(&self) -> u64 {
        self.0
    }
}

impl Default for Priority {
    fn default() -> Self {
        Self::default_priority()
    }
}

impl From<u64> for Priority {
    fn from(priority: u64) -> Self {
        Self(priority)
    }
}

/// Status of an intent in the pool
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentStatus {
    /// Intent is pending execution
    Pending,
    /// Intent is currently being executed (prepare/ACK phase)
    Executing,
    /// Intent completed successfully (retracted from pool)
    Completed,
    /// Intent failed and was rejected
    Failed,
    /// Intent was superseded by another intent
    Superseded,
}

impl fmt::Display for IntentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntentStatus::Pending => write!(f, "pending"),
            IntentStatus::Executing => write!(f, "executing"),
            IntentStatus::Completed => write!(f, "completed"),
            IntentStatus::Failed => write!(f, "failed"),
            IntentStatus::Superseded => write!(f, "superseded"),
        }
    }
}

/// Intent - a proposed tree mutation staged in the intent pool
///
/// Intents use observed-remove set (OR-Set) semantics for convergence.
/// The intent pool provides high availability: devices can enqueue intents
/// while offline, and convergence happens via gossip when they reconnect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    /// Unique identifier for this intent
    pub intent_id: IntentId,

    /// The proposed tree operation
    pub op: TreeOperation,

    /// Nodes that will be touched by this operation
    pub path_span: Vec<NodeIndex>,

    /// Snapshot commitment this intent is based on (for CAS check)
    pub snapshot_commitment: Commitment,

    /// Priority for deterministic ranking
    pub priority: Priority,

    /// Device that authored this intent
    pub author: DeviceId,

    /// Time when this intent was created (using unified time system)
    pub created_at: TimeStamp,

    /// Optional metadata
    pub metadata: std::collections::BTreeMap<String, String>,
}

impl Intent {
    /// Create a new intent.
    ///
    /// # Parameters
    /// - `intent_id`: Unique identifier for the intent (obtain from RandomEffects for testability)
    /// - `op`: Tree operation to perform
    /// - `path_span`: Path span for the operation
    /// - `snapshot_commitment`: Snapshot commitment
    /// - `priority`: Priority for execution
    /// - `author`: Device that authored this intent
    /// - `created_at`: Timestamp when this intent was created
    ///
    /// Note: Callers should obtain intent_id from RandomEffects to maintain testability
    /// and consistency with the effect system architecture.
    pub fn new(
        intent_id: IntentId,
        op: TreeOperation,
        path_span: Vec<NodeIndex>,
        snapshot_commitment: Commitment,
        priority: Priority,
        author: DeviceId,
        created_at: TimeStamp,
    ) -> Self {
        Self {
            intent_id,
            op,
            path_span,
            snapshot_commitment,
            priority,
            author,
            created_at,
            metadata: std::collections::BTreeMap::new(),
        }
    }

    /// Create an intent with metadata
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Check if this intent conflicts with another intent
    ///
    /// Two intents conflict if they touch overlapping nodes and have
    /// different snapshot commitments.
    pub fn conflicts_with(&self, other: &Intent) -> bool {
        // Same snapshot = compatible
        if self.snapshot_commitment == other.snapshot_commitment {
            return false;
        }

        // Check for path overlap
        for node in &self.path_span {
            if other.path_span.contains(node) {
                return true;
            }
        }

        false
    }

    /// Calculate ranking key for deterministic instigator selection
    ///
    /// Returns (snapshot_commitment, priority, intent_id) for comparison.
    /// Intents with higher priority and earlier IDs rank higher.
    pub fn rank_key(&self) -> (Commitment, Priority, IntentId) {
        (self.snapshot_commitment, self.priority, self.intent_id)
    }

    /// Check if this intent is stale (snapshot too old)
    pub fn is_stale(&self, current_commitment: &Commitment) -> bool {
        &self.snapshot_commitment != current_commitment
    }

    /// Get the age of this intent in milliseconds
    pub fn age(&self, current_time: &TimeStamp) -> Option<u64> {
        // For age calculation, we need proper duration computation
        // This is a simplified version - proper implementation would use
        // domain-specific duration calculation from aura-core
        use aura_core::time::{OrderingPolicy, TimeOrdering};
        match current_time.compare(&self.created_at, OrderingPolicy::DeterministicTieBreak) {
            TimeOrdering::After => {
                if let (TimeStamp::PhysicalClock(now), TimeStamp::PhysicalClock(created)) =
                    (current_time, &self.created_at)
                {
                    Some(created.ts_ms.saturating_sub(now.ts_ms))
                } else {
                    None
                }
            }
            _ => Some(0),
        }
    }
}

/// Intent batch - a collection of compatible intents
///
/// Used during the prepare/ACK phase to execute multiple intents atomically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentBatch {
    /// Intents in this batch
    pub intents: Vec<Intent>,

    /// Snapshot commitment for this batch
    pub snapshot_commitment: Commitment,

    /// Combined path span
    pub combined_path_span: Vec<NodeIndex>,
}

impl IntentBatch {
    /// Create a new intent batch
    pub fn new(snapshot_commitment: Commitment) -> Self {
        Self {
            intents: Vec::new(),
            snapshot_commitment,
            combined_path_span: Vec::new(),
        }
    }

    /// Try to add an intent to this batch
    ///
    /// Returns Ok if the intent is compatible, Err otherwise.
    pub fn try_add(&mut self, intent: Intent) -> Result<(), String> {
        // Check snapshot compatibility
        if intent.snapshot_commitment != self.snapshot_commitment {
            return Err("Snapshot mismatch".to_string());
        }

        // Check for conflicts with existing intents
        for existing in &self.intents {
            if intent.conflicts_with(existing) {
                return Err("Conflicts with existing intent".to_string());
            }
        }

        // Add to combined path span
        for node in &intent.path_span {
            if !self.combined_path_span.contains(node) {
                self.combined_path_span.push(*node);
            }
        }

        self.intents.push(intent);
        Ok(())
    }

    /// Check if this batch is empty
    pub fn is_empty(&self) -> bool {
        self.intents.is_empty()
    }

    /// Get the number of intents in this batch
    pub fn len(&self) -> usize {
        self.intents.len()
    }

    /// Get all intent IDs in this batch
    pub fn intent_ids(&self) -> Vec<IntentId> {
        self.intents.iter().map(|i| i.intent_id).collect()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use aura_core::Hash32;
    // Note: Tests commented out - need migration to new TreeOpKind from aura_core
    // Legacy TreeOp types have been replaced by fact-based AttestedOp

    // Helper function to create deterministic test UUIDs
    fn test_uuid(seed: u8) -> uuid::Uuid {
        uuid::Uuid::from_bytes([seed; 16])
    }

    #[test]
    fn test_intent_id_creation() {
        let id1 = IntentId::new(test_uuid(1));
        let id2 = IntentId::new(test_uuid(2));
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_priority_values() {
        assert!(Priority::high() > Priority::default_priority());
        assert!(Priority::default_priority() > Priority::low());
    }

    #[test]
    fn test_intent_creation() {
        use crate::LeafNode;

        let op = TreeOperation::AddLeaf {
            leaf: LeafNode::new_device(
                crate::LeafId(0),
                DeviceId(uuid::Uuid::from_bytes([9u8; 16])),
                vec![0u8; 32],
            )
            .expect("valid leaf"),
            under: NodeIndex(0),
        };

        let intent = Intent::new(
            IntentId::new(test_uuid(6)),
            op,
            vec![NodeIndex(0)],
            Hash32([0u8; 32]),
            Priority::default_priority(),
            DeviceId(uuid::Uuid::from_bytes([1u8; 16])),
            TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        );

        assert_eq!(intent.priority, Priority::default_priority());
        assert!(!intent.is_stale(&Hash32([0u8; 32])));
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_intent_conflicts() {
        let op = TreeOperation::RotateEpoch {
            affected: vec![NodeIndex(0)],
        };

        let intent1 = Intent::new(
            IntentId::new(test_uuid(4)),
            op.clone(),
            vec![NodeIndex(0), NodeIndex(1)],
            Hash32([1u8; 32]),
            Priority::default_priority(),
            DeviceId(uuid::Uuid::from_bytes([1u8; 16])),
            TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        );

        let intent2 = Intent::new(
            IntentId::new(test_uuid(5)),
            op,
            vec![NodeIndex(1), NodeIndex(2)],
            Hash32([2u8; 32]), // Different snapshot
            Priority::default_priority(),
            DeviceId(uuid::Uuid::from_bytes([1u8; 16])),
            TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        );

        // Should conflict due to overlapping nodes (1) and different snapshots
        assert!(intent1.conflicts_with(&intent2));
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_intent_no_conflict_same_snapshot() {
        let op = TreeOperation::RotateEpoch {
            affected: vec![NodeIndex(0)],
        };

        let snapshot = [1u8; 32];

        let intent1 = Intent::new(
            IntentId::new(test_uuid(7)),
            op.clone(),
            vec![NodeIndex(0), NodeIndex(1)],
            Hash32(snapshot),
            Priority::default_priority(),
            DeviceId(uuid::Uuid::from_bytes([1u8; 16])),
            TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        );

        let intent2 = Intent::new(
            IntentId::new(test_uuid(7)),
            op,
            vec![NodeIndex(1), NodeIndex(2)],
            Hash32(snapshot), // Same snapshot
            Priority::default_priority(),
            DeviceId(uuid::Uuid::from_bytes([1u8; 16])),
            TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        );

        // Should not conflict - same snapshot
        assert!(!intent1.conflicts_with(&intent2));
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_intent_is_stale() {
        let intent = Intent::new(
            IntentId::new(test_uuid(7)),
            TreeOperation::RotateEpoch {
                affected: vec![NodeIndex(0)],
            },
            vec![],
            Hash32([1u8; 32]),
            Priority::default_priority(),
            DeviceId(uuid::Uuid::from_bytes([1u8; 16])),
            TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        );

        assert!(!intent.is_stale(&Hash32([1u8; 32])));
        assert!(intent.is_stale(&Hash32([2u8; 32])));
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_intent_age() {
        use aura_core::time::{PhysicalTime, TimeStamp};

        let created_at = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        });

        let intent = Intent::new(
            IntentId::new(test_uuid(7)),
            TreeOperation::RemoveLeaf {
                leaf: aura_core::tree::LeafId(0),
                reason: 0,
            },
            vec![],
            Hash32([0u8; 32]),
            Priority::default_priority(),
            DeviceId(uuid::Uuid::from_bytes([1u8; 16])),
            created_at,
        );

        let time_1500 = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1500,
            uncertainty: None,
        });
        let time_1000 = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        });

        assert!(intent.age(&time_1500).is_some());
        assert_eq!(intent.age(&time_1000), Some(0));
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_intent_batch_add() {
        use crate::Policy;

        let snapshot = [1u8; 32];
        let mut batch = IntentBatch::new(Hash32(snapshot));

        let intent1 = Intent::new(
            IntentId::new(test_uuid(7)),
            TreeOperation::ChangePolicy {
                node: NodeIndex(0),
                new_policy: Policy::All,
            },
            vec![NodeIndex(0)],
            Hash32(snapshot),
            Priority::default_priority(),
            DeviceId(uuid::Uuid::from_bytes([1u8; 16])),
            TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        );

        let result = batch.try_add(intent1);
        assert!(result.is_ok());
        assert_eq!(batch.len(), 1);
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_intent_batch_rejects_snapshot_mismatch() {
        let snapshot1 = [1u8; 32];
        let snapshot2 = [2u8; 32];
        let mut batch = IntentBatch::new(Hash32(snapshot1));

        let intent = Intent::new(
            IntentId::new(test_uuid(7)),
            TreeOperation::RotateEpoch {
                affected: vec![NodeIndex(0)],
            },
            vec![NodeIndex(0)],
            Hash32(snapshot2),
            Priority::default_priority(),
            DeviceId(uuid::Uuid::from_bytes([1u8; 16])),
            TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        );

        let result = batch.try_add(intent);
        assert!(result.is_err());
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_intent_batch_intent_ids() {
        let snapshot = [1u8; 32];
        let mut batch = IntentBatch::new(Hash32(snapshot));

        let intent1 = Intent::new(
            IntentId::new(test_uuid(7)),
            TreeOperation::RotateEpoch {
                affected: vec![NodeIndex(0)],
            },
            vec![NodeIndex(0)],
            Hash32(snapshot),
            Priority::default_priority(),
            DeviceId(uuid::Uuid::from_bytes([1u8; 16])),
            TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        );

        let id = intent1.intent_id;
        batch.try_add(intent1).unwrap();

        let ids = batch.intent_ids();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], id);
    }

    #[test]
    fn test_intent_status_display() {
        assert_eq!(IntentStatus::Pending.to_string(), "pending");
        assert_eq!(IntentStatus::Executing.to_string(), "executing");
        assert_eq!(IntentStatus::Completed.to_string(), "completed");
    }
}
