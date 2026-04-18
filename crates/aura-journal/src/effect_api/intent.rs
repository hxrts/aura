//! Intent Pool Types
//!
//! Implements the staging area for proposed tree mutations using observed-remove set semantics.
//! Intents enable lock-free coordination where any online device can become the instigator
//! for executing a batch of compatible intents.

use super::journal_types::uuid_newtype;
use aura_core::types::identifiers::DeviceId;
use aura_core::{Hash32 as Commitment, NodeIndex};
use serde::{Deserialize, Serialize};

// TreeOperation aliases aura_core::TreeOpKind until the intent system is fully
// migrated to the new tree mutation types.
use aura_core::TreeOpKind as TreeOperation;

/// Import unified time types from aura-core
use aura_core::time::TimeStamp;

uuid_newtype!(IntentId, "intent-", "Unique identifier for an intent");

/// Priority for intent execution
///
/// Higher priorities are executed first when ranking intents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Priority(pub u64);

const DEFAULT_PRIORITY_VALUE: u64 = 100;
const HIGH_PRIORITY_VALUE: u64 = 1000;
const LOW_PRIORITY_VALUE: u64 = 10;

impl Priority {
    /// Create a new priority
    pub fn new(priority: u64) -> Self {
        Self(priority)
    }

    /// Default priority
    pub fn default_priority() -> Self {
        Self(DEFAULT_PRIORITY_VALUE)
    }

    /// High priority (for urgent operations)
    pub fn high() -> Self {
        Self(HIGH_PRIORITY_VALUE)
    }

    /// Low priority (for background operations)
    pub fn low() -> Self {
        Self(LOW_PRIORITY_VALUE)
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

impl std::fmt::Display for IntentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
