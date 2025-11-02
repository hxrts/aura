//! State difference calculation and analysis

use super::{StateError, UnifiedSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// State difference calculator
pub struct StateDiffCalculator;

impl StateDiffCalculator {
    /// Calculate comprehensive difference between two snapshots
    pub fn calculate_diff(
        from: &UnifiedSnapshot,
        to: &UnifiedSnapshot,
    ) -> Result<ComprehensiveStateDiff, StateError> {
        // Basic validation
        if from.state_type != to.state_type {
            return Err(StateError::InvalidState {
                reason: format!(
                    "Cannot diff different state types: {} vs {}",
                    from.state_type, to.state_type
                ),
            });
        }

        let mut changes = Vec::new();

        // Compare basic properties
        if from.tick != to.tick {
            changes.push(StateDiffEntry {
                path: "tick".to_string(),
                change_type: DiffChangeType::Modified,
                old_value: Some(serde_json::Value::Number(from.tick.into())),
                new_value: Some(serde_json::Value::Number(to.tick.into())),
            });
        }

        // Compare metadata
        let metadata_diff = Self::diff_metadata(&from.metadata, &to.metadata);
        changes.extend(metadata_diff);

        // Compare data content (JSON-based diff)
        let data_diff = Self::diff_json_values(&from.data, &to.data, "data".to_string())?;
        changes.extend(data_diff);

        let summary = Self::generate_diff_summary(&changes);
        let diff = ComprehensiveStateDiff {
            from_snapshot_id: from.id.clone(),
            to_snapshot_id: to.id.clone(),
            state_type: from.state_type.clone(),
            tick_range: (from.tick, to.tick),
            changes,
            summary,
        };

        Ok(diff)
    }

    /// Calculate incremental diff that can be applied
    pub fn calculate_incremental_diff(
        from: &UnifiedSnapshot,
        to: &UnifiedSnapshot,
    ) -> Result<IncrementalDiff, StateError> {
        let comprehensive_diff = Self::calculate_diff(from, to)?;

        // Convert to incremental operations
        let operations = comprehensive_diff
            .changes
            .into_iter()
            .map(DiffOperation::from_diff_entry)
            .collect();

        Ok(IncrementalDiff {
            base_snapshot_id: from.id.clone(),
            target_snapshot_id: to.id.clone(),
            operations,
            metadata: {
                let mut meta = HashMap::new();
                meta.insert(
                    "tick_delta".to_string(),
                    to.tick.saturating_sub(from.tick).to_string(),
                );
                meta.insert(
                    "time_delta".to_string(),
                    to.timestamp.saturating_sub(from.timestamp).to_string(),
                );
                meta
            },
        })
    }

    /// Apply incremental diff to a snapshot
    pub fn apply_incremental_diff(
        base: &UnifiedSnapshot,
        diff: &IncrementalDiff,
    ) -> Result<UnifiedSnapshot, StateError> {
        if base.id != diff.base_snapshot_id {
            return Err(StateError::InvalidState {
                reason: "Diff base snapshot ID does not match".to_string(),
            });
        }

        let mut result = base.clone();
        result.id = diff.target_snapshot_id.clone();

        // Apply operations
        for operation in &diff.operations {
            Self::apply_operation(&mut result, operation)?;
        }

        // Recalculate hash
        result.content_hash = Self::calculate_content_hash(&result);

        Ok(result)
    }

    /// Diff metadata maps
    fn diff_metadata(
        from_meta: &HashMap<String, String>,
        to_meta: &HashMap<String, String>,
    ) -> Vec<StateDiffEntry> {
        let mut changes = Vec::new();

        // Check for modified and removed keys
        for (key, old_value) in from_meta {
            match to_meta.get(key) {
                Some(new_value) if old_value != new_value => {
                    changes.push(StateDiffEntry {
                        path: format!("metadata.{}", key),
                        change_type: DiffChangeType::Modified,
                        old_value: Some(serde_json::Value::String(old_value.clone())),
                        new_value: Some(serde_json::Value::String(new_value.clone())),
                    });
                }
                None => {
                    changes.push(StateDiffEntry {
                        path: format!("metadata.{}", key),
                        change_type: DiffChangeType::Removed,
                        old_value: Some(serde_json::Value::String(old_value.clone())),
                        new_value: None,
                    });
                }
                _ => {} // No change
            }
        }

        // Check for added keys
        for (key, new_value) in to_meta {
            if !from_meta.contains_key(key) {
                changes.push(StateDiffEntry {
                    path: format!("metadata.{}", key),
                    change_type: DiffChangeType::Added,
                    old_value: None,
                    new_value: Some(serde_json::Value::String(new_value.clone())),
                });
            }
        }

        changes
    }

    /// Diff JSON values recursively
    fn diff_json_values(
        from: &serde_json::Value,
        to: &serde_json::Value,
        path: String,
    ) -> Result<Vec<StateDiffEntry>, StateError> {
        let mut changes = Vec::new();

        match (from, to) {
            (serde_json::Value::Object(from_obj), serde_json::Value::Object(to_obj)) => {
                // Diff object properties
                for (key, from_value) in from_obj {
                    let new_path = format!("{}.{}", path, key);
                    match to_obj.get(key) {
                        Some(to_value) => {
                            let nested_changes =
                                Self::diff_json_values(from_value, to_value, new_path)?;
                            changes.extend(nested_changes);
                        }
                        None => {
                            changes.push(StateDiffEntry {
                                path: new_path,
                                change_type: DiffChangeType::Removed,
                                old_value: Some(from_value.clone()),
                                new_value: None,
                            });
                        }
                    }
                }

                // Check for added properties
                for (key, to_value) in to_obj {
                    if !from_obj.contains_key(key) {
                        changes.push(StateDiffEntry {
                            path: format!("{}.{}", path, key),
                            change_type: DiffChangeType::Added,
                            old_value: None,
                            new_value: Some(to_value.clone()),
                        });
                    }
                }
            }
            (serde_json::Value::Array(from_arr), serde_json::Value::Array(to_arr)) => {
                // Simple array diff (could be enhanced with LCS algorithm)
                let max_len = from_arr.len().max(to_arr.len());
                for i in 0..max_len {
                    let new_path = format!("{}[{}]", path, i);
                    match (from_arr.get(i), to_arr.get(i)) {
                        (Some(from_value), Some(to_value)) => {
                            let nested_changes =
                                Self::diff_json_values(from_value, to_value, new_path)?;
                            changes.extend(nested_changes);
                        }
                        (Some(from_value), None) => {
                            changes.push(StateDiffEntry {
                                path: new_path,
                                change_type: DiffChangeType::Removed,
                                old_value: Some(from_value.clone()),
                                new_value: None,
                            });
                        }
                        (None, Some(to_value)) => {
                            changes.push(StateDiffEntry {
                                path: new_path,
                                change_type: DiffChangeType::Added,
                                old_value: None,
                                new_value: Some(to_value.clone()),
                            });
                        }
                        (None, None) => unreachable!(),
                    }
                }
            }
            _ => {
                // Different types or primitive values
                if from != to {
                    changes.push(StateDiffEntry {
                        path,
                        change_type: DiffChangeType::Modified,
                        old_value: Some(from.clone()),
                        new_value: Some(to.clone()),
                    });
                }
            }
        }

        Ok(changes)
    }

    /// Generate summary of changes
    fn generate_diff_summary(changes: &[StateDiffEntry]) -> DiffSummary {
        let mut added_count = 0;
        let mut removed_count = 0;
        let mut modified_count = 0;
        let mut affected_paths = std::collections::HashSet::new();

        for change in changes {
            affected_paths.insert(
                change
                    .path
                    .split('.')
                    .next()
                    .unwrap_or("unknown")
                    .to_string(),
            );

            match change.change_type {
                DiffChangeType::Added => added_count += 1,
                DiffChangeType::Removed => removed_count += 1,
                DiffChangeType::Modified => modified_count += 1,
            }
        }

        DiffSummary {
            total_changes: changes.len(),
            added_count,
            removed_count,
            modified_count,
            affected_root_paths: affected_paths.into_iter().collect(),
            has_significant_changes: !changes.is_empty(),
        }
    }

    /// Apply a single diff operation
    fn apply_operation(
        snapshot: &mut UnifiedSnapshot,
        operation: &DiffOperation,
    ) -> Result<(), StateError> {
        // This is a simplified implementation
        // A full implementation would need sophisticated JSON path modification
        match operation {
            DiffOperation::Set { path, value } => {
                if path == "tick" {
                    if let Some(tick_val) = value.as_u64() {
                        snapshot.tick = tick_val;
                    }
                }
                // Add more path handling as needed
            }
            DiffOperation::Remove { path: _ } => {
                // Implementation for removing values
            }
            DiffOperation::Add { path: _, value: _ } => {
                // Implementation for adding values
            }
        }

        Ok(())
    }

    /// Calculate content hash for a snapshot
    fn calculate_content_hash(snapshot: &UnifiedSnapshot) -> u64 {
        use std::hash::{Hash, Hasher};

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        snapshot.state_type.hash(&mut hasher);
        snapshot.tick.hash(&mut hasher);
        snapshot.data.to_string().hash(&mut hasher);
        hasher.finish()
    }
}

/// Comprehensive state difference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComprehensiveStateDiff {
    /// Source snapshot ID
    pub from_snapshot_id: String,
    /// Target snapshot ID
    pub to_snapshot_id: String,
    /// Type of state being compared
    pub state_type: String,
    /// Tick range covered by this diff
    pub tick_range: (u64, u64),
    /// List of all changes
    pub changes: Vec<StateDiffEntry>,
    /// Summary of changes
    pub summary: DiffSummary,
}

