//! Checkpoint management utilities

use super::{CheckpointId, CheckpointInfo, StateError, UnifiedSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Checkpoint strategy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointStrategy {
    /// Automatic checkpoint interval (0 = disabled)
    pub auto_interval: u64,
    /// Create checkpoints on significant events
    pub on_significant_events: bool,
    /// Create checkpoints before risky operations
    pub before_risky_operations: bool,
    /// Maximum checkpoint age before cleanup
    pub max_age_seconds: u64,
    /// Keep minimum number of checkpoints
    pub min_checkpoints: usize,
}

impl Default for CheckpointStrategy {
    fn default() -> Self {
        Self {
            auto_interval: 100,
            on_significant_events: true,
            before_risky_operations: true,
            max_age_seconds: 3600, // 1 hour
            min_checkpoints: 3,
        }
    }
}

/// Checkpoint metadata with extended information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendedCheckpointInfo {
    /// Basic checkpoint info
    pub info: CheckpointInfo,
    /// Strategy used to create this checkpoint
    pub strategy: CheckpointCreationReason,
    /// Additional context data
    pub context: HashMap<String, String>,
    /// Performance metrics when checkpoint was created
    pub performance_snapshot: Option<PerformanceSnapshot>,
    /// Tags for categorizing checkpoints
    pub tags: Vec<String>,
}

/// Reason why a checkpoint was created
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CheckpointCreationReason {
    /// Manual checkpoint requested by user
    Manual { label: Option<String> },
    /// Automatic interval-based checkpoint
    Automatic { interval: u64 },
    /// Checkpoint before significant event
    BeforeEvent { event_type: String },
    /// Checkpoint after significant event
    AfterEvent { event_type: String },
    /// Checkpoint before risky operation
    BeforeRiskyOperation { operation: String },
    /// Emergency checkpoint due to error
    Emergency { error_context: String },
}

/// Performance snapshot at checkpoint time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSnapshot {
    /// Memory usage in bytes
    pub memory_usage: usize,
    /// CPU utilization percentage
    pub cpu_utilization: f64,
    /// Simulation tick rate
    pub tick_rate: f64,
    /// Event processing rate
    pub event_rate: f64,
}

/// Checkpoint restoration options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreOptions {
    /// Validate state after restoration
    pub validate_after_restore: bool,
    /// Create backup before restore
    pub create_backup_before_restore: bool,
    /// Clear subsequent checkpoints
    pub clear_subsequent_checkpoints: bool,
    /// Restore specific components only
    pub selective_restore: Option<Vec<String>>,
}

impl Default for RestoreOptions {
    fn default() -> Self {
        Self {
            validate_after_restore: true,
            create_backup_before_restore: true,
            clear_subsequent_checkpoints: false,
            selective_restore: None,
        }
    }
}

/// Enhanced checkpoint manager with advanced features
#[derive(Debug, Clone)]
pub struct EnhancedCheckpointManager {
    /// Checkpoints with extended metadata
    checkpoints: HashMap<CheckpointId, (UnifiedSnapshot, ExtendedCheckpointInfo)>,
    /// Checkpoint strategy configuration
    strategy: CheckpointStrategy,
    /// Last automatic checkpoint tick
    last_auto_checkpoint: u64,
    /// Checkpoint creation history
    creation_history: Vec<CheckpointCreationReason>,
}

impl EnhancedCheckpointManager {
    /// Create new enhanced checkpoint manager
    pub fn new() -> Self {
        Self::with_strategy(CheckpointStrategy::default())
    }

    /// Create with custom strategy
    pub fn with_strategy(strategy: CheckpointStrategy) -> Self {
        Self {
            checkpoints: HashMap::new(),
            strategy,
            last_auto_checkpoint: 0,
            creation_history: Vec::new(),
        }
    }

