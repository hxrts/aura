//! Standalone checkpoint manager for WorldState serialization
//!
//! This module provides a simple utility for saving and loading WorldState
//! snapshots without any coupling to the core simulation logic.

use crate::world_state::WorldState;
use crate::{Result, SimError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Standalone checkpoint manager for WorldState snapshots
///
/// This utility handles saving and loading WorldState objects to/from disk
/// without any knowledge of simulation internals. It's a pure I/O utility.
pub struct CheckpointManager {
    /// Directory where checkpoints are stored
    checkpoint_dir: PathBuf,
    /// In-memory checkpoint registry
    checkpoints: HashMap<String, CheckpointInfo>,
    /// Maximum number of checkpoints to keep
    max_checkpoints: usize,
}

/// Information about a saved checkpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointInfo {
    /// Unique checkpoint identifier
    pub id: String,
    /// User-provided label
    pub label: Option<String>,
    /// Tick when checkpoint was created
    pub tick: u64,
    /// Simulation time when checkpoint was created
    pub time: u64,
    /// File path where checkpoint is stored
    pub file_path: PathBuf,
    /// When checkpoint was created
    pub created_at: u64,
    /// Size of checkpoint file in bytes
    pub file_size: u64,
    /// Metadata about the simulation state
    pub metadata: CheckpointMetadata,
}

/// Metadata about the simulation state at checkpoint time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMetadata {
    /// Simulation seed
    pub seed: u64,
    /// Number of participants
    pub participant_count: usize,
    /// Number of active protocol sessions
    pub active_sessions: usize,
    /// Number of queued protocols
    pub queued_protocols: usize,
    /// Number of in-flight messages
    pub in_flight_messages: usize,
    /// Number of network partitions
    pub network_partitions: usize,
    /// Number of byzantine participants
    pub byzantine_participants: usize,
}

/// Serializable checkpoint data
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointData {
    /// Checkpoint metadata
    info: CheckpointInfo,
    /// Complete world state
    world_state: WorldState,
}

impl CheckpointManager {
    /// Create a new checkpoint manager
    pub fn new<P: AsRef<Path>>(checkpoint_dir: P) -> Result<Self> {
        let dir = checkpoint_dir.as_ref().to_path_buf();

        // Create directory if it doesn't exist
        if !dir.exists() {
            fs::create_dir_all(&dir).map_err(|e| {
                SimError::CheckpointError(format!("Failed to create checkpoint directory: {}", e))
            })?;
        }

        let mut manager = Self {
            checkpoint_dir: dir,
            checkpoints: HashMap::new(),
            max_checkpoints: 100,
        };

        // Load existing checkpoints
        manager.load_checkpoint_registry()?;

        Ok(manager)
    }

    /// Set maximum number of checkpoints to keep
    pub fn set_max_checkpoints(&mut self, max: usize) {
        self.max_checkpoints = max;
    }