/// Individual state difference entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDiffEntry {
    /// Path to the changed value (dot-separated)
    pub path: String,
    /// Type of change
    pub change_type: DiffChangeType,
    /// Previous value (if any)
    pub old_value: Option<serde_json::Value>,
    /// New value (if any)
    pub new_value: Option<serde_json::Value>,
}

/// Type of change in a diff
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DiffChangeType {
    /// Value was added
    Added,
    /// Value was removed
    Removed,
    /// Value was modified
    Modified,
}

/// Summary of all changes in a diff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffSummary {
    /// Total number of changes
    pub total_changes: usize,
    /// Number of added values
    pub added_count: usize,
    /// Number of removed values
    pub removed_count: usize,
    /// Number of modified values
    pub modified_count: usize,
    /// Root paths that were affected
    pub affected_root_paths: Vec<String>,
    /// Whether there are significant changes
    pub has_significant_changes: bool,
}

/// Incremental diff that can be applied to transform one snapshot to another
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncrementalDiff {
    /// Base snapshot ID this diff applies to
    pub base_snapshot_id: String,
    /// Target snapshot ID this diff produces
    pub target_snapshot_id: String,
    /// Operations to apply
    pub operations: Vec<DiffOperation>,
    /// Metadata about the diff
    pub metadata: HashMap<String, String>,
}