    /// Create checkpoint with extended options
    pub fn create_checkpoint_extended(
        &mut self,
        snapshot: UnifiedSnapshot,
        reason: CheckpointCreationReason,
        context: HashMap<String, String>,
        tags: Vec<String>,
    ) -> Result<CheckpointId, StateError> {
        // Generate deterministic checkpoint ID from snapshot info
        let hash_input = format!("checkpoint-{}-{}", snapshot.tick, snapshot.timestamp);
        let hash_bytes = blake3::hash(hash_input.as_bytes());
        // SAFETY: blake3 hash is always 32 bytes, slice conversion to [u8; 16] always succeeds
        #[allow(clippy::expect_used)]
        let checkpoint_id = uuid::Uuid::from_bytes(
            hash_bytes.as_bytes()[..16]
                .try_into()
                .expect("blake3 hash is always 32 bytes, taking first 16 always succeeds"),
        )
        .to_string();

        let basic_info = CheckpointInfo {
            id: checkpoint_id.clone(),
            label: match &reason {
                CheckpointCreationReason::Manual { label } => label.clone(),
                _ => None,
            },
            tick: snapshot.tick,
            created_at: snapshot.timestamp,
            size_bytes: snapshot.size_bytes,
            content_hash: snapshot.content_hash,
        };

        let extended_info = ExtendedCheckpointInfo {
            info: basic_info,
            strategy: reason.clone(),
            context,
            performance_snapshot: None, // Would be populated with real metrics
            tags,
        };

        // Apply cleanup if at capacity
        self.cleanup_old_checkpoints()?;

        self.checkpoints
            .insert(checkpoint_id.clone(), (snapshot, extended_info));
        self.creation_history.push(reason);

        Ok(checkpoint_id)
    }

    /// Check if automatic checkpoint should be created
    pub fn should_create_auto_checkpoint(&self, current_tick: u64) -> bool {
        self.strategy.auto_interval > 0
            && current_tick >= self.last_auto_checkpoint + self.strategy.auto_interval
    }

    /// Create automatic checkpoint
    pub fn create_auto_checkpoint(
        &mut self,
        snapshot: UnifiedSnapshot,
    ) -> Result<CheckpointId, StateError> {
        let reason = CheckpointCreationReason::Automatic {
            interval: self.strategy.auto_interval,
        };

        let checkpoint_id = self.create_checkpoint_extended(
            snapshot.clone(),
            reason,
            HashMap::new(),
            vec!["auto".to_string()],
        )?;

        self.last_auto_checkpoint = snapshot.tick;
        Ok(checkpoint_id)
    }

    /// Create checkpoint before significant event
    pub fn create_event_checkpoint(
        &mut self,
        snapshot: UnifiedSnapshot,
        event_type: String,
        before: bool,
    ) -> Result<CheckpointId, StateError> {
        let reason = if before {
            CheckpointCreationReason::BeforeEvent {
                event_type: event_type.clone(),
            }
        } else {
            CheckpointCreationReason::AfterEvent {
                event_type: event_type.clone(),
            }
        };

        let mut context = HashMap::new();
        context.insert("event_type".to_string(), event_type.clone());
        context.insert(
            "timing".to_string(),
            if before { "before" } else { "after" }.to_string(),
        );

        self.create_checkpoint_extended(
            snapshot,
            reason,
            context,
            vec!["event".to_string(), event_type],
        )
    }

    /// Create emergency checkpoint
    pub fn create_emergency_checkpoint(
        &mut self,
        snapshot: UnifiedSnapshot,
        error_context: String,
    ) -> Result<CheckpointId, StateError> {
        let reason = CheckpointCreationReason::Emergency {
            error_context: error_context.clone(),
        };

        let mut context = HashMap::new();
        context.insert("error_context".to_string(), error_context);
        context.insert("emergency".to_string(), "true".to_string());

        self.create_checkpoint_extended(snapshot, reason, context, vec!["emergency".to_string()])
    }

    /// Get checkpoints by tag
    pub fn get_checkpoints_by_tag(&self, tag: &str) -> Vec<&ExtendedCheckpointInfo> {
        self.checkpoints
            .values()
            .map(|(_, info)| info)
            .filter(|info| info.tags.contains(&tag.to_string()))
            .collect()
    }

    /// Get checkpoints by creation reason
    pub fn get_checkpoints_by_reason(&self, reason_type: &str) -> Vec<&ExtendedCheckpointInfo> {
        self.checkpoints
            .values()
            .map(|(_, info)| info)
            .filter(|info| self.matches_reason_type(&info.strategy, reason_type))
            .collect()
    }

    /// Get checkpoint history summary
    pub fn get_history_summary(&self) -> CheckpointHistorySummary {
        let mut reason_counts = HashMap::new();
        for reason in &self.creation_history {
            let reason_type = self.reason_type_name(reason);
            *reason_counts.entry(reason_type).or_insert(0) += 1;
        }

        CheckpointHistorySummary {
            total_checkpoints: self.checkpoints.len(),
            active_checkpoints: self.checkpoints.len(),
            reason_distribution: reason_counts,
            oldest_checkpoint_age: self.get_oldest_checkpoint_age(),
            newest_checkpoint_age: self.get_newest_checkpoint_age(),
        }
    }

