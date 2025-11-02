//! Unified State Manager Implementation

use super::{
    CheckpointId, CheckpointInfo, CheckpointManager, SnapshotId, StateError, StateManager,
    StateSnapshot, UnifiedSnapshot,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Configuration for state management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateManagerConfig {
    /// Maximum number of snapshots to keep in memory
    pub max_snapshots: usize,
    /// Maximum number of checkpoints to keep
    pub max_checkpoints: usize,
    /// Automatic snapshot interval (0 = disabled)
    pub auto_snapshot_interval: u64,
    /// Maximum total memory usage for snapshots (bytes)
    pub max_memory_usage: usize,
    /// Enable compression for snapshots
    pub enable_compression: bool,
}

impl Default for StateManagerConfig {
    fn default() -> Self {
        Self {
            max_snapshots: 100,
            max_checkpoints: 50,
            auto_snapshot_interval: 100,
            max_memory_usage: 100 * 1024 * 1024, // 100MB
            enable_compression: true,
        }
    }
}

/// Unified state manager that handles snapshots and checkpoints
#[derive(Debug, Clone)]
pub struct UnifiedStateManager {
    /// Configuration for state management
    config: StateManagerConfig,
    /// Snapshots stored in chronological order
    snapshots: VecDeque<UnifiedSnapshot>,
    /// Checkpoints with metadata
    checkpoints: HashMap<CheckpointId, (UnifiedSnapshot, CheckpointInfo)>,
    /// Current memory usage estimate
    memory_usage: usize,
    /// Last auto-snapshot tick
    last_auto_snapshot: u64,
}

impl UnifiedStateManager {
    /// Create a new unified state manager
    pub fn new() -> Self {
        Self::with_config(StateManagerConfig::default())
    }

    /// Create state manager with custom configuration
    pub fn with_config(config: StateManagerConfig) -> Self {
        Self {
            config,
            snapshots: VecDeque::new(),
            checkpoints: HashMap::new(),
            memory_usage: 0,
            last_auto_snapshot: 0,
        }
    }

    /// Add a snapshot to the manager
    pub fn add_snapshot(&mut self, snapshot: UnifiedSnapshot) -> Result<SnapshotId, StateError> {
        // Check memory usage
        if self.memory_usage + snapshot.size_bytes > self.config.max_memory_usage {
            self.cleanup_old_snapshots()?;
        }

        self.memory_usage += snapshot.size_bytes;
        let id = snapshot.id.clone();

        self.snapshots.push_back(snapshot);

        // Limit number of snapshots
        while self.snapshots.len() > self.config.max_snapshots {
            if let Some(old_snapshot) = self.snapshots.pop_front() {
                self.memory_usage = self.memory_usage.saturating_sub(old_snapshot.size_bytes);
            }
        }

        Ok(id)
    }

    /// Get a snapshot by ID
    pub fn get_snapshot(&self, id: &SnapshotId) -> Option<&UnifiedSnapshot> {
        self.snapshots.iter().find(|s| s.id == *id)
    }

    /// Get the most recent snapshot
    pub fn latest_snapshot(&self) -> Option<&UnifiedSnapshot> {
        self.snapshots.back()
    }

    /// Get snapshots within a tick range
    pub fn get_snapshots_in_range(&self, start_tick: u64, end_tick: u64) -> Vec<&UnifiedSnapshot> {
        self.snapshots
            .iter()
            .filter(|s| s.tick >= start_tick && s.tick <= end_tick)
            .collect()
    }

    /// Check if auto-snapshot should be created
    pub fn should_auto_snapshot(&self, current_tick: u64) -> bool {
        self.config.auto_snapshot_interval > 0
            && current_tick >= self.last_auto_snapshot + self.config.auto_snapshot_interval
    }

    /// Mark that an auto-snapshot was created
    pub fn mark_auto_snapshot(&mut self, tick: u64) {
        self.last_auto_snapshot = tick;
    }

    /// Clean up old snapshots to free memory
    fn cleanup_old_snapshots(&mut self) -> Result<(), StateError> {
        // Remove oldest snapshots until we're under the memory limit
        let target_memory = self.config.max_memory_usage * 3 / 4; // Target 75% of max

        while self.memory_usage > target_memory && !self.snapshots.is_empty() {
            if let Some(old_snapshot) = self.snapshots.pop_front() {
                self.memory_usage = self.memory_usage.saturating_sub(old_snapshot.size_bytes);
            }
        }

        Ok(())
    }

    /// Get memory usage statistics
    pub fn memory_stats(&self) -> MemoryStats {
        MemoryStats {
            current_usage: self.memory_usage,
            max_usage: self.config.max_memory_usage,
            snapshot_count: self.snapshots.len(),
            checkpoint_count: self.checkpoints.len(),
        }
    }

    /// Get all snapshots (for debugging)
    pub fn all_snapshots(&self) -> &VecDeque<UnifiedSnapshot> {
        &self.snapshots
    }
}

impl<State: StateManager> CheckpointManager<State> for UnifiedStateManager {
    type Error = StateError;

    #[allow(clippy::disallowed_methods)]
    fn create_checkpoint(
        &mut self,
        state: &State,
        label: Option<String>,
    ) -> Result<CheckpointId, Self::Error> {
        // Check if we're at capacity
        if self.checkpoints.len() >= self.config.max_checkpoints {
            // Remove oldest checkpoint
            if let Some(oldest_id) = self
                .checkpoints
                .iter()
                .min_by_key(|(_, (_, info))| info.created_at)
                .map(|(id, _)| id.clone())
            {
                self.checkpoints.remove(&oldest_id);
            }
        }

        let snapshot = state.snapshot();
        // Generate deterministic checkpoint ID from state snapshot
        let hash_input = format!("checkpoint-{}", snapshot.tick());
        let hash_bytes = blake3::hash(hash_input.as_bytes());
        // SAFETY: blake3 hash is always 32 bytes, slice conversion to [u8; 16] always succeeds
        #[allow(clippy::expect_used)]
        let checkpoint_id = uuid::Uuid::from_bytes(
            hash_bytes.as_bytes()[..16]
                .try_into()
                .expect("blake3 hash is always 32 bytes, taking first 16 always succeeds"),
        )
        .to_string();

        // Convert state snapshot to unified snapshot
        let unified_snapshot = UnifiedSnapshot::new(
            std::any::type_name::<State>().to_string(),
            snapshot.tick(),
            &snapshot,
            HashMap::new(),
        )?;

        let checkpoint_info = CheckpointInfo {
            id: checkpoint_id.clone(),
            label,
            tick: snapshot.tick(),
            // SAFETY: SystemTime::now() will not be before UNIX_EPOCH on modern systems
            #[allow(clippy::expect_used)]
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("System time before UNIX epoch")
                .as_secs(),
            size_bytes: unified_snapshot.size_bytes,
            content_hash: unified_snapshot.content_hash,
        };

        self.checkpoints
            .insert(checkpoint_id.clone(), (unified_snapshot, checkpoint_info));

        Ok(checkpoint_id)
    }

    fn restore_checkpoint(
        &mut self,
        state: &mut State,
        checkpoint_id: &CheckpointId,
    ) -> Result<(), Self::Error> {
        let (unified_snapshot, _) =
            self.checkpoints
                .get(checkpoint_id)
                .ok_or_else(|| StateError::CheckpointNotFound {
                    id: checkpoint_id.clone(),
                })?;

        let snapshot: State::Snapshot = unified_snapshot.deserialize_data()?;
        state
            .restore(&snapshot)
            .map_err(|_| StateError::InvalidState {
                reason: "Failed to restore state from checkpoint".to_string(),
            })
    }

    fn list_checkpoints(&self) -> Vec<(CheckpointId, Option<String>, u64)> {
        self.checkpoints
            .values()
            .map(|(_, info)| (info.id.clone(), info.label.clone(), info.tick))
            .collect()
    }

    fn delete_checkpoint(&mut self, checkpoint_id: &CheckpointId) -> Result<(), Self::Error> {
        self.checkpoints
            .remove(checkpoint_id)
            .ok_or_else(|| StateError::CheckpointNotFound {
                id: checkpoint_id.clone(),
            })?;
        Ok(())
    }

    fn get_checkpoint_info(&self, checkpoint_id: &CheckpointId) -> Option<&CheckpointInfo> {
        self.checkpoints.get(checkpoint_id).map(|(_, info)| info)
    }
}

impl Default for UnifiedStateManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Memory usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    /// Current memory usage in bytes
    pub current_usage: usize,
    /// Maximum allowed memory usage in bytes
    pub max_usage: usize,
    /// Number of snapshots in memory
    pub snapshot_count: usize,
    /// Number of checkpoints stored
    pub checkpoint_count: usize,
}