    /// Save a world state as a checkpoint
    pub fn save(&mut self, world_state: &WorldState, label: Option<String>) -> Result<String> {
        let checkpoint_id = Uuid::new_v4().to_string();
        let file_name = format!("checkpoint_{}.json", checkpoint_id);
        let file_path = self.checkpoint_dir.join(&file_name);

        // Create checkpoint metadata
        let metadata = CheckpointMetadata {
            seed: world_state.seed,
            participant_count: world_state.participants.len(),
            active_sessions: world_state.protocols.active_sessions.len(),
            queued_protocols: world_state.protocols.execution_queue.len(),
            in_flight_messages: world_state.network.in_flight_messages.len(),
            network_partitions: world_state.network.partitions.len(),
            byzantine_participants: world_state.byzantine.byzantine_participants.len(),
        };

        let info = CheckpointInfo {
            id: checkpoint_id.clone(),
            label: label.clone(),
            tick: world_state.current_tick,
            time: world_state.current_time,
            file_path: file_path.clone(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            file_size: 0, // Will be updated after writing
            metadata,
        };

        let checkpoint_data = CheckpointData {
            info: info.clone(),
            world_state: world_state.clone(),
        };

        // Serialize and write to file
        let json = serde_json::to_string_pretty(&checkpoint_data).map_err(|e| {
            SimError::CheckpointError(format!("Failed to serialize checkpoint: {}", e))
        })?;

        fs::write(&file_path, &json).map_err(|e| {
            SimError::CheckpointError(format!("Failed to write checkpoint file: {}", e))
        })?;

        // Update file size
        let file_size = json.len() as u64;
        let mut final_info = info;
        final_info.file_size = file_size;

        // Store in registry
        self.checkpoints.insert(checkpoint_id.clone(), final_info);

        // Cleanup old checkpoints if necessary
        self.cleanup_old_checkpoints()?;

        // Save updated registry
        self.save_checkpoint_registry()?;

        Ok(checkpoint_id)
    }

    /// Load a world state from a checkpoint
    pub fn load(&self, checkpoint_id: &str) -> Result<WorldState> {
        let info = self
            .checkpoints
            .get(checkpoint_id)
            .ok_or_else(|| SimError::CheckpointError(format!("Checkpoint {} not found", checkpoint_id)))?;

        let json = fs::read_to_string(&info.file_path).map_err(|e| {
            SimError::CheckpointError(format!("Failed to read checkpoint file: {}", e))
        })?;

        let checkpoint_data: CheckpointData = serde_json::from_str(&json).map_err(|e| {
            SimError::CheckpointError(format!("Failed to deserialize checkpoint: {}", e))
        })?;

        Ok(checkpoint_data.world_state)
    }

    /// Get checkpoint information
    pub fn get_info(&self, checkpoint_id: &str) -> Option<&CheckpointInfo> {
        self.checkpoints.get(checkpoint_id)
    }

    /// List all available checkpoints
    pub fn list_checkpoints(&self) -> Vec<&CheckpointInfo> {
        let mut checkpoints: Vec<&CheckpointInfo> = self.checkpoints.values().collect();
        checkpoints.sort_by_key(|cp| cp.created_at);
        checkpoints
    }

    /// List checkpoints for a specific tick range
    pub fn list_checkpoints_in_range(&self, start_tick: u64, end_tick: u64) -> Vec<&CheckpointInfo> {
        let mut checkpoints: Vec<&CheckpointInfo> = self
            .checkpoints
            .values()
            .filter(|cp| cp.tick >= start_tick && cp.tick <= end_tick)
            .collect();
        checkpoints.sort_by_key(|cp| cp.tick);
        checkpoints
    }

    /// Delete a checkpoint
    pub fn delete(&mut self, checkpoint_id: &str) -> Result<()> {
        let info = self
            .checkpoints
            .remove(checkpoint_id)
            .ok_or_else(|| SimError::CheckpointError(format!("Checkpoint {} not found", checkpoint_id)))?;

        // Delete the file
        if info.file_path.exists() {
            fs::remove_file(&info.file_path).map_err(|e| {
                SimError::CheckpointError(format!("Failed to delete checkpoint file: {}", e))
            })?;
        }

        // Save updated registry
        self.save_checkpoint_registry()?;

        Ok(())
    }

    /// Delete all checkpoints
    pub fn clear_all(&mut self) -> Result<()> {
        for info in self.checkpoints.values() {
            if info.file_path.exists() {
                let _ = fs::remove_file(&info.file_path);
            }
        }

        self.checkpoints.clear();
        self.save_checkpoint_registry()?;

        Ok(())
    }

    /// Get total storage used by checkpoints
    pub fn get_storage_usage(&self) -> u64 {
        self.checkpoints.values().map(|cp| cp.file_size).sum()
    }

    /// Get checkpoint count
    pub fn checkpoint_count(&self) -> usize {
        self.checkpoints.len()
    }

    /// Find checkpoints with specific label
    pub fn find_by_label(&self, label: &str) -> Vec<&CheckpointInfo> {
        self.checkpoints
            .values()
            .filter(|cp| {
                cp.label
                    .as_ref()
                    .map(|l| l.contains(label))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Find the closest checkpoint to a specific tick
    pub fn find_closest_checkpoint(&self, target_tick: u64) -> Option<&CheckpointInfo> {
        self.checkpoints
            .values()
            .filter(|cp| cp.tick <= target_tick)
            .max_by_key(|cp| cp.tick)
    }

    /// Export checkpoint metadata to JSON
    pub fn export_metadata<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let metadata: Vec<&CheckpointInfo> = self.checkpoints.values().collect();
        let json = serde_json::to_string_pretty(&metadata).map_err(|e| {
            SimError::CheckpointError(format!("Failed to serialize metadata: {}", e))
        })?;

        fs::write(path, json).map_err(|e| {
            SimError::CheckpointError(format!("Failed to write metadata file: {}", e))
        })?;

        Ok(())
    }

    // Private helper methods

    /// Load checkpoint registry from disk
    fn load_checkpoint_registry(&mut self) -> Result<()> {
        let registry_path = self.checkpoint_dir.join("registry.json");

        if !registry_path.exists() {
            return Ok(()); // No existing registry
        }

        let json = fs::read_to_string(&registry_path).map_err(|e| {
            SimError::CheckpointError(format!("Failed to read checkpoint registry: {}", e))
        })?;

        let checkpoints: Vec<CheckpointInfo> = serde_json::from_str(&json).map_err(|e| {
            SimError::CheckpointError(format!("Failed to deserialize checkpoint registry: {}", e))
        })?;

        // Rebuild checkpoint map
        for info in checkpoints {
            // Verify file still exists
            if info.file_path.exists() {
                self.checkpoints.insert(info.id.clone(), info);
            }
        }

        Ok(())
    }

    /// Save checkpoint registry to disk
    fn save_checkpoint_registry(&self) -> Result<()> {
        let registry_path = self.checkpoint_dir.join("registry.json");
        let checkpoints: Vec<&CheckpointInfo> = self.checkpoints.values().collect();

        let json = serde_json::to_string_pretty(&checkpoints).map_err(|e| {
            SimError::CheckpointError(format!("Failed to serialize checkpoint registry: {}", e))
        })?;

        fs::write(&registry_path, json).map_err(|e| {
            SimError::CheckpointError(format!("Failed to write checkpoint registry: {}", e))
        })?;

        Ok(())
    }

    /// Clean up old checkpoints if we exceed the maximum
    fn cleanup_old_checkpoints(&mut self) -> Result<()> {
        if self.checkpoints.len() <= self.max_checkpoints {
            return Ok(());
        }

        // Sort by creation time, oldest first
        let mut checkpoints: Vec<(String, CheckpointInfo)> =
            self.checkpoints.drain().collect();
        checkpoints.sort_by_key(|(_, info)| info.created_at);

        // Keep only the most recent max_checkpoints
        let to_keep = checkpoints.len() - self.max_checkpoints;
        for (_, info) in checkpoints.iter().take(to_keep) {
            // Delete the file
            if info.file_path.exists() {
                let _ = fs::remove_file(&info.file_path);
            }
        }

        // Rebuild checkpoint map with remaining checkpoints
        for (id, info) in checkpoints.into_iter().skip(to_keep) {
            self.checkpoints.insert(id, info);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_world_state() -> WorldState {
        crate::test_utils::minimal_world_state()
    }

    #[test]
    fn test_checkpoint_manager_basic() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = CheckpointManager::new(temp_dir.path()).unwrap();

        let world_state = create_test_world_state();
        let checkpoint_id = manager
            .save(&world_state, Some("test checkpoint".to_string()))
            .unwrap();

        assert_eq!(manager.checkpoint_count(), 1);

        let loaded_world = manager.load(&checkpoint_id).unwrap();
        assert_eq!(loaded_world.seed, 42);
        assert_eq!(loaded_world.participants.len(), 1);
    }

    #[test]
    fn test_checkpoint_info() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = CheckpointManager::new(temp_dir.path()).unwrap();

        let world_state = create_test_world_state();
        let checkpoint_id = manager
            .save(&world_state, Some("labeled checkpoint".to_string()))
            .unwrap();

        let info = manager.get_info(&checkpoint_id).unwrap();
        assert_eq!(info.label, Some("labeled checkpoint".to_string()));
        assert_eq!(info.tick, 0);
        assert_eq!(info.metadata.participant_count, 1);
        assert!(info.file_size > 0);
    }

    #[test]
    fn test_checkpoint_listing() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = CheckpointManager::new(temp_dir.path()).unwrap();

        let mut world_state = create_test_world_state();
        
        // Create checkpoints at different ticks
        world_state.current_tick = 10;
        let _cp1 = manager.save(&world_state, Some("checkpoint 1".to_string())).unwrap();
        
        world_state.current_tick = 20;
        let _cp2 = manager.save(&world_state, Some("checkpoint 2".to_string())).unwrap();

        let all_checkpoints = manager.list_checkpoints();
        assert_eq!(all_checkpoints.len(), 2);

        let range_checkpoints = manager.list_checkpoints_in_range(15, 25);
        assert_eq!(range_checkpoints.len(), 1);
        assert_eq!(range_checkpoints[0].tick, 20);
    }

    #[test]
    fn test_closest_checkpoint() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = CheckpointManager::new(temp_dir.path()).unwrap();

        let mut world_state = create_test_world_state();
        
        world_state.current_tick = 10;
        let _cp1 = manager.save(&world_state, None).unwrap();
        
        world_state.current_tick = 30;
        let _cp2 = manager.save(&world_state, None).unwrap();

        let closest = manager.find_closest_checkpoint(25);
        assert!(closest.is_some());
        assert_eq!(closest.unwrap().tick, 10);

        let closest = manager.find_closest_checkpoint(35);
        assert!(closest.is_some());
        assert_eq!(closest.unwrap().tick, 30);
    }

    #[test]
    fn test_checkpoint_deletion() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = CheckpointManager::new(temp_dir.path()).unwrap();

        let world_state = create_test_world_state();
        let checkpoint_id = manager.save(&world_state, None).unwrap();

        assert_eq!(manager.checkpoint_count(), 1);

        manager.delete(&checkpoint_id).unwrap();
        assert_eq!(manager.checkpoint_count(), 0);
        assert!(manager.get_info(&checkpoint_id).is_none());
    }

    #[test]
    fn test_max_checkpoints_cleanup() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = CheckpointManager::new(temp_dir.path()).unwrap();
        manager.set_max_checkpoints(2);

        let world_state = create_test_world_state();

        // Create 3 checkpoints
        let _cp1 = manager.save(&world_state, Some("first".to_string())).unwrap();
        let _cp2 = manager.save(&world_state, Some("second".to_string())).unwrap();
        let _cp3 = manager.save(&world_state, Some("third".to_string())).unwrap();

        // Should only keep the last 2
        assert_eq!(manager.checkpoint_count(), 2);

        let remaining = manager.find_by_label("third");
        assert_eq!(remaining.len(), 1);
    }
}