    /// Clean up old checkpoints based on strategy
    #[allow(clippy::disallowed_methods)]
    fn cleanup_old_checkpoints(&mut self) -> Result<(), StateError> {
        // SAFETY: SystemTime::now() will not be before UNIX_EPOCH on modern systems
        #[allow(clippy::expect_used)]
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("System time before UNIX epoch")
            .as_secs();

        // Remove checkpoints older than max age (but keep minimum)
        let mut to_remove = Vec::new();
        for (id, (_, info)) in &self.checkpoints {
            let age = current_time.saturating_sub(info.info.created_at);
            if age > self.strategy.max_age_seconds
                && self.checkpoints.len() > self.strategy.min_checkpoints
            {
                to_remove.push(id.clone());
            }
        }

        for id in to_remove {
            self.checkpoints.remove(&id);
        }

        Ok(())
    }

    /// Helper methods
    fn matches_reason_type(&self, reason: &CheckpointCreationReason, reason_type: &str) -> bool {
        matches!(
            (reason, reason_type),
            (CheckpointCreationReason::Manual { .. }, "manual")
                | (CheckpointCreationReason::Automatic { .. }, "automatic")
                | (CheckpointCreationReason::BeforeEvent { .. }, "before_event")
                | (CheckpointCreationReason::AfterEvent { .. }, "after_event")
                | (
                    CheckpointCreationReason::BeforeRiskyOperation { .. },
                    "before_risky"
                )
                | (CheckpointCreationReason::Emergency { .. }, "emergency")
        )
    }

    fn reason_type_name(&self, reason: &CheckpointCreationReason) -> String {
        match reason {
            CheckpointCreationReason::Manual { .. } => "manual".to_string(),
            CheckpointCreationReason::Automatic { .. } => "automatic".to_string(),
            CheckpointCreationReason::BeforeEvent { .. } => "before_event".to_string(),
            CheckpointCreationReason::AfterEvent { .. } => "after_event".to_string(),
            CheckpointCreationReason::BeforeRiskyOperation { .. } => "before_risky".to_string(),
            CheckpointCreationReason::Emergency { .. } => "emergency".to_string(),
        }
    }

    #[allow(clippy::disallowed_methods)]
    fn get_oldest_checkpoint_age(&self) -> Option<u64> {
        // SAFETY: SystemTime::now() will not be before UNIX_EPOCH on modern systems
        #[allow(clippy::expect_used)]
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("System time before UNIX epoch")
            .as_secs();

        self.checkpoints
            .values()
            .map(|(_, info)| current_time.saturating_sub(info.info.created_at))
            .min()
    }

    #[allow(clippy::disallowed_methods)]
    fn get_newest_checkpoint_age(&self) -> Option<u64> {
        // SAFETY: SystemTime::now() will not be before UNIX_EPOCH on modern systems
        #[allow(clippy::expect_used)]
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("System time before UNIX epoch")
            .as_secs();

        self.checkpoints
            .values()
            .map(|(_, info)| current_time.saturating_sub(info.info.created_at))
            .max()
    }
}

impl Default for EnhancedCheckpointManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary of checkpoint creation history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointHistorySummary {
    /// Total checkpoints ever created
    pub total_checkpoints: usize,
    /// Currently active checkpoints
    pub active_checkpoints: usize,
    /// Distribution of checkpoint creation reasons
    pub reason_distribution: HashMap<String, usize>,
    /// Age of oldest checkpoint in seconds
    pub oldest_checkpoint_age: Option<u64>,
    /// Age of newest checkpoint in seconds
    pub newest_checkpoint_age: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SnapshotBuilder;

    #[test]
    fn test_enhanced_checkpoint_manager() {
        let mut manager = EnhancedCheckpointManager::new();

        let snapshot = SnapshotBuilder::new("TestState".to_string(), 100)
            .build(&"test_data")
            .unwrap();

        let checkpoint_id = manager.create_auto_checkpoint(snapshot).unwrap();
        assert!(!checkpoint_id.is_empty());
        assert_eq!(manager.checkpoints.len(), 1);
    }

    #[test]
    fn test_checkpoint_tags_and_filtering() {
        let mut manager = EnhancedCheckpointManager::new();

        let snapshot = SnapshotBuilder::new("TestState".to_string(), 100)
            .build(&"test_data")
            .unwrap();

        manager
            .create_checkpoint_extended(
                snapshot,
                CheckpointCreationReason::Manual {
                    label: Some("test".to_string()),
                },
                HashMap::new(),
                vec!["important".to_string(), "milestone".to_string()],
            )
            .unwrap();

        let important_checkpoints = manager.get_checkpoints_by_tag("important");
        assert_eq!(important_checkpoints.len(), 1);

        let manual_checkpoints = manager.get_checkpoints_by_reason("manual");
        assert_eq!(manual_checkpoints.len(), 1);
    }
}
