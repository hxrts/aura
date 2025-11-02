//! Snapshot utilities and implementations

use super::UnifiedSnapshot;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Builder for creating snapshots with fluent API
#[derive(Debug, Clone)]
pub struct SnapshotBuilder {
    state_type: String,
    tick: u64,
    metadata: HashMap<String, String>,
}

impl SnapshotBuilder {
    /// Create a new snapshot builder
    pub fn new(state_type: String, tick: u64) -> Self {
        Self {
            state_type,
            tick,
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the snapshot
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Add multiple metadata entries
    pub fn with_metadata_map(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata.extend(metadata);
        self
    }

    /// Build the snapshot from the provided data
    pub fn build<T: Serialize>(self, data: &T) -> Result<UnifiedSnapshot, serde_json::Error> {
        UnifiedSnapshot::new(self.state_type, self.tick, data, self.metadata)
    }
}

/// Trait for types that can be converted to snapshots
pub trait Snapshotable {
    type Error;

    /// Create a snapshot of the current state
    fn to_snapshot(&self, tick: u64) -> Result<UnifiedSnapshot, Self::Error>;

    /// Restore from a snapshot
    fn from_snapshot(snapshot: &UnifiedSnapshot) -> Result<Self, Self::Error>
    where
        Self: Sized;
}

/// Helper for creating snapshots from common simulation types
pub struct SnapshotHelper;

impl SnapshotHelper {
    /// Create a world state snapshot
    pub fn world_state_snapshot<T: Serialize>(
        tick: u64,
        participant_count: usize,
        active_sessions: usize,
        data: &T,
    ) -> Result<UnifiedSnapshot, serde_json::Error> {
        SnapshotBuilder::new("WorldState".to_string(), tick)
            .with_metadata(
                "participant_count".to_string(),
                participant_count.to_string(),
            )
            .with_metadata("active_sessions".to_string(), active_sessions.to_string())
            .with_metadata("snapshot_type".to_string(), "world_state".to_string())
            .build(data)
    }

    /// Create a protocol state snapshot
    pub fn protocol_state_snapshot<T: Serialize>(
        tick: u64,
        protocol_type: String,
        session_count: usize,
        data: &T,
    ) -> Result<UnifiedSnapshot, serde_json::Error> {
        SnapshotBuilder::new("ProtocolState".to_string(), tick)
            .with_metadata("protocol_type".to_string(), protocol_type)
            .with_metadata("session_count".to_string(), session_count.to_string())
            .with_metadata("snapshot_type".to_string(), "protocol_state".to_string())
            .build(data)
    }

    /// Create a network state snapshot
    pub fn network_state_snapshot<T: Serialize>(
        tick: u64,
        partition_count: usize,
        message_count: usize,
        data: &T,
    ) -> Result<UnifiedSnapshot, serde_json::Error> {
        SnapshotBuilder::new("NetworkState".to_string(), tick)
            .with_metadata("partition_count".to_string(), partition_count.to_string())
            .with_metadata("message_count".to_string(), message_count.to_string())
            .with_metadata("snapshot_type".to_string(), "network_state".to_string())
            .build(data)
    }

    /// Create a participant state snapshot
    pub fn participant_state_snapshot<T: Serialize>(
        tick: u64,
        participant_id: String,
        data: &T,
    ) -> Result<UnifiedSnapshot, serde_json::Error> {
        SnapshotBuilder::new("ParticipantState".to_string(), tick)
            .with_metadata("participant_id".to_string(), participant_id)
            .with_metadata("snapshot_type".to_string(), "participant_state".to_string())
            .build(data)
    }
}

/// Snapshot comparison utilities
pub struct SnapshotComparator;

impl SnapshotComparator {
    /// Compare two snapshots and return differences
    pub fn compare(snapshot1: &UnifiedSnapshot, snapshot2: &UnifiedSnapshot) -> SnapshotDiff {
        let mut changes = Vec::new();

        // Check if different types
        if snapshot1.state_type != snapshot2.state_type {
            changes.push(DiffEntry {
                field: "state_type".to_string(),
                old_value: Some(snapshot1.state_type.clone()),
                new_value: Some(snapshot2.state_type.clone()),
                diff_type: DiffType::Modified,
            });
        }

        // Check tick difference
        if snapshot1.tick != snapshot2.tick {
            changes.push(DiffEntry {
                field: "tick".to_string(),
                old_value: Some(snapshot1.tick.to_string()),
                new_value: Some(snapshot2.tick.to_string()),
                diff_type: DiffType::Modified,
            });
        }

        // Check metadata differences
        for (key, value1) in &snapshot1.metadata {
            match snapshot2.metadata.get(key) {
                Some(value2) if value1 != value2 => {
                    changes.push(DiffEntry {
                        field: format!("metadata.{}", key),
                        old_value: Some(value1.clone()),
                        new_value: Some(value2.clone()),
                        diff_type: DiffType::Modified,
                    });
                }
                None => {
                    changes.push(DiffEntry {
                        field: format!("metadata.{}", key),
                        old_value: Some(value1.clone()),
                        new_value: None,
                        diff_type: DiffType::Removed,
                    });
                }
                _ => {} // No change
            }
        }

        // Check for new metadata fields
        for (key, value2) in &snapshot2.metadata {
            if !snapshot1.metadata.contains_key(key) {
                changes.push(DiffEntry {
                    field: format!("metadata.{}", key),
                    old_value: None,
                    new_value: Some(value2.clone()),
                    diff_type: DiffType::Added,
                });
            }
        }

        let has_significant_changes = !changes.is_empty();
        SnapshotDiff {
            snapshot1_id: snapshot1.id.clone(),
            snapshot2_id: snapshot2.id.clone(),
            changes,
            has_significant_changes,
        }
    }

    /// Check if snapshots are equivalent
    pub fn are_equivalent(snapshot1: &UnifiedSnapshot, snapshot2: &UnifiedSnapshot) -> bool {
        snapshot1.content_hash == snapshot2.content_hash
    }
}

/// Difference between two snapshots
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDiff {
    /// ID of the first snapshot
    pub snapshot1_id: String,
    /// ID of the second snapshot
    pub snapshot2_id: String,
    /// List of changes between snapshots
    pub changes: Vec<DiffEntry>,
    /// Whether there are significant changes
    pub has_significant_changes: bool,
}

/// Individual difference entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffEntry {
    /// Field that changed
    pub field: String,
    /// Previous value (if any)
    pub old_value: Option<String>,
    /// New value (if any)
    pub new_value: Option<String>,
    /// Type of change
    pub diff_type: DiffType,
}

/// Type of difference
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DiffType {
    /// Field was added
    Added,
    /// Field was removed
    Removed,
    /// Field was modified
    Modified,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateSnapshot;

    #[test]
    fn test_snapshot_builder() {
        let data = vec![1, 2, 3];
        let snapshot = SnapshotBuilder::new("TestData".to_string(), 42)
            .with_metadata("test_key".to_string(), "test_value".to_string())
            .build(&data)
            .unwrap();

        assert_eq!(snapshot.state_type(), "TestData");
        assert_eq!(snapshot.tick(), 42);
        assert_eq!(snapshot.metadata().get("test_key").unwrap(), "test_value");
    }

    #[test]
    fn test_snapshot_comparison() {
        let data1 = vec![1, 2, 3];
        let data2 = vec![1, 2, 4];

        let snapshot1 = SnapshotBuilder::new("TestData".to_string(), 1)
            .with_metadata("test".to_string(), "value1".to_string())
            .build(&data1)
            .unwrap();

        let snapshot2 = SnapshotBuilder::new("TestData".to_string(), 2)
            .with_metadata("test".to_string(), "value2".to_string())
            .build(&data2)
            .unwrap();

        let diff = SnapshotComparator::compare(&snapshot1, &snapshot2);
        assert!(diff.has_significant_changes);
        assert!(!diff.changes.is_empty());
    }
}
