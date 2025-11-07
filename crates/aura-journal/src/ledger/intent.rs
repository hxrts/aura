//! Intent Pool Types
//!
//! Implements the staging area for proposed tree mutations using observed-remove set semantics.
//! Intents enable lock-free coordination where any online device can become the instigator
//! for executing a batch of compatible intents.

use crate::tree::{Commitment, NodeIndex, TreeOperation};
use aura_types::identifiers::DeviceId;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for an intent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct IntentId(pub uuid::Uuid);

impl IntentId {
    /// Create a new random intent ID
    #[allow(clippy::disallowed_methods)]
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    /// Create from a UUID
    pub fn from_uuid(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }
}

impl Default for IntentId {
    fn default() -> Self {
        Self::new()
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
    /// Intent completed successfully (tombstoned)
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

    /// Timestamp when this intent was created
    pub created_at: super::tree_op::Timestamp,

    /// Optional metadata
    pub metadata: std::collections::BTreeMap<String, String>,
}

impl Intent {
    /// Create a new intent
    pub fn new(
        op: TreeOperation,
        path_span: Vec<NodeIndex>,
        snapshot_commitment: Commitment,
        priority: Priority,
        author: DeviceId,
        created_at: super::tree_op::Timestamp,
    ) -> Self {
        Self {
            intent_id: IntentId::new(),
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
    pub fn age(&self, current_time: super::tree_op::Timestamp) -> super::tree_op::Timestamp {
        current_time.saturating_sub(self.created_at)
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
mod tests {
    use super::*;
    use crate::tree::{AffectedPath, LeafIndex};

    #[test]
    fn test_intent_id_creation() {
        let id1 = IntentId::new();
        let id2 = IntentId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_priority_values() {
        assert!(Priority::high() > Priority::default_priority());
        assert!(Priority::default_priority() > Priority::low());
    }

    #[test]
    fn test_intent_creation() {
        let op = TreeOperation::RotatePath {
            leaf_index: LeafIndex(0),
            affected_path: AffectedPath::new(),
        };

        let intent = Intent::new(
            op,
            vec![NodeIndex::new(0)],
            Commitment::default(),
            Priority::default(),
            DeviceId::new(),
            1000,
        );

        assert_eq!(intent.priority, Priority::default());
        assert!(!intent.is_stale(&Commitment::default()));
    }

    #[test]
    fn test_intent_conflicts() {
        let op = TreeOperation::RotatePath {
            leaf_index: LeafIndex(0),
            affected_path: AffectedPath::new(),
        };

        let intent1 = Intent::new(
            op.clone(),
            vec![NodeIndex::new(0), NodeIndex::new(1)],
            Commitment::new([1u8; 32]),
            Priority::default(),
            DeviceId::new(),
            1000,
        );

        let intent2 = Intent::new(
            op,
            vec![NodeIndex::new(1), NodeIndex::new(2)],
            Commitment::new([2u8; 32]), // Different snapshot
            Priority::default(),
            DeviceId::new(),
            1000,
        );

        // Should conflict due to overlapping nodes (1) and different snapshots
        assert!(intent1.conflicts_with(&intent2));
    }

    #[test]
    fn test_intent_no_conflict_same_snapshot() {
        let op = TreeOperation::RotatePath {
            leaf_index: LeafIndex(0),
            affected_path: AffectedPath::new(),
        };

        let snapshot = Commitment::new([1u8; 32]);

        let intent1 = Intent::new(
            op.clone(),
            vec![NodeIndex::new(0), NodeIndex::new(1)],
            snapshot,
            Priority::default(),
            DeviceId::new(),
            1000,
        );

        let intent2 = Intent::new(
            op,
            vec![NodeIndex::new(1), NodeIndex::new(2)],
            snapshot, // Same snapshot
            Priority::default(),
            DeviceId::new(),
            1000,
        );

        // Should not conflict - same snapshot
        assert!(!intent1.conflicts_with(&intent2));
    }

    #[test]
    fn test_intent_is_stale() {
        let intent = Intent::new(
            TreeOperation::RotatePath {
                leaf_index: LeafIndex(0),
                affected_path: AffectedPath::new(),
            },
            vec![],
            Commitment::new([1u8; 32]),
            Priority::default(),
            DeviceId::new(),
            1000,
        );

        assert!(!intent.is_stale(&Commitment::new([1u8; 32])));
        assert!(intent.is_stale(&Commitment::new([2u8; 32])));
    }

    #[test]
    fn test_intent_age() {
        let intent = Intent::new(
            TreeOperation::RotatePath {
                leaf_index: LeafIndex(0),
                affected_path: AffectedPath::new(),
            },
            vec![],
            Commitment::default(),
            Priority::default(),
            DeviceId::new(),
            1000,
        );

        assert_eq!(intent.age(1500), 500);
        assert_eq!(intent.age(1000), 0);
    }

    #[test]
    fn test_intent_batch_add() {
        let snapshot = Commitment::new([1u8; 32]);
        let mut batch = IntentBatch::new(snapshot);

        let intent1 = Intent::new(
            TreeOperation::RotatePath {
                leaf_index: LeafIndex(0),
                affected_path: AffectedPath::new(),
            },
            vec![NodeIndex::new(0)],
            snapshot,
            Priority::default(),
            DeviceId::new(),
            1000,
        );

        let result = batch.try_add(intent1);
        assert!(result.is_ok());
        assert_eq!(batch.len(), 1);
    }

    #[test]
    fn test_intent_batch_rejects_snapshot_mismatch() {
        let snapshot1 = Commitment::new([1u8; 32]);
        let snapshot2 = Commitment::new([2u8; 32]);
        let mut batch = IntentBatch::new(snapshot1);

        let intent = Intent::new(
            TreeOperation::RotatePath {
                leaf_index: LeafIndex(0),
                affected_path: AffectedPath::new(),
            },
            vec![NodeIndex::new(0)],
            snapshot2,
            Priority::default(),
            DeviceId::new(),
            1000,
        );

        let result = batch.try_add(intent);
        assert!(result.is_err());
    }

    #[test]
    fn test_intent_batch_intent_ids() {
        let snapshot = Commitment::new([1u8; 32]);
        let mut batch = IntentBatch::new(snapshot);

        let intent1 = Intent::new(
            TreeOperation::RotatePath {
                leaf_index: LeafIndex(0),
                affected_path: AffectedPath::new(),
            },
            vec![NodeIndex::new(0)],
            snapshot,
            Priority::default(),
            DeviceId::new(),
            1000,
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