/// Individual diff operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiffOperation {
    /// Set a value at a path
    Set {
        path: String,
        value: serde_json::Value,
    },
    /// Remove a value at a path
    Remove { path: String },
    /// Add a value at a path
    Add {
        path: String,
        value: serde_json::Value,
    },
}

impl DiffOperation {
    /// Convert a diff entry to an operation
    fn from_diff_entry(entry: StateDiffEntry) -> Self {
        match entry.change_type {
            DiffChangeType::Added => DiffOperation::Add {
                path: entry.path,
                value: entry.new_value.unwrap_or(serde_json::Value::Null),
            },
            DiffChangeType::Removed => DiffOperation::Remove { path: entry.path },
            DiffChangeType::Modified => DiffOperation::Set {
                path: entry.path,
                value: entry.new_value.unwrap_or(serde_json::Value::Null),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SnapshotBuilder;

    #[test]
    fn test_basic_diff_calculation() {
        let data1 = serde_json::json!({"value": 1, "name": "test"});
        let data2 = serde_json::json!({"value": 2, "name": "test", "new_field": "added"});

        let snapshot1 = SnapshotBuilder::new("TestState".to_string(), 1)
            .build(&data1)
            .unwrap();

        let snapshot2 = SnapshotBuilder::new("TestState".to_string(), 2)
            .build(&data2)
            .unwrap();

        let diff = StateDiffCalculator::calculate_diff(&snapshot1, &snapshot2).unwrap();

        assert!(!diff.changes.is_empty());
        assert!(diff.summary.has_significant_changes);
        assert_eq!(diff.summary.added_count, 1); // new_field
        assert_eq!(diff.summary.modified_count, 2); // tick and value
    }

    #[test]
    fn test_incremental_diff() {
        let data1 = serde_json::json!({"counter": 5});
        let data2 = serde_json::json!({"counter": 10});

        let snapshot1 = SnapshotBuilder::new("Counter".to_string(), 1)
            .build(&data1)
            .unwrap();

        let snapshot2 = SnapshotBuilder::new("Counter".to_string(), 2)
            .build(&data2)
            .unwrap();

        let diff = StateDiffCalculator::calculate_incremental_diff(&snapshot1, &snapshot2).unwrap();

        assert!(!diff.operations.is_empty());
        assert_eq!(diff.base_snapshot_id, snapshot1.id);
        assert_eq!(diff.target_snapshot_id, snapshot2.id);
    }
}
