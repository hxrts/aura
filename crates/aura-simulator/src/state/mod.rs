//! Unified State Management System
//!
//! This module provides a unified framework for state management, checkpointing,
//! and snapshot capabilities across the simulation system.

pub mod checkpoint;
pub mod diff;
pub mod manager;
pub mod snapshot;

pub use checkpoint::*;
pub use diff::*;
pub use manager::*;
pub use snapshot::*;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Unique identifier for state snapshots
pub type SnapshotId = String;

/// Unique identifier for checkpoints
pub type CheckpointId = String;

/// State management trait for all state types
pub trait StateManager: Clone {
    /// Snapshot type produced by this state manager
    type Snapshot: StateSnapshot;
    /// Error type for state operations
    type Error;

    /// Create a snapshot of the current state
    fn snapshot(&self) -> Self::Snapshot;

    /// Restore state from a snapshot
    fn restore(&mut self, snapshot: &Self::Snapshot) -> Result<(), Self::Error>;

    /// Validate the current state
    fn validate(&self) -> Result<(), Self::Error>;

    /// Get a hash of the current state
    fn state_hash(&self) -> u64;

    /// Check if state has changed since last snapshot
    fn has_changed(&self, snapshot: &Self::Snapshot) -> bool {
        self.state_hash() != snapshot.hash()
    }
}

/// Trait for state snapshots
pub trait StateSnapshot: Clone + Serialize + for<'de> Deserialize<'de> {
    /// Get the timestamp when this snapshot was created
    fn timestamp(&self) -> u64;

    /// Get the simulation tick for this snapshot
    fn tick(&self) -> u64;

    /// Get a hash of the snapshot content
    fn hash(&self) -> u64;

    /// Get metadata about the snapshot
    fn metadata(&self) -> &HashMap<String, String>;

    /// Get the size of this snapshot in bytes (estimated)
    fn size_bytes(&self) -> usize;
}

/// Trait for checkpoint management
pub trait CheckpointManager<State: StateManager> {
    /// Error type for checkpoint operations
    type Error;

    /// Create a checkpoint with optional label
    fn create_checkpoint(
        &mut self,
        state: &State,
        label: Option<String>,
    ) -> Result<CheckpointId, Self::Error>;

    /// Restore state from a checkpoint
    fn restore_checkpoint(
        &mut self,
        state: &mut State,
        checkpoint_id: &CheckpointId,
    ) -> Result<(), Self::Error>;

    /// List all available checkpoints
    fn list_checkpoints(&self) -> Vec<(CheckpointId, Option<String>, u64)>;

    /// Delete a checkpoint
    fn delete_checkpoint(&mut self, checkpoint_id: &CheckpointId) -> Result<(), Self::Error>;

    /// Get checkpoint metadata
    fn get_checkpoint_info(&self, checkpoint_id: &CheckpointId) -> Option<&CheckpointInfo>;
}

/// Checkpoint metadata information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointInfo {
    /// Unique checkpoint identifier
    pub id: CheckpointId,
    /// Optional user-provided label
    pub label: Option<String>,
    /// Tick when checkpoint was created
    pub tick: u64,
    /// Timestamp when checkpoint was created
    pub created_at: u64,
    /// Size of the checkpoint in bytes
    pub size_bytes: usize,
    /// Hash of the checkpoint content
    pub content_hash: u64,
}

/// State difference calculation trait
pub trait StateDiff<T> {
    /// Type representing the difference between two states
    type Diff: Serialize + for<'de> Deserialize<'de>;

    /// Calculate difference between two states
    fn diff(&self, other: &T) -> Self::Diff;

    /// Apply a difference to create a new state
    fn apply_diff(&self, diff: &Self::Diff) -> T;
}

/// Unified snapshot that can represent different types of state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedSnapshot {
    /// Unique snapshot identifier
    pub id: SnapshotId,
    /// Timestamp when snapshot was created
    pub timestamp: u64,
    /// Simulation tick for this snapshot
    pub tick: u64,
    /// Type of state this snapshot represents
    pub state_type: String,
    /// Raw state data
    pub data: serde_json::Value,
    /// Hash of the snapshot content
    pub content_hash: u64,
    /// Metadata about the snapshot
    pub metadata: HashMap<String, String>,
    /// Estimated size in bytes
    pub size_bytes: usize,
}

impl StateSnapshot for UnifiedSnapshot {
    fn timestamp(&self) -> u64 {
        self.timestamp
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn hash(&self) -> u64 {
        self.content_hash
    }

    fn metadata(&self) -> &HashMap<String, String> {
        &self.metadata
    }

    fn size_bytes(&self) -> usize {
        self.size_bytes
    }
}

impl UnifiedSnapshot {
    /// Create a new unified snapshot
    pub fn new<T: Serialize>(
        state_type: String,
        tick: u64,
        data: &T,
        metadata: HashMap<String, String>,
    ) -> Result<Self, serde_json::Error> {
        // Generate deterministic timestamp and ID from state type and tick
        let hash_input = format!("{}-{}", state_type, tick);
        let hash_bytes = blake3::hash(hash_input.as_bytes());
        // SAFETY: blake3 hash is always 32 bytes, slice conversion to [u8; 8] always succeeds
        #[allow(clippy::expect_used)]
        let timestamp = u64::from_le_bytes(
            hash_bytes.as_bytes()[..8]
                .try_into()
                .expect("blake3 hash is always 32 bytes, taking first 8 always succeeds"),
        );

        // SAFETY: blake3 hash is always 32 bytes, slice conversion to [u8; 16] always succeeds
        #[allow(clippy::expect_used)]
        let id = uuid::Uuid::from_bytes(
            hash_bytes.as_bytes()[8..24]
                .try_into()
                .expect("blake3 hash is always 32 bytes, taking 8..24 always succeeds"),
        )
        .to_string();
        let serialized_data = serde_json::to_value(data)?;

        // Calculate hash
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        state_type.hash(&mut hasher);
        tick.hash(&mut hasher);
        serialized_data.to_string().hash(&mut hasher);
        let content_hash = hasher.finish();

        // Estimate size
        let size_bytes = serialized_data.to_string().len()
            + metadata
                .iter()
                .map(|(k, v)| k.len() + v.len())
                .sum::<usize>();

        Ok(Self {
            id,
            timestamp,
            tick,
            state_type,
            data: serialized_data,
            content_hash,
            metadata,
            size_bytes,
        })
    }

    /// Deserialize the data into a specific type
    pub fn deserialize_data<T: for<'de> Deserialize<'de>>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.data.clone())
    }

    /// Get the state type
    pub fn state_type(&self) -> &str {
        &self.state_type
    }
}

/// Error types for state management
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    /// Requested snapshot does not exist
    #[error("Snapshot not found: {id}")]
    SnapshotNotFound {
        /// Snapshot identifier
        id: String,
    },

    /// Requested checkpoint does not exist
    #[error("Checkpoint not found: {id}")]
    CheckpointNotFound {
        /// Checkpoint identifier
        id: String,
    },

    /// State validation failed
    #[error("Invalid state: {reason}")]
    InvalidState {
        /// Reason for validation failure
        reason: String,
    },

    /// Serialization or deserialization failed
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Storage operation failed
    #[error("Storage error: {reason}")]
    Storage {
        /// Reason for storage failure
        reason: String,
    },

    /// Storage capacity exceeded
    #[error("Capacity exceeded: {message}")]
    CapacityExceeded {
        /// Details about capacity violation
        message: String,
    },
}