impl MemoryStats {
    /// Get memory usage as a percentage
    pub fn usage_percentage(&self) -> f64 {
        if self.max_usage == 0 {
            0.0
        } else {
            (self.current_usage as f64 / self.max_usage as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestState {
        value: u64,
        data: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestSnapshot {
        tick: u64,
        timestamp: u64,
        state_hash: u64,
        content: TestState,
        metadata: HashMap<String, String>,
    }

    impl StateSnapshot for TestSnapshot {
        fn timestamp(&self) -> u64 {
            self.timestamp
        }
        fn tick(&self) -> u64 {
            self.tick
        }
        fn hash(&self) -> u64 {
            self.state_hash
        }
        fn metadata(&self) -> &HashMap<String, String> {
            &self.metadata
        }
        fn size_bytes(&self) -> usize {
            100
        }
    }

    impl StateManager for TestState {
        type Snapshot = TestSnapshot;
        type Error = StateError;

        fn snapshot(&self) -> Self::Snapshot {
            TestSnapshot {
                tick: 0,
                timestamp: 0,
                state_hash: self.value,
                content: self.clone(),
                metadata: HashMap::new(),
            }
        }

        fn restore(&mut self, snapshot: &Self::Snapshot) -> Result<(), Self::Error> {
            *self = snapshot.content.clone();
            Ok(())
        }

        fn validate(&self) -> Result<(), Self::Error> {
            Ok(())
        }

        fn state_hash(&self) -> u64 {
            self.value
        }
    }

    #[test]
    fn test_state_manager_creation() {
        let manager = UnifiedStateManager::new();
        assert_eq!(manager.snapshots.len(), 0);
        assert_eq!(manager.checkpoints.len(), 0);
    }

    #[test]
    fn test_checkpoint_creation() {
        let mut manager = UnifiedStateManager::new();
        let state = TestState {
            value: 42,
            data: "test".to_string(),
        };

        let checkpoint_id = manager
            .create_checkpoint(&state, Some("test".to_string()))
            .unwrap();
        assert!(!checkpoint_id.is_empty());
        assert_eq!(manager.checkpoints.len(), 1);
    }
}
